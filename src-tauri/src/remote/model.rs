//! Consent state is shared by both transports. No operating-system work occurs here.
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Help,
    Control,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
}
impl Screen {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 100
            || self.name.len() > 200
            || self.width == 0
            || self.height == 0
            || self.width > 32768
            || self.height > 32768
        {
            return Err("共享屏幕信息无效".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    pub preset: String,
    pub fps: u8,
    pub reduced_color: bool,
}
impl Default for Quality {
    fn default() -> Self {
        Self {
            preset: "auto".into(),
            fps: 20,
            reduced_color: false,
        }
    }
}
impl Quality {
    pub fn validate(&self) -> Result<()> {
        if !["auto", "fluent", "clear"].contains(&self.preset.as_str())
            || ![10, 20, 30].contains(&self.fps)
        {
            return Err("画质设置无效".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invite {
    pub mode: Mode,
    pub note: String,
    pub screen: Option<Screen>,
    pub control: bool,
    pub voice: bool,
    pub native_host: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Wire {
    Invite {
        invitation: Invite,
    },
    Accept {
        screen: Option<Screen>,
        control: bool,
        voice: bool,
        native_host: bool,
    },
    Stop {
        reason: String,
    },
    Ready,
    Heartbeat,
    Description {
        kind: String,
        sdp: String,
    },
    Candidate {
        candidate: serde_json::Value,
    },
    ControlRequest,
    ReleaseControl,
    HostState {
        revision: u64,
        screen: Screen,
        paused: bool,
        grant: Option<String>,
        native_host: bool,
    },
    VoiceInvite {
        id: String,
    },
    VoiceAnswer {
        id: String,
        accepted: bool,
    },
    VoiceEnd {
        id: String,
    },
    Muted {
        muted: bool,
    },
    Quality {
        quality: Quality,
    },
}
impl Wire {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Invite { invitation } => {
                if invitation.note.chars().count() > 200 {
                    return Err("协助说明最多 200 字".into());
                }
                if invitation.mode == Mode::Help && invitation.screen.is_none() {
                    return Err("请先选择共享屏幕".into());
                }
                if invitation.control && (!invitation.native_host || invitation.mode != Mode::Help)
                {
                    return Err("此共享方式不支持操作本机".into());
                }
                if let Some(screen) = &invitation.screen {
                    screen.validate()?;
                }
            }
            Self::Accept { screen, .. } => {
                if let Some(screen) = screen {
                    screen.validate()?;
                }
            }
            Self::Description { kind, sdp } => {
                if !["offer", "answer"].contains(&kind.as_str())
                    || sdp.len() > 65536
                    || !sdp.starts_with("v=0")
                {
                    return Err("会话描述无效".into());
                }
            }
            Self::Candidate { candidate } => {
                if !candidate.is_object() || candidate.to_string().len() > 4096 {
                    return Err("连接候选地址无效".into());
                }
            }
            Self::HostState { screen, grant, .. } => {
                screen.validate()?;
                if grant
                    .as_ref()
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_err())
                {
                    return Err("控制授权无效".into());
                }
            }
            Self::VoiceInvite { id } | Self::VoiceAnswer { id, .. } | Self::VoiceEnd { id } => {
                if uuid::Uuid::parse_str(id).is_err() {
                    return Err("语音通话标识无效".into());
                }
            }
            Self::Stop { reason }
                if ![
                    "ended",
                    "cancelled",
                    "rejected",
                    "disconnected",
                    "locked",
                    "expired",
                ]
                .contains(&reason.as_str()) =>
            {
                return Err("结束原因无效".into())
            }
            Self::Quality { quality } => quality.validate()?,
            _ => (),
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Voice {
    pub stage: String,
    pub id: String,
    pub local_caller: bool,
    pub local_muted: bool,
    pub peer_muted: bool,
    pub started_at: Option<i64>,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            stage: "idle".into(),
            id: String::new(),
            local_caller: false,
            local_muted: false,
            peer_muted: false,
            started_at: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct View {
    pub version: u64,
    pub id: String,
    pub peer_id: String,
    pub peer_name: String,
    pub mode: Mode,
    pub initiator: bool,
    pub local_host: bool,
    pub phase: String,
    pub note: String,
    pub screen: Option<Screen>,
    pub offered_control: bool,
    pub offered_voice: bool,
    pub native_host: bool,
    pub grant: Option<String>,
    pub revision: u64,
    pub control_requested: bool,
    pub paused: bool,
    pub voice: Voice,
    pub quality: Quality,
    pub created_at: i64,
    pub started_at: Option<i64>,
}

impl View {
    pub fn new(
        id: String,
        peer_id: String,
        peer_name: String,
        initiator: bool,
        request: Invite,
    ) -> Self {
        Self {
            version: 1,
            id,
            peer_id,
            peer_name,
            mode: request.mode,
            initiator,
            local_host: initiator == (request.mode == Mode::Help),
            phase: "waiting".into(),
            note: request.note,
            screen: request.screen,
            offered_control: request.control,
            offered_voice: request.voice,
            native_host: request.native_host,
            grant: None,
            revision: 0,
            control_requested: false,
            paused: false,
            voice: Voice::default(),
            quality: Quality::default(),
            created_at: chrono::Utc::now().timestamp(),
            started_at: None,
        }
    }
    pub fn live(&self) -> bool {
        ["waiting", "connecting", "active"].contains(&self.phase.as_str())
    }
    pub fn accepted(&self) -> bool {
        ["connecting", "active"].contains(&self.phase.as_str())
    }
    pub fn stop(&mut self, reason: &str) {
        if !self.live() {
            return;
        }
        self.phase = reason.into();
        self.version += 1;
        self.grant = None;
        self.control_requested = false;
        self.paused = true;
        self.voice = Voice::default();
        self.revision += 1;
    }
    pub fn require_host(&self) -> Result<()> {
        if !self.local_host || !self.accepted() {
            return Err("只有共享方可更改本次屏幕授权".into());
        }
        Ok(())
    }
    pub fn host_state(&mut self, control: bool) -> Result<Wire> {
        self.require_host()?;
        if control && (!self.native_host || self.paused) {
            return Err("当前仅允许查看共享画面".into());
        }
        self.grant = control.then(|| uuid::Uuid::new_v4().to_string());
        self.control_requested = false;
        self.revision += 1;
        Ok(Wire::HostState {
            revision: self.revision,
            screen: self.screen.clone().ok_or("共享屏幕缺失")?,
            paused: self.paused,
            grant: self.grant.clone(),
            native_host: self.native_host,
        })
    }
    /// Called only after the local recipient has explicitly accepted, or a valid peer Accept arrives.
    pub fn accept(
        &mut self,
        screen: Option<Screen>,
        control: bool,
        voice: bool,
        native: bool,
    ) -> Result<Option<Wire>> {
        if self.phase != "waiting" {
            return Err("请求已经结束或被回应".into());
        }
        if self.mode == Mode::Control {
            screen.as_ref().ok_or("请先选择共享屏幕")?.validate()?;
            self.screen = screen;
            self.native_host = native;
        }
        if control && !self.native_host {
            return Err("该共享端只能提供画面查看".into());
        }
        self.phase = "connecting".into();
        if self.offered_voice && voice {
            self.voice.stage = "active".into();
            self.voice.id = self.id.clone();
            self.voice.local_caller = self.initiator;
            self.voice.started_at = Some(chrono::Utc::now().timestamp());
        }
        if self.local_host {
            self.host_state(if self.mode == Mode::Help {
                self.offered_control
            } else {
                control
            })
            .map(Some)
        } else {
            Ok(None)
        }
    }
    pub fn apply_host(
        &mut self,
        revision: u64,
        screen: Screen,
        paused: bool,
        grant: Option<String>,
        native: bool,
    ) -> Result<()> {
        if self.local_host || !self.accepted() {
            return Err("对方不能代替本机授予权限".into());
        }
        if revision <= self.revision {
            return Ok(());
        }
        if grant.is_some() && (paused || !native) {
            return Err("控制授权与屏幕状态不一致".into());
        }
        self.revision = revision;
        self.screen = Some(screen);
        self.paused = paused;
        self.grant = grant;
        self.native_host = native;
        self.control_requested = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn invite(mode: Mode) -> Invite {
        Invite {
            mode,
            note: String::new(),
            screen: Some(Screen {
                id: "1".into(),
                name: "主屏幕".into(),
                width: 1920,
                height: 1080,
            }),
            control: mode == Mode::Help,
            voice: true,
            native_host: true,
        }
    }
    fn view(mode: Mode, initiator: bool) -> View {
        View::new(
            uuid::Uuid::new_v4().to_string(),
            "peer".into(),
            "peer".into(),
            initiator,
            invite(mode),
        )
    }
    #[test]
    fn both_directions_require_consent_and_never_reuse_control() {
        for (mode, initiator) in [(Mode::Help, true), (Mode::Control, false)] {
            let mut host = view(mode, initiator);
            assert!(host.host_state(true).is_err());
            assert!(host.grant.is_none());
            host.accept(invite(mode).screen, true, true, true).unwrap();
            let original = host.grant.clone().unwrap();
            host.paused = true;
            host.host_state(false).unwrap();
            assert!(host.grant.is_none());
            host.paused = false;
            host.host_state(false).unwrap();
            assert!(host.grant.is_none());
            host.host_state(true).unwrap();
            assert_ne!(host.grant.as_ref(), Some(&original));
            host.stop("disconnected");
            assert!(host.grant.is_none());
            assert_eq!(host.voice.stage, "idle");
            assert!(host.accept(invite(mode).screen, true, true, true).is_err());
        }
    }
    #[test]
    fn remote_cannot_grant_on_host_and_old_revision_cannot_undo_revoke() {
        let mut host = view(Mode::Help, true);
        host.accept(None, true, false, true).unwrap();
        let screen = host.screen.clone().unwrap();
        assert!(host
            .apply_host(
                100,
                screen.clone(),
                false,
                Some(uuid::Uuid::new_v4().to_string()),
                true
            )
            .is_err());
        let mut viewer = view(Mode::Help, false);
        viewer.accept(None, false, false, true).unwrap();
        viewer
            .apply_host(2, screen.clone(), true, None, true)
            .unwrap();
        viewer
            .apply_host(
                1,
                screen,
                false,
                Some(uuid::Uuid::new_v4().to_string()),
                true,
            )
            .unwrap();
        assert!(viewer.paused);
        assert!(viewer.grant.is_none());
    }
    #[test]
    fn cancellation_wins_over_late_accept_and_voice_is_optional() {
        let mut host = view(Mode::Control, false);
        host.stop("cancelled");
        assert!(host
            .accept(invite(Mode::Control).screen, true, true, true)
            .is_err());
        let mut host = view(Mode::Control, false);
        host.accept(invite(Mode::Control).screen, false, false, false)
            .unwrap();
        assert!(host.grant.is_none());
        assert_eq!(host.voice.stage, "idle");
        assert!(host.host_state(true).is_err());
    }
}
