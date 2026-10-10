//! Ephemeral, explicitly accepted LAN assistance. History/backups never contain its grants.
#[cfg(feature = "desktop")]
pub mod commands;
pub mod http;
mod input;
pub mod model;
mod platform;
use crate::{db, peers::PeerManager};
use model::*;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub id: String,
    pub from: String,
    pub to: String,
    pub secret: String,
    pub sequence: u64,
    pub body: Wire,
}
#[derive(Clone, Serialize)]
pub struct Signal {
    pub sequence: u64,
    pub body: Wire,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Accept {
        screen: Option<Screen>,
        control: bool,
        voice: bool,
    },
    Stop {
        reason: String,
    },
    Ready,
    Description {
        kind: String,
        sdp: String,
    },
    Candidate {
        candidate: serde_json::Value,
    },
    RequestControl,
    ReleaseControl,
    Control {
        allow: bool,
    },
    Pause {
        paused: bool,
    },
    Screen {
        screen: Screen,
    },
    VoiceInvite,
    VoiceAnswer {
        id: String,
        accepted: bool,
    },
    VoiceEnd,
    Muted {
        muted: bool,
    },
    Quality {
        quality: Quality,
    },
}
struct Actor {
    native: bool,
    seen: Instant,
}
struct Session {
    view: View,
    secret: String,
    owner: Option<String>,
    peer_addr: String,
    incoming_sequence: u64,
    outgoing_sequence: u64,
    event_sequence: u64,
    events: VecDeque<Signal>,
    outgoing: VecDeque<Envelope>,
    created: Instant,
    accepted: Option<Instant>,
    peer_seen: Instant,
    owner_seen: Instant,
    last_heartbeat: Instant,
    local_ready: bool,
    peer_ready: bool,
    candidates: u16,
    last_input: Instant,
    voice_ringing: Option<Instant>,
}
impl Session {
    fn new(view: View, secret: String, owner: Option<String>, peer_addr: String) -> Self {
        let now = Instant::now();
        Self {
            view,
            secret,
            owner,
            peer_addr,
            incoming_sequence: 0,
            outgoing_sequence: 0,
            event_sequence: 0,
            events: VecDeque::new(),
            outgoing: VecDeque::new(),
            created: now,
            accepted: None,
            peer_seen: now,
            owner_seen: now,
            last_heartbeat: now,
            local_ready: false,
            peer_ready: false,
            candidates: 0,
            last_input: now,
            voice_ringing: None,
        }
    }
    fn queue(&mut self, from: &str, body: Wire) -> Result<()> {
        body.validate()?;
        if self.outgoing.len() >= 192 {
            return Err("远程信令队列已满，请重新连接".into());
        }
        self.outgoing_sequence += 1;
        self.outgoing.push_back(Envelope {
            id: self.view.id.clone(),
            from: from.into(),
            to: self.view.peer_id.clone(),
            secret: self.secret.clone(),
            sequence: self.outgoing_sequence,
            body,
        });
        Ok(())
    }
    fn event(&mut self, body: Wire) -> Result<()> {
        if self.events.len() >= 192 {
            return Err("远程页面没有及时处理连接信息".into());
        }
        self.event_sequence += 1;
        self.events.push_back(Signal {
            sequence: self.event_sequence,
            body,
        });
        Ok(())
    }
    fn ready(&mut self) {
        if self.local_ready && self.peer_ready && self.view.accepted() {
            self.view.phase = "active".into();
            self.view
                .started_at
                .get_or_insert_with(|| chrono::Utc::now().timestamp());
        }
    }
    fn expire_voice(&mut self, self_id: &str) {
        if self.view.voice.stage == "ringing"
            && self
                .voice_ringing
                .is_some_and(|started| started.elapsed() >= Duration::from_secs(30))
        {
            let id = self.view.voice.id.clone();
            self.view.voice = Voice::default();
            self.view.version += 1;
            self.voice_ringing = None;
            let _ = self.queue(self_id, Wire::VoiceEnd { id });
        }
    }
    fn stop(&mut self, reason: &str, self_id: &str, notify: bool) {
        if !self.view.live() {
            return;
        }
        self.view.stop(reason);
        self.events.clear();
        if notify {
            let _ = self.queue(
                self_id,
                Wire::Stop {
                    reason: reason.into(),
                },
            );
        }
    }
}
#[derive(Default)]
struct State {
    actors: HashMap<String, Actor>,
    session: Option<Session>,
    held: input::Held,
}
pub struct Hub {
    pool: Pool<Sqlite>,
    peers: Arc<PeerManager>,
    self_id: String,
    state: Mutex<State>,
    wake: tokio::sync::Notify,
    client: reqwest::Client,
    capture: Arc<tokio::sync::Semaphore>,
}
fn registry() -> &'static Mutex<HashMap<String, Arc<Hub>>> {
    static HUBS: OnceLock<Mutex<HashMap<String, Arc<Hub>>>> = OnceLock::new();
    HUBS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub async fn hub(pool: &Pool<Sqlite>, peers: &Arc<PeerManager>) -> Result<Arc<Hub>> {
    let scope = pool
        .connect_options()
        .get_filename()
        .to_string_lossy()
        .into_owned();
    if let Some(hub) = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&scope)
        .cloned()
    {
        return Ok(hub);
    }
    let candidate = Arc::new(Hub {
        pool: pool.clone(),
        peers: peers.clone(),
        self_id: db::get_user_id(pool).await.map_err(|e| e.to_string())?,
        state: Mutex::new(State::default()),
        wake: tokio::sync::Notify::new(),
        client: reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(4))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?,
        capture: Arc::new(tokio::sync::Semaphore::new(1)),
    });
    let mut all = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = all.get(&scope) {
        return Ok(existing.clone());
    }
    all.insert(scope, candidate.clone());
    tokio::spawn(candidate.clone().sender());
    tokio::spawn(candidate.clone().watchdog());
    Ok(candidate)
}
impl Hub {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    #[cfg(feature = "desktop")]
    fn native_actor(&self, actor: &str) -> Result<()> {
        if !self.lock().actors.get(actor).is_some_and(|a| a.native) {
            return Err("当前窗口没有原生共享权限".into());
        }
        Ok(())
    }
    fn native_session<'a>(state: &'a State, actor: &str, id: &str) -> Result<&'a Session> {
        let session = state
            .session
            .as_ref()
            .filter(|s| s.view.id == id && s.owner.as_deref() == Some(actor))
            .ok_or("远程会话所有者不匹配")?;
        if !state.actors.get(actor).is_some_and(|a| a.native)
            || !session.view.local_host
            || !session.view.native_host
            || !session.view.accepted()
            || session.view.paused
            || session.owner_seen.elapsed() > Duration::from_secs(8)
        {
            return Err("本机共享未获授权或已经停止".into());
        }
        Ok(session)
    }
    pub async fn frame(self: &Arc<Self>, actor: &str, id: &str, revision: u64) -> Result<Vec<u8>> {
        let _permit = self
            .capture
            .clone()
            .try_acquire_owned()
            .map_err(|_| "正在处理上一帧")?;
        let (screen, quality) = {
            let state = self.lock();
            let session = Self::native_session(&state, actor, id)
                .map_err(|e| format!("remote_state_changed: {e}"))?;
            if session.view.revision != revision {
                return Err("remote_state_changed: 屏幕授权已变化".into());
            }
            (
                session.view.screen.clone().ok_or("未选择共享屏幕")?,
                session.view.quality.clone(),
            )
        };
        let bytes = tokio::task::spawn_blocking(move || platform::frame(&screen, &quality))
            .await
            .map_err(|e| e.to_string())??;
        let state = self.lock();
        let session = Self::native_session(&state, actor, id)
            .map_err(|e| format!("remote_state_changed: {e}"))?;
        if session.view.revision != revision {
            return Err("remote_state_changed: 画面已过期".into());
        }
        Ok(bytes)
    }
    pub fn input(&self, actor: &str, id: &str, packet: input::Packet) -> Result<()> {
        let mut state = self.lock();
        let session = Self::native_session(&state, actor, id)?;
        if session.view.phase != "active" || session.peer_seen.elapsed() > Duration::from_secs(6) {
            return Err("远程连接未就绪或已失联".into());
        }
        let grant = session
            .view
            .grant
            .as_deref()
            .ok_or("本次鼠标键盘控制尚未授权")?;
        state.held.validate(&packet, grant)?;
        let screen = session.view.screen.clone().ok_or("共享屏幕缺失")?;
        if let Err(error) = platform::execute(&screen, &packet.event, &mut state.held) {
            let State { session, held, .. } = &mut *state;
            let session = session.as_mut().unwrap();
            if let Ok(wire) = session.view.host_state(false) {
                session.view.version += 1;
                let _ = session.queue(&self.self_id, wire);
            }
            let _ = platform::release(held);
            self.wake.notify_one();
            return Err(error);
        }
        state.held.record(&packet);
        state.session.as_mut().unwrap().last_input = Instant::now();
        Ok(())
    }
    pub fn bootstrap(&self, native: bool) -> Result<serde_json::Value> {
        let mut state = self.lock();
        let owner = state.session.as_ref().and_then(|s| s.owner.clone());
        state.actors.retain(|key, actor| {
            actor.seen.elapsed() < Duration::from_secs(30) || owner.as_ref() == Some(key)
        });
        if state.actors.len() >= 32 {
            return Err("远程协助页面过多，请关闭多余页面".into());
        }
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let native = native && platform::native_supported();
        state.actors.insert(
            token.clone(),
            Actor {
                native,
                seen: Instant::now(),
            },
        );
        Ok(serde_json::json!({"actor":token,"native_host":native,"self_id":self.self_id}))
    }
    pub fn poll(&self, actor: &str, id: Option<&str>, after: u64) -> Result<serde_json::Value> {
        let mut state = self.lock();
        state
            .actors
            .get_mut(actor)
            .ok_or("远程页面身份已失效")?
            .seen = Instant::now();
        let Some(session) = state.session.as_mut() else {
            return Ok(serde_json::json!({"session":null,"signals":[]}));
        };
        let owned = session.owner.as_deref() == Some(actor);
        if owned {
            session.owner_seen = Instant::now();
        }
        let signals = if owned && id == Some(session.view.id.as_str()) {
            while session.events.front().is_some_and(|e| e.sequence <= after) {
                session.events.pop_front();
            }
            session.events.iter().cloned().collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut view = session.view.clone();
        if !owned {
            view.grant = None;
        }
        Ok(serde_json::json!({"session":view,"owned":owned,"signals":signals}))
    }
    pub async fn start(&self, actor: &str, peer_id: &str, mut invitation: Invite) -> Result<View> {
        let native = {
            let state = self.lock();
            state.actors.get(actor).ok_or("远程页面身份已失效")?.native
        };
        invitation.native_host = native && invitation.mode == Mode::Help;
        Wire::Invite {
            invitation: invitation.clone(),
        }
        .validate()?;
        if peer_id == self.self_id {
            return Err("不能向自己发起远程协助".into());
        }
        if invitation.mode == Mode::Help && native {
            self.validate_screen(invitation.screen.as_ref().ok_or("请选择屏幕")?)
                .await?;
        }
        let address = crate::network::peer_connection::ensure_peer_connection(
            &self.pool,
            &self.peers,
            peer_id,
        )
        .await?;
        let name = self
            .peers
            .get_all_peers()
            .into_iter()
            .find(|p| p.id == peer_id)
            .map(|p| p.remark.filter(|s| !s.is_empty()).unwrap_or(p.name))
            .ok_or("对方设备不存在")?;
        let mut state = self.lock();
        if state.session.as_ref().is_some_and(|s| s.view.live()) {
            return Err("请先结束当前远程协助".into());
        }
        platform::release(&mut state.held)?;
        state.held.reset_sequence();
        let view = View::new(
            uuid::Uuid::new_v4().to_string(),
            peer_id.into(),
            name,
            true,
            invitation.clone(),
        );
        let mut session = Session::new(
            view,
            format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
            Some(actor.into()),
            address,
        );
        session.queue(&self.self_id, Wire::Invite { invitation })?;
        let view = session.view.clone();
        state.session = Some(session);
        self.wake.notify_one();
        Ok(view)
    }
    async fn validate_screen(&self, screen: &Screen) -> Result<()> {
        screen.validate()?;
        let wanted = screen.clone();
        let found = tokio::task::spawn_blocking(move || {
            platform::screens().map(|list| list.into_iter().any(|s| s == wanted))
        })
        .await
        .map_err(|e| e.to_string())??;
        if !found {
            return Err("所选屏幕已变化，请重新选择".into());
        }
        Ok(())
    }
    pub fn challenge(&self, id: &str, secret: &str, invitation: &Invite) -> bool {
        let state = self.lock();
        state.session.as_ref().is_some_and(|s| {
            s.view.initiator
                && s.view.live()
                && s.view.id == id
                && s.secret == secret
                && s.view.mode == invitation.mode
                && s.view.note == invitation.note
                && s.view.screen == invitation.screen
                && s.view.offered_control == invitation.control
                && s.view.offered_voice == invitation.voice
                && s.view.native_host == invitation.native_host
        })
    }
    pub async fn receive(&self, envelope: Envelope) -> Result<()> {
        if envelope.to != self.self_id
            || envelope.from == self.self_id
            || uuid::Uuid::parse_str(&envelope.id).is_err()
            || envelope.secret.len() != 64
        {
            return Err("远程请求身份无效".into());
        }
        envelope.body.validate()?;
        if let Wire::Invite { invitation } = &envelope.body {
            if envelope.sequence != 1 {
                return Err("请求序号无效".into());
            }
            {
                let state = self.lock();
                if let Some(session) = &state.session {
                    if session.view.id == envelope.id
                        && session.secret == envelope.secret
                        && session.view.peer_id == envelope.from
                    {
                        return Ok(());
                    }
                    if session.view.live() {
                        return Err("对方正在另一场远程协助中".into());
                    }
                }
            }
            let address = crate::network::peer_connection::ensure_peer_connection(
                &self.pool,
                &self.peers,
                &envelope.from,
            )
            .await?;
            let proof=self.client.post(format!("http://{address}/api/remote/peer/challenge")).json(&serde_json::json!({"id":envelope.id,"secret":envelope.secret,"invitation":invitation})).send().await.map_err(|_|"无法确认远程请求的来源")?;
            if !proof.status().is_success() {
                return Err("请求与对方当前会话不匹配".into());
            }
            let name = self
                .peers
                .get_all_peers()
                .into_iter()
                .find(|p| p.id == envelope.from)
                .map(|p| p.remark.filter(|s| !s.is_empty()).unwrap_or(p.name))
                .ok_or("请求方设备不存在")?;
            let mut state = self.lock();
            if state.session.as_ref().is_some_and(|s| s.view.live()) {
                return Err("当前已有远程协助请求".into());
            }
            platform::release(&mut state.held)?;
            state.held.reset_sequence();
            let mut session = Session::new(
                View::new(envelope.id, envelope.from, name, false, invitation.clone()),
                envelope.secret,
                None,
                address,
            );
            session.incoming_sequence = 1;
            state.session = Some(session);
            return Ok(());
        }
        let mut state = self.lock();
        let State { session, held, .. } = &mut *state;
        let session = session.as_mut().ok_or("远程会话已过期")?;
        if session.view.id != envelope.id
            || session.secret != envelope.secret
            || session.view.peer_id != envelope.from
        {
            return Err("远程请求不属于本次会话".into());
        }
        if envelope.sequence <= session.incoming_sequence {
            return Ok(());
        }
        if envelope.sequence != session.incoming_sequence + 1 {
            return Err("远程信令顺序不正确".into());
        }
        if !session.view.live() {
            return Ok(());
        }
        let previous = session.view.grant.clone();
        match envelope.body {
            Wire::Accept {
                screen,
                control,
                voice,
                native_host,
            } => {
                if !session.view.initiator {
                    return Err("请求发起方不能代替接收方接受".into());
                }
                if let Some(host) = session.view.accept(screen, control, voice, native_host)? {
                    session.queue(&self.self_id, host)?;
                }
                session.accepted = Some(Instant::now());
            }
            Wire::Stop { reason } => session.stop(&reason, &self.self_id, false),
            Wire::Heartbeat => (),
            body if !session.view.accepted() => {
                let _ = body;
                return Err("对方尚未同意远程协助".into());
            }
            Wire::Ready => {
                session.peer_ready = true;
                session.ready();
            }
            Wire::Description { kind, sdp } => session.event(Wire::Description { kind, sdp })?,
            Wire::Candidate { candidate } => {
                session.candidates += 1;
                if session.candidates > 128 {
                    return Err("候选地址数量超限".into());
                }
                session.event(Wire::Candidate { candidate })?;
            }
            Wire::ControlRequest => {
                session.view.require_host()?;
                if !session.view.paused && session.view.native_host && session.view.grant.is_none()
                {
                    session.view.control_requested = true;
                }
            }
            Wire::ReleaseControl => {
                let wire = session.view.host_state(false)?;
                session.queue(&self.self_id, wire)?;
            }
            Wire::HostState {
                revision,
                screen,
                paused,
                grant,
                native_host,
            } => session
                .view
                .apply_host(revision, screen, paused, grant, native_host)?,
            Wire::VoiceInvite { id } => {
                if session.view.voice.stage == "idle" {
                    session.voice_ringing = Some(Instant::now());
                    session.view.voice = Voice {
                        stage: "ringing".into(),
                        id,
                        local_caller: false,
                        ..Voice::default()
                    };
                } else {
                    session.queue(
                        &self.self_id,
                        Wire::VoiceAnswer {
                            id,
                            accepted: false,
                        },
                    )?;
                }
            }
            Wire::VoiceAnswer { id, accepted } => {
                let voice = &mut session.view.voice;
                if voice.id == id && voice.stage == "ringing" && voice.local_caller {
                    if accepted {
                        voice.stage = "active".into();
                        voice.started_at = Some(chrono::Utc::now().timestamp());
                    } else {
                        *voice = Voice::default();
                    }
                }
            }
            Wire::VoiceEnd { id } => {
                if session.view.voice.id == id {
                    session.view.voice = Voice::default();
                }
            }
            Wire::Muted { muted } => session.view.voice.peer_muted = muted,
            Wire::Quality { quality } => session.view.quality = quality,
            Wire::Invite { .. } => unreachable!(),
        }
        if previous != session.view.grant || !session.view.live() {
            let _ = platform::release(held);
            held.reset_sequence();
        }
        session.incoming_sequence = envelope.sequence;
        session.view.version += 1;
        session.peer_seen = Instant::now();
        self.wake.notify_one();
        Ok(())
    }
    pub async fn action(&self, actor: &str, id: &str, action: Action) -> Result<View> {
        let native = {
            let state = self.lock();
            state.actors.get(actor).ok_or("远程页面身份已失效")?.native
        };
        if native {
            match &action {
                Action::Accept {
                    screen: Some(screen),
                    ..
                }
                | Action::Screen { screen } => self.validate_screen(screen).await?,
                _ => (),
            }
        }
        let mut state = self.lock();
        let State { session, held, .. } = &mut *state;
        let session = session
            .as_mut()
            .filter(|s| s.view.id == id)
            .ok_or("远程会话已过期")?;
        let accepting = matches!(action, Action::Accept { .. })
            || matches!(action,Action::Stop{ref reason} if reason=="rejected");
        if session.owner.as_deref() != Some(actor)
            && !(accepting
                && !session.view.initiator
                && session.view.phase == "waiting"
                && session.owner.is_none())
        {
            return Err("此会话由另一个窗口处理".into());
        }
        if !session.view.live() {
            return Err("这次远程协助已结束".into());
        }
        let previous = session.view.grant.clone();
        let wire = match action {
            Action::Accept {
                screen,
                control,
                voice,
            } => {
                if session.view.initiator {
                    return Err("请等待对方回应".into());
                }
                let host = session
                    .view
                    .accept(screen.clone(), control, voice, native)?;
                session.owner = Some(actor.into());
                session.accepted = Some(Instant::now());
                session.queue(
                    &self.self_id,
                    Wire::Accept {
                        screen,
                        control,
                        voice,
                        native_host: native,
                    },
                )?;
                host
            }
            Action::Stop { reason } => {
                let body = Wire::Stop { reason };
                body.validate()?;
                if let Wire::Stop { reason } = &body {
                    session.stop(reason, &self.self_id, false);
                }
                Some(body)
            }
            _ if !session.view.accepted() => return Err("对方尚未同意远程协助".into()),
            Action::Ready => {
                session.local_ready = true;
                session.ready();
                Some(Wire::Ready)
            }
            Action::Description { kind, sdp } => Some(Wire::Description { kind, sdp }),
            Action::Candidate { candidate } => Some(Wire::Candidate { candidate }),
            Action::RequestControl => {
                if session.view.local_host
                    || session.view.paused
                    || !session.view.native_host
                    || session.view.grant.is_some()
                {
                    return Err("当前无法申请控制".into());
                }
                session.view.control_requested = true;
                Some(Wire::ControlRequest)
            }
            Action::ReleaseControl => {
                if session.view.local_host {
                    return Err("共享方请使用收回控制".into());
                }
                session.view.grant = None;
                Some(Wire::ReleaseControl)
            }
            Action::Control { allow } => {
                session.view.require_host()?;
                if allow && !session.view.control_requested {
                    return Err("没有待确认的控制请求".into());
                }
                Some(session.view.host_state(allow)?)
            }
            Action::Pause { paused } => {
                session.view.require_host()?;
                session.view.paused = paused;
                Some(session.view.host_state(false)?)
            }
            Action::Screen { screen } => {
                session.view.require_host()?;
                screen.validate()?;
                session.view.screen = Some(screen);
                Some(session.view.host_state(false)?)
            }
            Action::VoiceInvite => {
                if session.view.voice.stage != "idle" {
                    return Err("当前已经有语音通话或邀请".into());
                }
                let id = uuid::Uuid::new_v4().to_string();
                session.view.voice = Voice {
                    stage: "ringing".into(),
                    id: id.clone(),
                    local_caller: true,
                    ..Voice::default()
                };
                session.voice_ringing = Some(Instant::now());
                Some(Wire::VoiceInvite { id })
            }
            Action::VoiceAnswer { id, accepted } => {
                let voice = &mut session.view.voice;
                if voice.id != id || voice.stage != "ringing" || voice.local_caller {
                    return Err("语音邀请已过期".into());
                }
                if accepted {
                    voice.stage = "active".into();
                    voice.started_at = Some(chrono::Utc::now().timestamp());
                } else {
                    *voice = Voice::default();
                }
                Some(Wire::VoiceAnswer { id, accepted })
            }
            Action::VoiceEnd => {
                let id = session.view.voice.id.clone();
                session.view.voice = Voice::default();
                if id.is_empty() {
                    None
                } else {
                    Some(Wire::VoiceEnd { id })
                }
            }
            Action::Muted { muted } => {
                session.view.voice.local_muted = muted;
                Some(Wire::Muted { muted })
            }
            Action::Quality { quality } => {
                quality.validate()?;
                session.view.quality = quality.clone();
                Some(Wire::Quality { quality })
            }
        };
        if previous != session.view.grant || !session.view.live() {
            let _ = platform::release(held);
            held.reset_sequence();
        }
        if let Some(wire) = wire {
            session.queue(&self.self_id, wire)?;
        }
        session.owner_seen = Instant::now();
        session.view.version += 1;
        self.wake.notify_one();
        Ok(session.view.clone())
    }
    async fn sender(self: Arc<Self>) {
        loop {
            if self.pool.is_closed() {
                break;
            }
            let next = {
                let state = self.lock();
                state
                    .session
                    .as_ref()
                    .and_then(|s| s.outgoing.front().map(|e| (s.peer_addr.clone(), e.clone())))
            };
            let Some((address, envelope)) = next else {
                tokio::select! {_=self.wake.notified()=>(),_=tokio::time::sleep(Duration::from_secs(1))=>()};
                continue;
            };
            let mut sent = false;
            for _ in 0..2 {
                match self
                    .client
                    .post(format!("http://{address}/api/remote/peer/signal"))
                    .json(&envelope)
                    .send()
                    .await
                {
                    Ok(response) if response.status().is_success() => {
                        sent = true;
                        break;
                    }
                    Ok(_) => break,
                    Err(_) => tokio::time::sleep(Duration::from_millis(150)).await,
                }
            }
            let mut state = self.lock();
            let State { session, held, .. } = &mut *state;
            if let Some(session) = session.as_mut().filter(|s| s.view.id == envelope.id) {
                if sent {
                    session.outgoing.pop_front();
                } else {
                    session.stop("disconnected", &self.self_id, false);
                    session.outgoing.clear();
                    let _ = platform::release(held);
                    held.reset_sequence();
                }
            }
        }
    }
    async fn watchdog(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_millis(400)).await;
            if self.pool.is_closed() {
                let mut state = self.lock();
                let _ = platform::release(&mut state.held);
                break;
            }
            let locked = !platform::interactive();
            let mut state = self.lock();
            let State { session, held, .. } = &mut *state;
            if let Some(session) = session.as_mut() {
                if session.view.live() {
                    session.expire_voice(&self.self_id);
                    let reason = if session.view.local_host && session.view.native_host && locked {
                        Some("locked")
                    } else if session.view.phase == "waiting"
                        && session.created.elapsed() > Duration::from_secs(60)
                    {
                        Some("expired")
                    } else if session.owner.is_some()
                        && session.owner_seen.elapsed() > Duration::from_secs(8)
                    {
                        Some("disconnected")
                    } else if session.view.accepted()
                        && session.peer_seen.elapsed() > Duration::from_secs(12)
                    {
                        Some("disconnected")
                    } else if session.view.phase == "connecting"
                        && session
                            .accepted
                            .is_some_and(|t| t.elapsed() > Duration::from_secs(40))
                    {
                        Some("disconnected")
                    } else {
                        None
                    };
                    if let Some(reason) = reason {
                        session.stop(reason, &self.self_id, true);
                        self.wake.notify_one();
                    }
                    if session.view.live()
                        && session.last_heartbeat.elapsed() > Duration::from_secs(2)
                    {
                        let _ = session.queue(&self.self_id, Wire::Heartbeat);
                        session.last_heartbeat = Instant::now();
                        self.wake.notify_one();
                    }
                }
                if !session.view.live()
                    || session.view.grant.is_none()
                    || session.last_input.elapsed() > Duration::from_secs(2)
                {
                    let _ = platform::release(held);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let screen = Screen {
            id: "display".into(),
            name: "Display".into(),
            width: 1920,
            height: 1080,
        };
        let mut view = View::new(
            uuid::Uuid::new_v4().to_string(),
            "peer".into(),
            "Peer".into(),
            true,
            Invite {
                mode: Mode::Help,
                note: String::new(),
                screen: Some(screen),
                control: false,
                voice: false,
                native_host: false,
            },
        );
        view.accept(None, false, false, false).unwrap();
        Session::new(
            view,
            "s".repeat(64),
            Some("owner".into()),
            "127.0.0.1:9".into(),
        )
    }

    #[test]
    fn unanswered_voice_expires_without_ending_screen_or_active_call() {
        let mut session = session();
        let call_id = uuid::Uuid::new_v4().to_string();
        session.view.voice = Voice {
            stage: "ringing".into(),
            id: call_id.clone(),
            ..Voice::default()
        };
        session.voice_ringing = Some(Instant::now() - Duration::from_secs(31));
        session.expire_voice("self");
        assert_eq!(session.view.voice.stage, "idle");
        assert!(session.view.accepted());
        assert!(
            matches!(&session.outgoing.back().unwrap().body, Wire::VoiceEnd { id } if id == &call_id)
        );
        session.view.voice.stage = "active".into();
        session.voice_ringing = Some(Instant::now() - Duration::from_secs(31));
        session.expire_voice("self");
        assert_eq!(session.view.voice.stage, "active");
    }

    #[tokio::test]
    async fn owner_and_terminal_state_fence_late_commands_and_signals() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_lazy("sqlite::memory:")
            .unwrap();
        let hub = Hub {
            pool,
            peers: Arc::new(PeerManager::new()),
            self_id: "self".into(),
            state: Mutex::new(State::default()),
            wake: tokio::sync::Notify::new(),
            client: reqwest::Client::new(),
            capture: Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let session = session();
        let id = session.view.id.clone();
        {
            let mut state = hub.lock();
            for key in ["owner", "observer"] {
                state.actors.insert(
                    key.into(),
                    Actor {
                        native: false,
                        seen: Instant::now(),
                    },
                );
            }
            state.session = Some(session);
        }
        assert!(hub
            .action("observer", &id, Action::Pause { paused: true })
            .await
            .is_err());
        assert!(hub
            .action("owner", &id, Action::Control { allow: true })
            .await
            .is_err());
        hub.action(
            "owner",
            &id,
            Action::Stop {
                reason: "ended".into(),
            },
        )
        .await
        .unwrap();
        assert!(hub.action("owner", &id, Action::Ready).await.is_err());
        hub.receive(Envelope {
            id: id.clone(),
            from: "peer".into(),
            to: "self".into(),
            secret: "s".repeat(64),
            sequence: 1,
            body: Wire::Ready,
        })
        .await
        .unwrap();
        let state = hub.poll("observer", Some(&id), 0).unwrap();
        assert_eq!(state["session"]["phase"], "ended");
        assert_eq!(state["owned"], false);
        assert_eq!(state["session"]["grant"], serde_json::Value::Null);
        assert_eq!(state["signals"], serde_json::json!([]));
    }
}
