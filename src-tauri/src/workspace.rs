use crate::{
    db::{
        self, ConversationMemberRecord, ConversationRecord, MessageRecord, NewConversationMember,
        TransferRecord,
    },
    network::{
        messaging,
        protocol::{GroupMember, ProtocolMessage},
        transfer::{cancellation_registry, CancellationRequest},
    },
    peers::{Peer, PeerManager},
};
use serde::Serialize;
use sqlx::{Pool, Sqlite};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub capture: bool,
    pub capture_shortcut: bool,
    pub reveal_file: bool,
    pub notifications: bool,
    pub group_chat: bool,
    pub read_receipts: bool,
    pub conversation_state: bool,
    pub message_search: bool,
    pub file_center: bool,
    pub transfer_cancel: bool,
    pub device_metadata: bool,
    pub native_file_picker: bool,
}

impl RuntimeCapabilities {
    pub fn current() -> Self {
        let capture = cfg!(all(feature = "desktop", target_os = "macos"));
        Self {
            capture,
            capture_shortcut: capture,
            reveal_file: cfg!(all(feature = "desktop", not(target_os = "android"))),
            notifications: notifications_available(),
            group_chat: true,
            read_receipts: true,
            conversation_state: true,
            message_search: true,
            file_center: true,
            transfer_cancel: true,
            device_metadata: true,
            native_file_picker: cfg!(all(feature = "desktop", not(target_os = "android"))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSelf {
    pub id: String,
    pub name: String,
    pub hostname: Option<String>,
    pub mac_address: Option<String>,
    pub addr: String,
    pub avatar: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSettings {
    pub name: String,
    pub avatar: String,
    pub theme: String,
    pub language: String,
    pub notifications_enabled: bool,
    pub download_path: String,
    pub auto_download: bool,
    pub port: u16,
    pub db_path: String,
    pub capture_shortcut: String,
    pub custom_peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceDevice {
    pub id: String,
    pub name: String,
    pub remark: Option<String>,
    pub hostname: Option<String>,
    pub addr: String,
    pub mac_address: Option<String>,
    pub discovery_source: Option<String>,
    pub is_offline: bool,
    pub last_seen: i64,
    pub available_memory_mb: i64,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceConversation {
    pub id: String,
    pub kind: String,
    pub peer_id: Option<String>,
    pub title: String,
    pub pinned: bool,
    pub forced_unread: bool,
    pub draft: String,
    pub unread_count: i64,
    pub last_message: String,
    pub last_message_at: i64,
    pub members: Vec<ConversationMemberRecord>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceMessage {
    pub id: i64,
    pub message_id: i64,
    pub client_message_id: Option<String>,
    pub conversation_id: Option<String>,
    pub sender_id: String,
    pub sender_name: String,
    pub receiver_id: Option<String>,
    pub content: String,
    pub msg_type: String,
    pub timestamp: i64,
    pub status: String,
    pub own: bool,
    pub file_name: Option<String>,
    pub file_path: Option<String>,
    pub file_size: i64,
    pub file_status: Option<String>,
    pub local_available: bool,
    pub sender_msg_id: Option<String>,
    pub sender_addr: Option<String>,
    pub direction: String,
    pub peer_id: Option<String>,
    pub peer_name: Option<String>,
    pub delivered_count: usize,
    pub read_count: usize,
    pub recipient_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceTransfer {
    #[serde(flatten)]
    pub transfer: TransferRecord,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSnapshot {
    #[serde(rename = "self")]
    pub current_user: WorkspaceSelf,
    pub conversations: Vec<WorkspaceConversation>,
    pub devices: Vec<WorkspaceDevice>,
    pub files: Vec<WorkspaceMessage>,
    pub transfers: Vec<WorkspaceTransfer>,
    pub settings: WorkspaceSettings,
    pub capabilities: RuntimeCapabilities,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn notifications_available() -> bool {
    #[cfg(all(feature = "desktop", windows))]
    {
        return true;
    }
    #[cfg(all(feature = "desktop", target_os = "linux"))]
    {
        return std::process::Command::new("sh")
            .args(["-c", "command -v notify-send >/dev/null 2>&1"])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
    }
    #[cfg(all(feature = "desktop", target_os = "android"))]
    {
        return true;
    }
    #[allow(unreachable_code)]
    false
}

fn device_from_peer(peer: Peer) -> WorkspaceDevice {
    WorkspaceDevice {
        id: peer.id,
        name: peer.name,
        remark: peer.remark,
        hostname: peer.hostname,
        addr: peer.addr,
        mac_address: peer.mac_address,
        discovery_source: peer.discovery_source,
        is_offline: peer.is_offline,
        last_seen: peer.last_seen as i64,
        available_memory_mb: peer.available_memory_mb as i64,
        capabilities: peer.capabilities,
    }
}

async fn devices(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    self_id: &str,
) -> Result<Vec<WorkspaceDevice>, String> {
    let mut merged = BTreeMap::new();
    for user in db::list_users_with_metadata(pool).await? {
        if user.id == self_id {
            continue;
        }
        merged.insert(
            user.id.clone(),
            WorkspaceDevice {
                id: user.id,
                name: user.name,
                remark: user.remark,
                hostname: user.hostname,
                addr: user.addr,
                mac_address: user.mac_address,
                discovery_source: user.discovery_source,
                is_offline: user.is_offline,
                last_seen: user.last_seen,
                available_memory_mb: user.available_memory_mb,
                capabilities: Vec::new(),
            },
        );
    }

    for live in peer_manager.get_all_peers() {
        if live.id == self_id {
            continue;
        }
        let mut device = device_from_peer(live);
        if let Some(stored) = merged.get(&device.id) {
            device.remark = stored.remark.clone();
            device.hostname = device.hostname.or_else(|| stored.hostname.clone());
            device.mac_address = device.mac_address.or_else(|| stored.mac_address.clone());
            device.discovery_source = device
                .discovery_source
                .or_else(|| stored.discovery_source.clone());
        }
        merged.insert(device.id.clone(), device);
    }
    Ok(merged.into_values().collect())
}

async fn names_and_addresses(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    self_id: &str,
    self_name: &str,
) -> Result<(HashMap<String, String>, HashMap<String, String>), String> {
    let mut names = HashMap::from([
        (self_id.to_string(), self_name.to_string()),
        ("me".to_string(), self_name.to_string()),
    ]);
    let mut addresses = HashMap::new();
    for device in devices(pool, peer_manager, self_id).await? {
        names.insert(
            device.id.clone(),
            device.remark.clone().unwrap_or(device.name),
        );
        addresses.insert(device.id, device.addr);
    }
    Ok((names, addresses))
}

async fn message_view(
    pool: &Pool<Sqlite>,
    message: MessageRecord,
    self_id: &str,
    names: &HashMap<String, String>,
    addresses: &HashMap<String, String>,
) -> Result<WorkspaceMessage, String> {
    let receipts = match message.client_message_id.as_deref() {
        Some(id) => db::get_message_receipts(pool, id).await?,
        None => Vec::new(),
    };
    let recipient_count = receipts.len();
    let delivered_count = receipts
        .iter()
        .filter(|receipt| receipt.delivered_at.is_some())
        .count();
    let read_count = receipts
        .iter()
        .filter(|receipt| receipt.read_at.is_some())
        .count();
    let own = message.sender_id == self_id || message.sender_id == "me";
    let peer_id = if own {
        message.receiver_id.clone()
    } else {
        Some(message.sender_id.clone())
    };
    let peer_name = peer_id.as_ref().and_then(|id| names.get(id)).cloned();
    let mut status = message
        .status
        .clone()
        .unwrap_or_else(|| if own { "sent" } else { "received" }.to_string());
    if own && recipient_count > 0 {
        if read_count == recipient_count {
            status = "read".to_string();
        } else if delivered_count == recipient_count {
            status = "delivered".to_string();
        }
    }
    let is_file = message.msg_type == "file";
    let local_available = is_file && trusted_file_path(pool, message.id).await.is_ok();
    Ok(WorkspaceMessage {
        id: message.id,
        message_id: message.id,
        client_message_id: message.client_message_id,
        conversation_id: message.conversation_id,
        sender_name: names
            .get(&message.sender_id)
            .cloned()
            .unwrap_or_else(|| message.sender_id.clone()),
        sender_addr: addresses.get(&message.sender_id).cloned(),
        direction: if own { "outgoing" } else { "incoming" }.to_string(),
        peer_id,
        peer_name,
        receiver_id: message.receiver_id,
        content: message.content.clone(),
        msg_type: message.msg_type,
        timestamp: message.timestamp,
        status,
        own,
        file_name: is_file.then_some(message.content),
        file_path: message.file_path,
        file_size: message.file_size.unwrap_or(0),
        file_status: message.file_status,
        local_available,
        sender_msg_id: message.sender_msg_id,
        sender_id: message.sender_id,
        delivered_count,
        read_count,
        recipient_count,
    })
}

async fn message_views(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    messages: Vec<MessageRecord>,
) -> Result<Vec<WorkspaceMessage>, String> {
    let self_id = db::get_user_id(pool).await?;
    let self_name = db::get_username(pool).await?;
    let (names, addresses) = names_and_addresses(pool, peer_manager, &self_id, &self_name).await?;
    let mut views = Vec::with_capacity(messages.len());
    for message in messages {
        views.push(message_view(pool, message, &self_id, &names, &addresses).await?);
    }
    Ok(views)
}

async fn conversation_view(
    pool: &Pool<Sqlite>,
    record: ConversationRecord,
    device_names: &HashMap<String, String>,
    self_id: &str,
) -> Result<WorkspaceConversation, String> {
    let mut members = db::get_conversation_members(pool, &record.id).await?;
    for member in &mut members {
        if let Some(name) = device_names.get(&member.peer_id) {
            member.display_name = name.clone();
        }
    }
    let latest = db::get_conversation_messages(pool, &record.id, 1, 0)
        .await?
        .pop();
    let unread_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages
         WHERE conversation_id = ?
           AND sender_id NOT IN ('me', ?)
           AND client_message_id IS NOT NULL
           AND COALESCE(status, 'received') != 'read'",
    )
    .bind(&record.id)
    .bind(self_id)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("查询未读消息失败: {error}"))?;
    let title = record
        .peer_id
        .as_ref()
        .and_then(|id| device_names.get(id))
        .cloned()
        .or(record.title)
        .unwrap_or_else(|| "未命名会话".to_string());
    Ok(WorkspaceConversation {
        id: record.id,
        kind: record.kind,
        peer_id: record.peer_id,
        title,
        pinned: record.pinned,
        forced_unread: record.forced_unread,
        draft: record.draft,
        unread_count: unread_count.max(i64::from(record.forced_unread)),
        last_message: latest
            .as_ref()
            .map(|message| message.content.clone())
            .unwrap_or_default(),
        last_message_at: latest
            .as_ref()
            .map(|message| message.timestamp)
            .unwrap_or(record.updated_at),
        members,
        version: record.version,
    })
}

pub async fn get_snapshot(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
) -> Result<WorkspaceSnapshot, String> {
    let self_id = db::get_user_id(pool).await?;
    let self_name = db::get_username(pool).await?;
    let devices = devices(pool, peer_manager, &self_id).await?;
    for device in &devices {
        db::ensure_direct_conversation(pool, &device.id).await?;
    }

    let display_names = devices
        .iter()
        .map(|device| {
            (
                device.id.clone(),
                device.remark.clone().unwrap_or_else(|| device.name.clone()),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut conversations = Vec::new();
    for record in db::list_conversations(pool).await? {
        conversations.push(conversation_view(pool, record, &display_names, &self_id).await?);
    }

    let files = message_views(
        pool,
        peer_manager,
        db::list_file_messages(pool, 500, 0).await?,
    )
    .await?;
    let config = crate::config_file::read_config();
    let avatar = db::get_setting(pool, "avatar")
        .await?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "🐼".to_string());
    let theme = db::get_current_theme(pool)
        .await?
        .unwrap_or_else(|| "system".to_string());
    let capture_shortcut = db::get_setting(pool, "capture_shortcut")
        .await?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Ctrl/⌘ ⇧ A".to_string());
    let (hostname, mac_address) = crate::network::discovery::local_device_metadata();
    let settings = WorkspaceSettings {
        name: self_name.clone(),
        avatar: avatar.clone(),
        theme,
        language: config.lang.unwrap_or_else(|| "zh-CN".to_string()),
        notifications_enabled: db::get_notifications_enabled(pool).await,
        download_path: db::get_download_path(pool).await?,
        auto_download: db::get_auto_download(pool).await,
        port: config.port.or(db::get_port(pool).await).unwrap_or(8888),
        db_path: config
            .db_path
            .unwrap_or_else(crate::config_file::get_default_db_path),
        capture_shortcut,
        custom_peers: db::get_custom_peers(pool).await,
    };

    Ok(WorkspaceSnapshot {
        current_user: WorkspaceSelf {
            id: self_id,
            name: self_name,
            hostname,
            mac_address,
            addr: String::new(),
            avatar,
        },
        conversations,
        devices,
        files,
        transfers: transfers(pool).await?,
        settings,
        capabilities: RuntimeCapabilities::current(),
    })
}

fn group_sync_message(
    conversation: &ConversationRecord,
    members: &[ConversationMemberRecord],
) -> ProtocolMessage {
    ProtocolMessage::GroupSync {
        group_id: conversation.id.clone(),
        title: conversation.title.clone().unwrap_or_default(),
        created_by: conversation.created_by.clone().unwrap_or_default(),
        members: members
            .iter()
            .map(|member| GroupMember {
                peer_id: member.peer_id.clone(),
                display_name: member.display_name.clone(),
                role: member.role.clone(),
            })
            .collect(),
        version: conversation.version.max(0) as u64,
        timestamp: now() as u64,
    }
}

fn peer_map(peer_manager: &PeerManager) -> HashMap<String, Peer> {
    peer_manager
        .get_all_peers()
        .into_iter()
        .map(|peer| (peer.id.clone(), peer))
        .collect()
}

pub async fn create_group(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    title: &str,
    member_ids: Vec<String>,
) -> Result<ConversationRecord, String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 80 {
        return Err("群名称需要 1–80 个字符".to_string());
    }
    let member_ids = member_ids
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .collect::<BTreeSet<_>>();
    if member_ids.len() < 2 {
        return Err("群聊至少需要两台其他设备".to_string());
    }
    let peers = peer_map(peer_manager);
    let self_id = db::get_user_id(pool).await?;
    let self_name = db::get_username(pool).await?;
    let mut members = vec![NewConversationMember {
        peer_id: self_id.clone(),
        display_name: self_name,
        role: "owner".to_string(),
    }];
    for member_id in member_ids {
        let peer = peers
            .get(&member_id)
            .ok_or_else(|| format!("找不到设备 {member_id}"))?;
        if !peer.capabilities.iter().any(|value| value == "group_chat") {
            return Err(format!("设备 {} 不支持群聊协议", peer.name));
        }
        members.push(NewConversationMember {
            peer_id: peer.id.clone(),
            display_name: peer.name.clone(),
            role: "member".to_string(),
        });
    }
    let group_id = format!("group:{}", uuid::Uuid::new_v4());
    let conversation =
        db::create_group_conversation(pool, Some(&group_id), title, &self_id, &members).await?;
    send_group_sync(pool, peer_manager, &conversation).await?;
    Ok(conversation)
}

pub async fn send_group_sync(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation: &ConversationRecord,
) -> Result<(), String> {
    let members = db::get_conversation_members(pool, &conversation.id).await?;
    let sync = group_sync_message(conversation, &members);
    let self_id = db::get_user_id(pool).await?;
    let peers = peer_map(peer_manager);
    for member in members {
        let Some(peer) = peers.get(&member.peer_id) else {
            continue;
        };
        if member.peer_id == self_id || peer.is_offline {
            continue;
        }
        let addr = peer.addr.clone();
        let sync = sync.clone();
        tokio::spawn(async move {
            if let Err(error) = crate::network::protocol::send_protocol_message(&addr, &sync).await
            {
                eprintln!("[Workspace] 群同步发送失败: {error}");
            }
        });
    }
    Ok(())
}

pub async fn send_message(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    client_message_id: &str,
    content: &str,
    msg_type: &str,
) -> Result<WorkspaceMessage, String> {
    let content = content.trim();
    if content.is_empty() || content.len() > 64 * 1024 {
        return Err("消息内容不能为空且不能超过 64 KiB".to_string());
    }
    if msg_type != "text" {
        return Err("此接口只接受文本消息".to_string());
    }
    if client_message_id.trim().is_empty() || client_message_id.len() > 128 {
        return Err("无效的客户端消息 ID".to_string());
    }
    let conversation = db::get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "conversation not found".to_string())?;
    let self_id = db::get_user_id(pool).await?;
    let self_name = db::get_username(pool).await?;
    let peers = peer_map(peer_manager);
    let recipients = if conversation.kind == "group" {
        db::get_conversation_members(pool, conversation_id)
            .await?
            .into_iter()
            .map(|member| member.peer_id)
            .filter(|id| id != &self_id)
            .collect::<Vec<_>>()
    } else {
        vec![conversation
            .peer_id
            .clone()
            .ok_or_else(|| "direct conversation has no peer".to_string())?]
    };
    let online = recipients
        .iter()
        .filter_map(|id| peers.get(id))
        .filter(|peer| !peer.is_offline)
        .cloned()
        .collect::<Vec<_>>();
    let message = db::save_conversation_message(
        pool,
        conversation_id,
        &self_id,
        conversation.peer_id.as_deref(),
        content,
        msg_type,
        now(),
        "sent",
        client_message_id,
    )
    .await?;
    db::ensure_message_recipients(pool, client_message_id, &recipients).await?;

    if conversation.kind == "group" {
        let members = db::get_conversation_members(pool, conversation_id).await?;
        let sync = group_sync_message(&conversation, &members);
        for peer in online {
            let addr = peer.addr;
            let sync = sync.clone();
            let outgoing = ProtocolMessage::GroupMessage {
                group_id: conversation_id.to_string(),
                client_message_id: client_message_id.to_string(),
                from_id: self_id.clone(),
                from_name: self_name.clone(),
                content: content.to_string(),
                content_type: msg_type.to_string(),
                timestamp: message.timestamp as u64,
            };
            tokio::spawn(async move {
                let _ = crate::network::protocol::send_protocol_message(&addr, &sync).await;
                if let Err(error) =
                    crate::network::protocol::send_protocol_message(&addr, &outgoing).await
                {
                    eprintln!("[Workspace] 群消息发送失败: {error}");
                }
            });
        }
    } else if let Some(peer) = online.into_iter().next() {
        let conversation_id = conversation_id.to_string();
        let client_message_id = client_message_id.to_string();
        let content = content.to_string();
        let sender_id = self_id.clone();
        let sender_name = self_name.clone();
        tokio::spawn(async move {
            if let Err(error) = messaging::send_direct_message(
                &peer.addr,
                sender_id,
                sender_name,
                conversation_id,
                client_message_id,
                content,
            )
            .await
            {
                eprintln!("[Workspace] 单聊消息发送失败: {error}");
            }
        });
    }

    let (names, addresses) = names_and_addresses(pool, peer_manager, &self_id, &self_name).await?;
    message_view(pool, message, &self_id, &names, &addresses).await
}

pub async fn get_messages(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<WorkspaceMessage>, String> {
    message_views(
        pool,
        peer_manager,
        db::get_conversation_messages(pool, conversation_id, limit, offset).await?,
    )
    .await
}

pub async fn search_messages(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    query: &str,
    limit: i64,
) -> Result<Vec<WorkspaceMessage>, String> {
    message_views(
        pool,
        peer_manager,
        db::search_messages(pool, query, limit).await?,
    )
    .await
}

pub async fn file_center(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
) -> Result<Vec<WorkspaceMessage>, String> {
    message_views(
        pool,
        peer_manager,
        db::list_file_messages(pool, 500, 0).await?,
    )
    .await
}

pub async fn transfers(pool: &Pool<Sqlite>) -> Result<Vec<WorkspaceTransfer>, String> {
    let mut views = Vec::new();
    for transfer in db::list_transfers(pool, 500).await? {
        let file_name = match transfer.message_id {
            Some(message_id) => db::get_file_message_by_id(pool, message_id)
                .await?
                .map(|message| message.content)
                .unwrap_or_default(),
            None => String::new(),
        };
        views.push(WorkspaceTransfer {
            transfer,
            file_name,
        });
    }
    Ok(views)
}

pub async fn mark_messages_read(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    message_ids: Vec<String>,
) -> Result<usize, String> {
    if message_ids.len() > crate::network::protocol::MAX_RECEIPT_BATCH_SIZE {
        return Err("一次最多标记 128 条消息".to_string());
    }
    let self_id = db::get_user_id(pool).await?;
    let peers = peer_map(peer_manager);
    let mut by_sender = BTreeMap::<String, Vec<String>>::new();
    for message_id in message_ids {
        let Some(message) = db::get_message_by_client_id(pool, &message_id).await? else {
            continue;
        };
        if message.conversation_id.as_deref() != Some(conversation_id)
            || message.sender_id == self_id
            || message.sender_id == "me"
        {
            continue;
        }
        db::mark_message_status_by_client_id(pool, &message_id, "read").await?;
        db::save_message_receipt(pool, &message_id, &self_id, None, Some(now())).await?;
        by_sender
            .entry(message.sender_id)
            .or_default()
            .push(message_id);
    }

    let marked = by_sender.values().map(Vec::len).sum();
    for (sender_id, ids) in by_sender {
        let Some(peer) = peers.get(&sender_id).filter(|peer| !peer.is_offline) else {
            continue;
        };
        let ack = ProtocolMessage::ReadAck {
            conversation_id: conversation_id.to_string(),
            from_id: self_id.clone(),
            message_ids: ids.clone(),
            timestamp: now() as u64,
        };
        if crate::network::protocol::send_protocol_message(&peer.addr, &ack)
            .await
            .is_ok()
        {
            for id in ids {
                db::mark_receipt_ack_sent(pool, &id, &self_id, "read").await?;
            }
        }
    }
    Ok(marked)
}

pub async fn resend_for_peer(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    peer_id: &str,
    peer_addr: &str,
) -> Result<(), String> {
    let self_id = db::get_user_id(pool).await?;
    let self_name = db::get_username(pool).await?;
    for group in db::list_groups_for_member(pool, peer_id).await? {
        let members = db::get_conversation_members(pool, &group.id).await?;
        crate::network::protocol::send_protocol_message(
            peer_addr,
            &group_sync_message(&group, &members),
        )
        .await?;
    }
    for receipt in db::get_pending_receipts_for_peer(pool, peer_id).await? {
        if receipt.delivered_at.is_some() && receipt.delivery_ack_sent_at.is_none() {
            let ack = ProtocolMessage::DeliveryAck {
                conversation_id: receipt.conversation_id.clone(),
                from_id: receipt.reader_id.clone(),
                message_ids: vec![receipt.message_client_id.clone()],
                timestamp: now() as u64,
            };
            crate::network::protocol::send_protocol_message(peer_addr, &ack).await?;
            db::mark_receipt_ack_sent(
                pool,
                &receipt.message_client_id,
                &receipt.reader_id,
                "delivery",
            )
            .await?;
        }
        if receipt.read_at.is_some() && receipt.read_ack_sent_at.is_none() {
            let ack = ProtocolMessage::ReadAck {
                conversation_id: receipt.conversation_id,
                from_id: receipt.reader_id.clone(),
                message_ids: vec![receipt.message_client_id.clone()],
                timestamp: now() as u64,
            };
            crate::network::protocol::send_protocol_message(peer_addr, &ack).await?;
            db::mark_receipt_ack_sent(pool, &receipt.message_client_id, &receipt.reader_id, "read")
                .await?;
        }
    }
    for message in db::get_undelivered_messages_for_peer(pool, peer_id).await? {
        if message.sender_id != self_id && message.sender_id != "me" || message.msg_type != "text" {
            continue;
        }
        let Some(conversation_id) = message.conversation_id.clone() else {
            continue;
        };
        let Some(client_message_id) = message.client_message_id.clone() else {
            continue;
        };
        let conversation = db::get_conversation(pool, &conversation_id)
            .await?
            .ok_or_else(|| "conversation not found".to_string())?;
        if conversation.kind == "group" {
            let outgoing = ProtocolMessage::GroupMessage {
                group_id: conversation_id,
                client_message_id,
                from_id: self_id.clone(),
                from_name: self_name.clone(),
                content: message.content,
                content_type: message.msg_type,
                timestamp: message.timestamp as u64,
            };
            crate::network::protocol::send_protocol_message(peer_addr, &outgoing).await?;
        } else {
            messaging::send_direct_message(
                peer_addr,
                self_id.clone(),
                self_name.clone(),
                conversation_id,
                client_message_id,
                message.content,
            )
            .await?;
        }
    }
    crate::network::conversation_file::resume_waiting_for_peer(
        pool,
        peer_manager,
        peer_id,
        peer_addr,
    )
    .await?;
    Ok(())
}

pub async fn delete_local_file(
    pool: &Pool<Sqlite>,
    message_id: i64,
) -> Result<db::FileMessageRecord, String> {
    let message = db::get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    let self_id = db::get_user_id(pool).await?;
    if message.sender_id == self_id || message.sender_id == "me" {
        return Err("不能删除发送方的源文件".to_string());
    }
    let Some(path) = message.file_path.as_deref() else {
        return db::clear_file_path_and_mark_removed(pool, message_id).await;
    };
    let file_path = PathBuf::from(path);
    if file_path.exists() {
        let download_root = PathBuf::from(db::get_download_path(pool).await?)
            .canonicalize()
            .map_err(|error| format!("下载目录不可用: {error}"))?;
        let canonical_file = file_path
            .canonicalize()
            .map_err(|error| format!("文件路径不可用: {error}"))?;
        if !canonical_file.starts_with(&download_root) || !canonical_file.is_file() {
            return Err("拒绝删除下载目录之外的路径".to_string());
        }
        tokio::fs::remove_file(&canonical_file)
            .await
            .map_err(|error| format!("删除本地文件失败: {error}"))?;
    }
    db::clear_file_path_and_mark_removed(pool, message_id).await
}

fn validate_known_file_path(
    file_path: &Path,
    download_root: &Path,
    outgoing: bool,
) -> Result<PathBuf, String> {
    let canonical_file = file_path
        .canonicalize()
        .map_err(|error| format!("文件路径不可用: {error}"))?;
    if !canonical_file.is_file() {
        return Err("文件不可用".to_string());
    }
    if !outgoing {
        let canonical_root = download_root
            .canonicalize()
            .map_err(|error| format!("下载目录不可用: {error}"))?;
        if !canonical_file.starts_with(canonical_root) {
            return Err("拒绝打开下载目录之外的接收文件".to_string());
        }
    }
    Ok(canonical_file)
}

pub async fn trusted_file_path(
    pool: &Pool<Sqlite>,
    message_id: i64,
) -> Result<PathBuf, String> {
    let message = db::get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    let path = message
        .file_path
        .as_deref()
        .ok_or_else(|| "文件尚未下载".to_string())?;
    let self_id = db::get_user_id(pool).await?;
    validate_known_file_path(
        Path::new(path),
        Path::new(&db::get_download_path(pool).await?),
        message.sender_id == self_id || message.sender_id == "me",
    )
}

struct IncomingFileRequestContext {
    self_id: String,
    sender_addr: String,
    client_message_id: String,
    message_id: i64,
    transfer_id: String,
}

async fn prepare_incoming_file_request(
    pool: &Pool<Sqlite>,
    message_id: Option<i64>,
    sender_msg_id: i64,
) -> Result<IncomingFileRequestContext, String> {
    let self_id = db::get_user_id(pool).await?;
    let sender_msg_id_text = sender_msg_id.to_string();
    let mut message = match message_id {
        Some(id) => db::get_file_message_by_id(pool, id).await?,
        None => None,
    };
    if message.is_none() {
        message = sqlx::query_as::<_, MessageRecord>(
            "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                    file_status, file_size, sender_msg_id, status, conversation_id,
                    client_message_id
             FROM messages
             WHERE sender_msg_id = ? AND msg_type = 'file' AND sender_id != ?
               AND client_message_id IS NOT NULL
             ORDER BY id DESC LIMIT 1",
        )
        .bind(&sender_msg_id_text)
        .bind(&self_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("查询待接受文件失败: {error}"))?;
    }
    let Some(message) = message else {
        return Err("待接收文件消息不存在".to_string());
    };
    if message.sender_id == self_id
        || message.sender_id == "me"
        || message.msg_type != "file"
        || message.client_message_id.is_none()
        || (message.sender_msg_id.as_deref() != Some(sender_msg_id_text.as_str())
            && message.id != sender_msg_id)
    {
        return Err("该文件消息不可接收".to_string());
    }
    let transfer = sqlx::query_as::<_, TransferRecord>(
        "SELECT id, message_id, conversation_id, peer_id, direction, status,
                bytes_total, bytes_transferred, error, created_at, updated_at
         FROM transfers
         WHERE message_id = ? AND direction = 'receive'
           AND status IN ('awaiting_acceptance', 'failed')
         ORDER BY rowid DESC LIMIT 1",
    )
    .bind(message.id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("查询待接受文件传输失败: {error}"))?
    .ok_or_else(|| "该文件没有可接收的传输".to_string())?;
    let claimed = db::transition_transfer_status(
        pool,
        &transfer.id,
        &transfer.status,
        "queued",
        transfer.bytes_transferred,
        None,
    )
    .await?
    .ok_or_else(|| "该文件传输已被处理".to_string())?;
    let sender = db::get_user_metadata(pool, &message.sender_id)
        .await?
        .filter(|peer| !peer.addr.trim().is_empty())
        .ok_or_else(|| "发送设备地址不可用".to_string())?;
    db::update_file_status_by_id(pool, message.id, "downloading").await?;
    Ok(IncomingFileRequestContext {
        self_id,
        sender_addr: sender.addr,
        client_message_id: message.client_message_id.unwrap(),
        message_id: message.id,
        transfer_id: claimed.id,
    })
}

pub async fn request_incoming_file(
    pool: &Pool<Sqlite>,
    message_id: Option<i64>,
    sender_msg_id: i64,
) -> Result<(), String> {
    let context = prepare_incoming_file_request(pool, message_id, sender_msg_id).await?;
    let request = serde_json::json!({
        "msg_type": "file_request",
        "sender_msg_id": sender_msg_id,
        "from_id": context.self_id,
        "client_message_id": context.client_message_id,
    });
    if let Err(error) =
        messaging::send_json_via_ws(&context.sender_addr, &request.to_string()).await
    {
        let _ = db::transition_transfer_status(
            pool,
            &context.transfer_id,
            "queued",
            "failed",
            0,
            Some("发送文件接收请求失败"),
        )
        .await;
        let _ = db::update_file_status_by_id(pool, context.message_id, "failed").await;
        return Err(error);
    }
    Ok(())
}

pub async fn clear_conversation_history(
    pool: &Pool<Sqlite>,
    conversation_id: &str,
) -> Result<(), String> {
    let conversation = db::get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "conversation not found".to_string())?;
    if conversation.kind == "direct" {
        let peer_id = conversation
            .peer_id
            .as_deref()
            .ok_or_else(|| "direct conversation has no peer".to_string())?;
        let my_id = db::get_user_id(pool).await?;
        db::clear_conversation_messages(pool, conversation_id).await?;
        return db::clear_chat_history(pool, &my_id, peer_id).await;
    }
    db::clear_conversation_messages(pool, conversation_id).await
}

pub async fn cancel_transfer(
    pool: &Pool<Sqlite>,
    transfer_id: &str,
) -> Result<TransferRecord, String> {
    let current = db::get_transfer(pool, transfer_id)
        .await?
        .ok_or_else(|| "transfer not found".to_string())?;
    if current.direction == "receive" {
        let transfer =
            crate::network::conversation_file::cancel_receive_transfer(pool, transfer_id).await?;
        let _ =
            crate::network::conversation_file::notify_peer_terminal(pool, &transfer, "cancelled")
                .await;
        return Ok(transfer);
    }
    let transfer = db::cancel_transfer(pool, transfer_id).await?;
    let mut transfer = if matches!(
        cancellation_registry().request_cancel(transfer_id),
        CancellationRequest::NotFound
    ) && transfer.status == "cancelling"
    {
        db::update_transfer(
            pool,
            transfer_id,
            "cancelled",
            transfer.bytes_transferred,
            None,
        )
        .await?
    } else {
        transfer
    };
    if let Some(message_id) = transfer.message_id {
        crate::network::conversation_file::refresh_file_status(pool, message_id).await?;
    }
    if matches!(transfer.status.as_str(), "cancelled" | "cancelling") {
        if crate::network::conversation_file::notify_peer_terminal(pool, &transfer, "cancelled")
            .await
            .as_deref()
            == Some("completed")
        {
            for expected in ["cancelled", "cancelling", "failed"] {
                if let Some(completed) = db::transition_transfer_status(
                    pool,
                    &transfer.id,
                    expected,
                    "completed",
                    transfer.bytes_total,
                    None,
                )
                .await?
                {
                    transfer = completed;
                    break;
                }
            }
            if let Some(message_id) = transfer.message_id {
                crate::network::conversation_file::refresh_file_status(pool, message_id).await?;
            }
        }
    }
    Ok(transfer)
}

pub async fn update_device(
    pool: &Pool<Sqlite>,
    device_id: &str,
    remark: Option<&str>,
) -> Result<db::UserRecord, String> {
    db::set_user_remark(pool, device_id, remark).await
}

pub async fn update_preference(pool: &Pool<Sqlite>, key: &str, value: &str) -> Result<(), String> {
    match key {
        "avatar" if value.len() <= 64 => db::set_setting(pool, key, value).await,
        "capture_shortcut" if value.len() <= 128 => db::set_setting(pool, key, value).await,
        "avatar" | "capture_shortcut" => Err(format!("{key} is too long")),
        _ => Err(format!("unsupported preference key: {key}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_match_the_frontend_contract() {
        let value = serde_json::to_value(RuntimeCapabilities::current()).unwrap();
        assert!(value
            .get("groupChat")
            .and_then(|item| item.as_bool())
            .unwrap());
        assert!(value
            .get("readReceipts")
            .and_then(|item| item.as_bool())
            .unwrap());
        assert_eq!(
            value.get("capture"),
            value.get("captureShortcut"),
            "the shortcut is only exposed when capture is real"
        );
    }

    #[tokio::test]
    async fn preferences_are_whitelisted_and_persisted() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();

        update_preference(&pool, "avatar", "🌊").await.unwrap();
        update_preference(&pool, "capture_shortcut", "CommandOrControl+Shift+X")
            .await
            .unwrap();
        assert_eq!(
            db::get_setting(&pool, "avatar").await.unwrap().as_deref(),
            Some("🌊")
        );
        assert_eq!(
            db::get_setting(&pool, "capture_shortcut")
                .await
                .unwrap()
                .as_deref(),
            Some("CommandOrControl+Shift+X")
        );
        assert_eq!(
            update_preference(&pool, "port", "9999").await.unwrap_err(),
            "unsupported preference key: port"
        );
        assert_eq!(
            update_preference(&pool, "avatar", &"x".repeat(65))
                .await
                .unwrap_err(),
            "avatar is too long"
        );
    }

    #[test]
    fn received_files_must_stay_inside_the_download_directory() {
        let root =
            std::env::temp_dir().join(format!("xchat-path-test-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("xchat-outside-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("inside.txt"), b"inside").unwrap();
        std::fs::write(&outside, b"outside").unwrap();

        assert!(validate_known_file_path(&root.join("inside.txt"), &root, false).is_ok());
        assert!(validate_known_file_path(&outside, &root, false).is_err());
        assert!(validate_known_file_path(&outside, &root, true).is_ok());

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_file(outside).unwrap();
    }
}
