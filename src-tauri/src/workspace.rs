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

// ponytail: text sends are rare; shard this lock by client_message_id if throughput matters.
static MESSAGE_WRITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
        // macOS 用系统 screencapture，Windows/Linux 用 xcap。
        let capture = cfg!(all(
            feature = "desktop",
            any(target_os = "macos", target_os = "windows", target_os = "linux")
        ));
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
    pub max_parallel_channels: u8,
    pub port: u16,
    pub db_path: String,
    pub capture_shortcut: String,
    pub custom_peers: Vec<db::CustomPeerRecord>,
    pub discovery_settings: crate::network::discovery_policy::DiscoverySettings,
    pub network_interfaces: Vec<crate::network::discovery_policy::NetworkInterfaceView>,
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
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceConversation {
    pub id: String,
    pub kind: String,
    pub peer_id: Option<String>,
    pub title: String,
    pub created_by: Option<String>,
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
    pub mention_ids: Vec<String>,
    pub reactions: Vec<WorkspaceReaction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceReaction {
    pub from_id: String,
    pub emoji: String,
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
    #[cfg(all(feature = "desktop", target_os = "macos"))]
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
        app_version: peer.app_version,
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
                app_version: user.app_version,
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
            device.app_version = device.app_version.or_else(|| stored.app_version.clone());
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
    let mention_ids = receipts
        .iter()
        .filter(|receipt| receipt.mentioned)
        .map(|receipt| receipt.reader_id.clone())
        .collect();
    let reactions = match message.client_message_id.as_deref() {
        Some(id) => db::get_message_reactions(pool, id)
            .await?
            .into_iter()
            .map(|reaction| WorkspaceReaction {
                from_id: reaction.reactor_id,
                emoji: reaction.emoji,
            })
            .collect(),
        None => Vec::new(),
    };
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
        mention_ids,
        reactions,
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
           AND COALESCE(status, '') != 'recalled'
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
        created_by: record.created_by,
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
    let discovery = crate::network::discovery_policy::network_snapshot(pool).await?;
    crate::network::discovery::update_local_ip_cache(&discovery);
    let settings = WorkspaceSettings {
        name: self_name.clone(),
        avatar: avatar.clone(),
        theme,
        language: config.lang.unwrap_or_else(|| "zh-CN".to_string()),
        notifications_enabled: db::get_notifications_enabled(pool).await,
        download_path: db::get_download_path(pool).await?,
        auto_download: db::get_auto_download(pool).await,
        max_parallel_channels: crate::network::transfer::load_max_parallel_channels(pool).await?,
        port: config.port.or(db::get_port(pool).await).unwrap_or(8888),
        db_path: config
            .db_path
            .unwrap_or_else(crate::config_file::get_default_db_path),
        capture_shortcut,
        custom_peers: db::get_custom_peer_records(pool).await,
        discovery_settings: discovery.settings,
        network_interfaces: discovery.interfaces,
    };

    Ok(WorkspaceSnapshot {
        current_user: WorkspaceSelf {
            id: self_id,
            name: self_name,
            hostname,
            mac_address,
            addr: crate::network::discovery::local_ip_address().unwrap_or_default(),
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
    let recipients = members
        .iter()
        .map(|member| member.peer_id.clone())
        .collect();
    send_group_sync_to(pool, peer_manager, sync, recipients).await
}

async fn send_group_sync_to(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    sync: ProtocolMessage,
    recipients: BTreeSet<String>,
) -> Result<(), String> {
    let self_id = db::get_user_id(pool).await?;
    let peers = peer_map(peer_manager);
    for peer_id in recipients {
        let Some(peer) = peers.get(&peer_id) else {
            continue;
        };
        if peer_id == self_id || peer.is_offline {
            continue;
        }
        let addr = peer.addr.clone();
        let expected_peer_id = peer.id.clone();
        let sync = sync.clone();
        tokio::spawn(async move {
            if let Err(error) =
                crate::network::protocol::send_protocol_message(&addr, &expected_peer_id, &sync)
                    .await
            {
                eprintln!("[Workspace] 群同步发送失败: {error}");
            }
        });
    }
    Ok(())
}

pub async fn update_group(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    operation: &str,
    value: Option<String>,
    member_ids: Vec<String>,
) -> Result<Option<ConversationRecord>, String> {
    let conversation = db::get_conversation(pool, conversation_id)
        .await?
        .filter(|conversation| conversation.kind == "group")
        .ok_or_else(|| "群聊不存在".to_string())?;
    let self_id = db::get_user_id(pool).await?;
    let old_members = db::get_conversation_members(pool, conversation_id).await?;
    let my_role = old_members
        .iter()
        .find(|member| member.peer_id == self_id)
        .map(|member| member.role.as_str())
        .ok_or_else(|| "你不是群成员".to_string())?;
    let owner = my_role == "owner";
    let manager = owner || my_role == "admin";
    if matches!(
        operation,
        "rename" | "set_admin" | "remove_admin" | "disband"
    ) && !owner
    {
        return Err("仅群主可执行此操作".to_string());
    }
    if matches!(operation, "add_members" | "remove_members" | "announcement") && !manager {
        return Err("仅群主或群管理员可执行此操作".to_string());
    }
    if operation == "announcement" {
        let content = value.unwrap_or_default();
        if content.trim().is_empty() || content.chars().count() > 2000 {
            return Err("群公告需要 1–2000 个字符".to_string());
        }
        send_message(
            pool,
            peer_manager,
            conversation_id,
            &uuid::Uuid::new_v4().to_string(),
            content.trim(),
            "announcement",
            vec![],
        )
        .await?;
        return Ok(Some(conversation));
    }

    let previous_recipients = old_members
        .iter()
        .map(|member| member.peer_id.clone())
        .collect::<BTreeSet<_>>();
    if operation == "disband" {
        let sync = ProtocolMessage::GroupSync {
            group_id: conversation.id.clone(),
            title: conversation.title.clone().unwrap_or_default(),
            created_by: conversation.created_by.clone().unwrap_or_default(),
            members: vec![],
            version: (conversation.version + 1) as u64,
            timestamp: now() as u64,
        };
        send_group_sync_to(pool, peer_manager, sync, previous_recipients).await?;
        db::delete_conversation(pool, conversation_id).await?;
        return Ok(None);
    }

    let requested = member_ids
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .collect::<BTreeSet<_>>();
    let peers = peer_map(peer_manager);
    let mut members = old_members
        .into_iter()
        .map(|member| NewConversationMember {
            peer_id: member.peer_id,
            display_name: member.display_name,
            role: member.role,
        })
        .collect::<Vec<_>>();
    match operation {
        "rename" => {
            let title = value.as_deref().unwrap_or_default();
            if title.trim().is_empty() || title.chars().count() > 80 {
                return Err("群名称需要 1–80 个字符".to_string());
            }
        }
        "add_members" => {
            for id in &requested {
                if members.iter().any(|member| &member.peer_id == id) {
                    continue;
                }
                let peer = peers.get(id).ok_or_else(|| format!("找不到设备 {id}"))?;
                if !peer
                    .capabilities
                    .iter()
                    .any(|capability| capability == "group_chat")
                {
                    return Err(format!("设备 {} 不支持群聊协议", peer.name));
                }
                members.push(NewConversationMember {
                    peer_id: id.clone(),
                    display_name: peer.name.clone(),
                    role: "member".to_string(),
                });
            }
        }
        "remove_members" => members.retain(|member| {
            !requested.contains(&member.peer_id)
                || member.role == "owner"
                || (!owner && member.role == "admin")
        }),
        "set_admin" | "remove_admin" => {
            for member in &mut members {
                if requested.contains(&member.peer_id) && member.role != "owner" {
                    member.role = if operation == "set_admin" {
                        "admin"
                    } else {
                        "member"
                    }
                    .to_string();
                }
            }
        }
        _ => return Err("未知的群聊操作".to_string()),
    }
    let title = if operation == "rename" {
        value.unwrap_or_default().trim().to_string()
    } else {
        conversation.title.clone().unwrap_or_default()
    };
    let updated = db::apply_group_sync(
        pool,
        conversation_id,
        &title,
        conversation.created_by.as_deref().unwrap_or_default(),
        conversation.version + 1,
        &members,
    )
    .await?;
    let current_members = db::get_conversation_members(pool, conversation_id).await?;
    let recipients = previous_recipients
        .into_iter()
        .chain(current_members.iter().map(|member| member.peer_id.clone()))
        .collect();
    send_group_sync_to(
        pool,
        peer_manager,
        group_sync_message(&updated, &current_members),
        recipients,
    )
    .await?;
    Ok(Some(updated))
}

pub async fn recall_message(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    client_message_id: &str,
) -> Result<(), String> {
    let message = db::get_message_by_client_id(pool, client_message_id)
        .await?
        .ok_or_else(|| "消息不存在".to_string())?;
    let self_id = db::get_user_id(pool).await?;
    if message.conversation_id.as_deref() != Some(conversation_id) || message.sender_id != self_id {
        return Err("只能撤回自己发送的消息".to_string());
    }
    let conversation = db::get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "会话不存在".to_string())?;
    let recipients = if conversation.kind == "group" {
        db::get_conversation_members(pool, conversation_id)
            .await?
            .into_iter()
            .map(|member| member.peer_id)
            .filter(|peer_id| peer_id != &self_id)
            .collect::<Vec<_>>()
    } else {
        vec![conversation
            .peer_id
            .ok_or_else(|| "会话缺少接收方".to_string())?]
    };
    let peers = peer_map(peer_manager);
    let recall = ProtocolMessage::MessageRecall {
        conversation_id: conversation_id.to_string(),
        client_message_id: client_message_id.to_string(),
        from_id: self_id.clone(),
        timestamp: now() as u64,
    };
    db::recall_message_for_recipients(
        pool,
        conversation_id,
        client_message_id,
        &self_id,
        &recipients,
    )
    .await?;
    for recipient in recipients {
        if let Some(peer) = peers.get(&recipient).filter(|peer| !peer.is_offline) {
            if crate::network::protocol::send_protocol_message(&peer.addr, &peer.id, &recall)
                .await
                .is_ok()
            {
                db::mark_recall_sent(pool, client_message_id, &recipient).await?;
            }
        }
    }
    Ok(())
}

pub async fn react_to_message(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    client_message_id: &str,
    emoji: &str,
) -> Result<bool, String> {
    let emoji = emoji.trim();
    if emoji.is_empty() || emoji.chars().count() > 8 {
        return Err("invalid reaction emoji".to_string());
    }
    send_message_control(
        pool,
        peer_manager,
        conversation_id,
        client_message_id,
        Some(emoji),
    )
    .await?
    .ok_or_else(|| "reaction state is missing".to_string())
}

pub async fn send_strong_reminder(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    client_message_id: &str,
) -> Result<(), String> {
    send_message_control(
        pool,
        peer_manager,
        conversation_id,
        client_message_id,
        None,
    )
    .await
    .map(|_| ())
}

async fn send_message_control(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    client_message_id: &str,
    reaction: Option<&str>,
) -> Result<Option<bool>, String> {
    let message = db::get_message_by_client_id(pool, client_message_id)
        .await?
        .ok_or_else(|| "message not found".to_string())?;
    let self_id = db::get_user_id(pool).await?;
    if message.conversation_id.as_deref() != Some(conversation_id) {
        return Err("message does not belong to this conversation".to_string());
    }
    if reaction.is_none() && message.sender_id != self_id {
        return Err("only the sender can issue a strong reminder".to_string());
    }
    let reaction = if let Some(emoji) = reaction {
        let active = !db::get_message_reactions(pool, client_message_id)
            .await?
            .iter()
            .any(|item| item.reactor_id == self_id && item.emoji == emoji);
        Some((emoji, active))
    } else {
        None
    };
    let self_name = db::get_username(pool).await?;
    let conversation = db::get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "conversation not found".to_string())?;
    let recipients = if conversation.kind == "group" {
        db::get_conversation_members(pool, conversation_id)
            .await?
            .into_iter()
            .map(|member| member.peer_id)
            .filter(|peer_id| peer_id != &self_id)
            .collect::<Vec<_>>()
    } else {
        vec![conversation
            .peer_id
            .ok_or_else(|| "direct conversation has no peer".to_string())?]
    };
    let peers = peer_map(peer_manager);
    let summary_source = if message.msg_type == "quote" {
        serde_json::from_str::<serde_json::Value>(&message.content)
            .ok()
            .and_then(|value| value.get("text").and_then(|text| text.as_str()).map(str::to_owned))
            .unwrap_or_else(|| message.content.clone())
    } else {
        message.content.clone()
    };
    let summary = summary_source.chars().take(240).collect::<String>();
    let mut delivered = 0usize;
    for recipient in recipients {
        let Some(peer) = peers.get(&recipient).filter(|peer| !peer.is_offline) else {
            continue;
        };
        let result = if conversation.kind == "group" {
            let control = if let Some((emoji, active)) = reaction {
                ProtocolMessage::MessageReaction {
                    conversation_id: conversation_id.to_string(),
                    client_message_id: client_message_id.to_string(),
                    from_id: self_id.clone(),
                    emoji: emoji.to_string(),
                    active,
                    timestamp: now() as u64,
                }
            } else {
                ProtocolMessage::StrongReminder {
                    conversation_id: conversation_id.to_string(),
                    client_message_id: client_message_id.to_string(),
                    from_id: self_id.clone(),
                    from_name: self_name.clone(),
                    summary: summary.clone(),
                    timestamp: now() as u64,
                }
            };
            crate::network::protocol::send_protocol_message(&peer.addr, &peer.id, &control).await
        } else {
            let (msg_type, content) = if let Some((emoji, active)) = reaction {
                (
                    "message_reaction",
                    serde_json::json!({ "emoji": emoji, "active": active }).to_string(),
                )
            } else {
                (
                    "strong_reminder",
                    serde_json::json!({ "summary": summary.clone() }).to_string(),
                )
            };
            messaging::send_direct_control(
                &peer.addr,
                &peer.id,
                self_id.clone(),
                self_name.clone(),
                conversation_id.to_string(),
                client_message_id.to_string(),
                content,
                msg_type.to_string(),
            )
            .await
        };
        if result.is_ok() {
            delivered += 1;
        }
    }
    if delivered == 0 {
        return Err("no recipient is currently online".to_string());
    }
    if let Some((emoji, active)) = reaction {
        db::set_message_reaction(pool, client_message_id, &self_id, emoji, active).await?;
    }
    Ok(reaction.map(|(_, active)| active))
}

/// 消息落库时的初始状态。
/// 单聊先落 pending，等后台真的发出去再升到 sent —— 状态阶梯不允许 sent 退回 pending，
/// 所以只能先低后高，否则「已发送」在对方收不到时也照样显示。
/// 群聊维持原状：多收件人下 sent 表示已进网络，单个成员的送达由 message_receipts 单独追踪。
fn initial_send_status(kind: &str, no_one_online: bool) -> &'static str {
    if kind == "group" && !no_one_online {
        "sent"
    } else {
        "pending"
    }
}

pub async fn send_message(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    client_message_id: &str,
    content: &str,
    msg_type: &str,
    mention_ids: Vec<String>,
) -> Result<WorkspaceMessage, String> {
    let content = content.trim();
    if content.is_empty() || content.len() > 64 * 1024 {
        return Err("消息内容不能为空且不能超过 64 KiB".to_string());
    }
    if !matches!(msg_type, "text" | "quote" | "announcement") {
        return Err("不支持的消息类型".to_string());
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
    let (recipients, members) = if conversation.kind == "group" {
        let members = db::get_conversation_members(pool, conversation_id).await?;
        let recipients = members
            .iter()
            .map(|member| member.peer_id.clone())
            .filter(|id| id != &self_id)
            .collect::<Vec<_>>();
        (recipients, Some(members))
    } else {
        (
            vec![conversation
                .peer_id
                .clone()
                .ok_or_else(|| "direct conversation has no peer".to_string())?],
            None,
        )
    };
    let mention_ids = mention_ids.into_iter().collect::<BTreeSet<_>>();
    if conversation.kind == "group" {
        let recipient_ids = recipients.iter().collect::<BTreeSet<_>>();
        if let Some(id) = mention_ids.iter().find(|id| !recipient_ids.contains(id)) {
            return Err(format!("@ 目标必须是当前群成员且不能是自己: {id}"));
        }
    } else if !mention_ids.is_empty() {
        return Err("单聊消息不能携带 @ 目标".to_string());
    }
    let mention_ids = mention_ids.into_iter().collect::<Vec<_>>();
    let write_guard = MESSAGE_WRITE_LOCK.lock().await;
    let is_new = db::get_message_by_client_id(pool, client_message_id)
        .await?
        .is_none();
    let online = recipients
        .iter()
        .filter_map(|id| peers.get(id))
        .filter(|peer| !peer.is_offline)
        .cloned()
        .collect::<Vec<_>>();
    let initial_status = initial_send_status(&conversation.kind, online.is_empty());
    let message = db::save_conversation_message(
        pool,
        conversation_id,
        &self_id,
        conversation.peer_id.as_deref(),
        content,
        msg_type,
        now(),
        initial_status,
        client_message_id,
    )
    .await?;
    db::ensure_message_recipients(pool, client_message_id, &recipients).await?;
    if is_new {
        db::mark_message_mentions(pool, client_message_id, &mention_ids).await?;
    } else {
        let stored_mentions = db::get_message_receipts(pool, client_message_id)
            .await?
            .into_iter()
            .filter(|receipt| receipt.mentioned)
            .map(|receipt| receipt.reader_id)
            .collect::<BTreeSet<_>>();
        if stored_mentions != mention_ids.iter().cloned().collect() {
            return Err("client message id conflicts with another mention set".to_string());
        }
    }
    drop(write_guard);

    if is_new && conversation.kind == "group" {
        let sync = group_sync_message(&conversation, members.as_deref().unwrap_or_default());
        for peer in online {
            let addr = peer.addr;
            let expected_peer_id = peer.id;
            let sync = sync.clone();
            let outgoing = ProtocolMessage::GroupMessage {
                group_id: conversation_id.to_string(),
                client_message_id: client_message_id.to_string(),
                from_id: self_id.clone(),
                from_name: self_name.clone(),
                content: content.to_string(),
                content_type: msg_type.to_string(),
                mention_ids: mention_ids.clone(),
                timestamp: message.timestamp as u64,
            };
            tokio::spawn(async move {
                let _ = crate::network::protocol::send_protocol_message(
                    &addr,
                    &expected_peer_id,
                    &sync,
                )
                .await;
                if let Err(error) = crate::network::protocol::send_protocol_message(
                    &addr,
                    &expected_peer_id,
                    &outgoing,
                )
                .await
                {
                    eprintln!("[Workspace] 群消息发送失败: {error}");
                }
            });
        }
    } else if is_new {
        if let Some(peer) = online.into_iter().next() {
            let conversation_id = conversation_id.to_string();
            let client_message_id = client_message_id.to_string();
            let content = content.to_string();
            let msg_type = msg_type.to_string();
            let sender_id = self_id.clone();
            let sender_name = self_name.clone();
            let pool = pool.clone();
            let peer_manager = peer_manager.clone();
            tokio::spawn(async move {
                match messaging::send_direct_message(
                    &peer.addr,
                    &peer.id,
                    sender_id,
                    sender_name,
                    conversation_id,
                    client_message_id.clone(),
                    content,
                    msg_type,
                )
                .await
                {
                    // 只有真的发出去了才升到 sent，界面上的「已发送」从此有实际含义
                    Ok(()) => {
                        if let Err(error) =
                            db::mark_message_status_by_client_id(&pool, &client_message_id, "sent")
                                .await
                        {
                            eprintln!("[Workspace] 回写 sent 失败: {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("[Workspace] 单聊消息发送失败: {error}");
                        // 立刻判对方离线：补发靠「离线→上线」跳变触发，
                        // 不标记的话对方回来时不会走补发分支，消息就永久卡住了。
                        peer_manager.force_mark_offline(&peer.id);
                    }
                }
            });
        }
    }

    let (names, addresses) = names_and_addresses(pool, peer_manager, &self_id, &self_name).await?;
    message_view(pool, message, &self_id, &names, &addresses).await
}

pub async fn forward_message(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    source_message_id: i64,
    conversation_ids: Vec<String>,
    note: Option<String>,
) -> Result<(), String> {
    let source = db::get_message_by_id(pool, source_message_id)
        .await?
        .ok_or_else(|| "消息不存在".to_string())?;
    let targets = conversation_ids
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .collect::<BTreeSet<_>>();
    if targets.is_empty() || targets.len() > 100 {
        return Err("请选择 1–100 个转发对象".to_string());
    }
    let note = note.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    for conversation_id in targets {
        if source.msg_type == "file" {
            let path = source
                .file_path
                .as_deref()
                .ok_or_else(|| "原文件已不存在，无法转发".to_string())?;
            crate::network::conversation_file::send_path(
                pool,
                peer_manager,
                &conversation_id,
                path,
            )
            .await?;
        } else if matches!(source.msg_type.as_str(), "text" | "quote" | "announcement") {
            send_message(
                pool,
                peer_manager,
                &conversation_id,
                &uuid::Uuid::new_v4().to_string(),
                &source.content,
                &source.msg_type,
                vec![],
            )
            .await?;
        } else {
            return Err("此消息类型暂不支持转发".to_string());
        }
        if let Some(note) = note.as_deref() {
            send_message(
                pool,
                peer_manager,
                &conversation_id,
                &uuid::Uuid::new_v4().to_string(),
                note,
                "text",
                vec![],
            )
            .await?;
        }
    }
    Ok(())
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
        if crate::network::protocol::send_protocol_message(&peer.addr, &peer.id, &ack)
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
    // 下面每一段都逐条容错：任何一条发失败都不能 `?` 早退，
    // 否则排在后面的待发消息会被一条失败的群同步整段跳过。
    for group in db::list_groups_for_member(pool, peer_id).await? {
        let members = db::get_conversation_members(pool, &group.id).await?;
        if let Err(error) = crate::network::protocol::send_protocol_message(
            peer_addr,
            peer_id,
            &group_sync_message(&group, &members),
        )
        .await
        {
            eprintln!("[Workspace] 群同步补发失败 {}: {error}", group.id);
        }
    }
    for pending in db::get_pending_recalls_for_peer(pool, peer_id).await? {
        match crate::network::protocol::send_protocol_message(
            peer_addr,
            peer_id,
            &ProtocolMessage::MessageRecall {
                conversation_id: pending.conversation_id,
                client_message_id: pending.message_client_id.clone(),
                from_id: pending.sender_id,
                timestamp: pending.recall_requested_at.max(0) as u64,
            },
        )
        .await
        {
            // 只有确认发出去了才记 sent，失败的下次上线再试
            Ok(()) => db::mark_recall_sent(pool, &pending.message_client_id, peer_id).await?,
            Err(error) => {
                eprintln!(
                    "[Workspace] 撤回补发失败 {}: {error}",
                    pending.message_client_id
                );
            }
        }
    }
    for receipt in db::get_pending_receipts_for_peer(pool, peer_id).await? {
        if receipt.delivered_at.is_some() && receipt.delivery_ack_sent_at.is_none() {
            let ack = ProtocolMessage::DeliveryAck {
                conversation_id: receipt.conversation_id.clone(),
                from_id: receipt.reader_id.clone(),
                message_ids: vec![receipt.message_client_id.clone()],
                timestamp: now() as u64,
            };
            if crate::network::protocol::send_protocol_message(peer_addr, peer_id, &ack)
                .await
                .is_ok()
            {
                db::mark_receipt_ack_sent(
                    pool,
                    &receipt.message_client_id,
                    &receipt.reader_id,
                    "delivery",
                )
                .await?;
            }
        }
        if receipt.read_at.is_some() && receipt.read_ack_sent_at.is_none() {
            let ack = ProtocolMessage::ReadAck {
                conversation_id: receipt.conversation_id,
                from_id: receipt.reader_id.clone(),
                message_ids: vec![receipt.message_client_id.clone()],
                timestamp: now() as u64,
            };
            if crate::network::protocol::send_protocol_message(peer_addr, peer_id, &ack)
                .await
                .is_ok()
            {
                db::mark_receipt_ack_sent(
                    pool,
                    &receipt.message_client_id,
                    &receipt.reader_id,
                    "read",
                )
                .await?;
            }
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
        // 会话被删掉的历史消息不该让整轮补发失败，跳过即可
        let Some(conversation) = db::get_conversation(pool, &conversation_id).await? else {
            continue;
        };
        if conversation.kind == "group" {
            let mention_ids = db::get_message_receipts(pool, &client_message_id)
                .await?
                .into_iter()
                .filter(|receipt| receipt.mentioned)
                .map(|receipt| receipt.reader_id)
                .collect();
            let outgoing = ProtocolMessage::GroupMessage {
                group_id: conversation_id,
                client_message_id,
                from_id: self_id.clone(),
                from_name: self_name.clone(),
                content: message.content,
                content_type: message.msg_type,
                mention_ids,
                timestamp: message.timestamp as u64,
            };
            if let Err(error) = crate::network::protocol::send_protocol_message(
                peer_addr,
                peer_id,
                &outgoing,
            )
            .await
            {
                eprintln!("[Workspace] 群消息补发失败: {error}");
            }
        } else {
            let sent = messaging::send_direct_message(
                peer_addr,
                peer_id,
                self_id.clone(),
                self_name.clone(),
                conversation_id,
                client_message_id.clone(),
                message.content,
                message.msg_type,
            )
            .await;
            match sent {
                // 补发成功后把 pending 升到 sent，界面不再一直显示未发送
                Ok(()) => {
                    if let Err(error) =
                        db::mark_message_status_by_client_id(pool, &client_message_id, "sent").await
                    {
                        eprintln!("[Workspace] 补发后回写 sent 失败: {error}");
                    }
                }
                Err(error) => {
                    eprintln!("[Workspace] 单聊消息补发失败 {client_message_id}: {error}");
                }
            }
        }
    }
    if let Err(error) = crate::network::conversation_file::resume_waiting_for_peer(
        pool,
        peer_manager,
        peer_id,
        peer_addr,
    )
    .await
    {
        eprintln!("[Workspace] 文件续传恢复失败: {error}");
    }
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
    sender_id: String,
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
    let sender_id = message.sender_id.clone();
    let sender = db::get_user_metadata(pool, &sender_id)
        .await?
        .filter(|peer| !peer.addr.trim().is_empty())
        .ok_or_else(|| "发送设备地址不可用".to_string())?;
    db::update_file_status_by_id(pool, message.id, "downloading").await?;
    Ok(IncomingFileRequestContext {
        self_id,
        sender_id,
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
    if let Err(error) = messaging::send_json_via_ws(
        &context.sender_addr,
        &context.sender_id,
        &request.to_string(),
    )
    .await
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

    #[tokio::test]
    async fn workspace_settings_include_default_max_parallel_channels() {
        let app_dir = std::env::temp_dir().join(format!(
            "xchat-parallel-settings-test-{}",
            uuid::Uuid::new_v4()
        ));
        let pool = db::init_db_standalone(Some(app_dir.clone())).await.unwrap();
        let peer_manager = PeerManager::new();

        let snapshot = get_snapshot(&pool, &peer_manager).await.unwrap();

        assert_eq!(
            snapshot.settings.max_parallel_channels,
            crate::network::transfer::DEFAULT_MAX_PARALLEL_CHANNELS
        );
        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn one_failed_send_does_not_abort_the_rest_of_the_resend_queue() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-resend-test-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone())).await.unwrap();
        let self_id = db::get_user_id(&pool).await.unwrap();
        let peer_id = "peer-offline";

        // 一个含该成员的群：群同步是补发的第一段，以前它一失败就 `?` 早退，
        // 后面排队的单聊消息永远发不出去。
        db::create_group_conversation(
            &pool,
            Some("group-1"),
            "测试群",
            &self_id,
            &[
                db::NewConversationMember {
                    peer_id: self_id.clone(),
                    display_name: "我".into(),
                    role: "owner".into(),
                },
                db::NewConversationMember {
                    peer_id: peer_id.into(),
                    display_name: "对方".into(),
                    role: "member".into(),
                },
            ],
        )
        .await
        .unwrap();

        let direct = db::ensure_direct_conversation(&pool, peer_id).await.unwrap();
        for client_message_id in ["queued-1", "queued-2"] {
            db::save_conversation_message(
                &pool,
                &direct.id,
                &self_id,
                Some(peer_id),
                "离线期间发的消息",
                "text",
                now(),
                "pending",
                client_message_id,
            )
            .await
            .unwrap();
            db::ensure_message_recipients(&pool, client_message_id, &[peer_id.to_string()])
                .await
                .unwrap();
        }

        // 再排一条待撤回，验证撤回段也逐条容错
        db::save_conversation_message(
            &pool,
            &direct.id,
            &self_id,
            Some(peer_id),
            "要撤回的消息",
            "text",
            now(),
            "pending",
            "recalled-1",
        )
        .await
        .unwrap();
        db::recall_message_for_recipients(
            &pool,
            &direct.id,
            "recalled-1",
            &self_id,
            &[peer_id.to_string()],
        )
        .await
        .unwrap();

        // 127.0.0.1:9 上没人监听，每一次发送都会失败
        let result = resend_for_peer(&pool, &PeerManager::new(), peer_id, "127.0.0.1:9").await;
        assert!(result.is_ok(), "整轮补发不能因为单条发送失败而报错");

        // 全部发送都失败，所以队列必须原样留着，下次上线再试
        assert_eq!(
            db::get_undelivered_messages_for_peer(&pool, peer_id)
                .await
                .unwrap()
                .len(),
            2,
            "发送失败的消息不能被当成已送达"
        );
        for client_message_id in ["queued-1", "queued-2"] {
            let message = db::get_message_by_client_id(&pool, client_message_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                message.status.as_deref(),
                Some("pending"),
                "没发出去就不能显示已发送"
            );
        }
        assert_eq!(
            db::get_pending_recalls_for_peer(&pool, peer_id)
                .await
                .unwrap()
                .len(),
            1,
            "撤回没发出去就不能记成已发送"
        );

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[test]
    fn direct_messages_only_claim_sent_after_the_network_succeeds() {
        // 单聊必须先 pending：状态阶梯不允许 sent 退回 pending，
        // 一开始就写 sent 的话，发送失败时界面永远停在「已发送」。
        assert_eq!(initial_send_status("direct", false), "pending");
        assert_eq!(initial_send_status("direct", true), "pending");

        // 群聊有在线成员就算已进网络，单个成员的送达交给 message_receipts
        assert_eq!(initial_send_status("group", false), "sent");
        // 全员离线的群聊没发出去任何东西，同样只能是 pending
        assert_eq!(initial_send_status("group", true), "pending");
    }

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

    #[test]
    fn conversation_snapshot_serializes_the_group_creator() {
        let conversation = WorkspaceConversation {
            id: "group-1".into(),
            kind: "group".into(),
            peer_id: None,
            title: "Test group".into(),
            created_by: Some("self-device".into()),
            pinned: false,
            forced_unread: false,
            draft: String::new(),
            unread_count: 0,
            last_message: String::new(),
            last_message_at: 0,
            members: vec![],
            version: 1,
        };

        let value = serde_json::to_value(conversation).unwrap();
        assert_eq!(value.get("created_by").and_then(|item| item.as_str()), Some("self-device"));
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

    #[tokio::test]
    async fn concurrent_retries_keep_the_first_mention_set() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-send-test-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone())).await.unwrap();
        let self_id = db::get_user_id(&pool).await.unwrap();
        db::create_group_conversation(
            &pool,
            Some("group-send-test"),
            "Test",
            &self_id,
            &[
                NewConversationMember {
                    peer_id: self_id.clone(),
                    display_name: "Me".into(),
                    role: "owner".into(),
                },
                NewConversationMember {
                    peer_id: "peer-a".into(),
                    display_name: "Peer A".into(),
                    role: "member".into(),
                },
                NewConversationMember {
                    peer_id: "peer-b".into(),
                    display_name: "Peer B".into(),
                    role: "member".into(),
                },
            ],
        )
        .await
        .unwrap();
        let peers = PeerManager::new();

        let first = send_message(
            &pool,
            &peers,
            "group-send-test",
            "same-client-id",
            "hello",
            "text",
            vec!["peer-a".into()],
        );
        let second = send_message(
            &pool,
            &peers,
            "group-send-test",
            "same-client-id",
            "hello",
            "text",
            vec!["peer-b".into()],
        );
        let (first, second) = tokio::join!(first, second);

        assert_ne!(first.is_ok(), second.is_ok());
        assert!(first
            .err()
            .or_else(|| second.err())
            .unwrap()
            .contains("mention"));
        let mentioned = db::get_message_receipts(&pool, "same-client-id")
            .await
            .unwrap()
            .into_iter()
            .filter(|receipt| receipt.mentioned)
            .count();
        assert_eq!(mentioned, 1);

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn offline_recall_keeps_a_hidden_tombstone_instead_of_losing_retry_state() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-recall-test-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone())).await.unwrap();
        let conversation = db::ensure_direct_conversation(&pool, "peer-offline")
            .await
            .unwrap();
        let peers = PeerManager::new();

        send_message(
            &pool,
            &peers,
            &conversation.id,
            "recall-offline-1",
            "withdraw me",
            "text",
            vec![],
        )
        .await
        .unwrap();
        recall_message(
            &pool,
            &peers,
            &conversation.id,
            "recall-offline-1",
        )
        .await
        .unwrap();

        let tombstone = db::get_message_by_client_id(&pool, "recall-offline-1")
            .await
            .unwrap()
            .expect("offline recall must survive for retry");
        assert_eq!(tombstone.status.as_deref(), Some("recalled"));
        let pending = db::get_pending_recalls_for_peer(&pool, "peer-offline")
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message_client_id, "recall-offline-1");
        assert!(db::get_conversation_messages(&pool, &conversation.id, 40, 0)
            .await
            .unwrap()
            .is_empty());

        db::store_recalled_tombstone(
            &pool,
            &conversation.id,
            "late-message-1",
            "peer-offline",
            42,
        )
        .await
        .unwrap();
        let late = db::save_conversation_message(
            &pool,
            &conversation.id,
            "peer-offline",
            None,
            "must stay hidden",
            "text",
            43,
            "delivered",
            "late-message-1",
        )
        .await
        .unwrap();
        assert_eq!(late.status.as_deref(), Some("recalled"));
        assert!(late.content.is_empty());
        assert!(db::get_undelivered_messages_for_peer(&pool, "peer-offline")
            .await
            .unwrap()
            .is_empty());

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn group_recall_tracks_each_offline_member() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-group-recall-test-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone())).await.unwrap();
        let self_id = db::get_user_id(&pool).await.unwrap();
        db::create_group_conversation(
            &pool,
            Some("group-recall-test"),
            "Test",
            &self_id,
            &[
                NewConversationMember {
                    peer_id: self_id.clone(),
                    display_name: "Me".into(),
                    role: "owner".into(),
                },
                NewConversationMember {
                    peer_id: "peer-a".into(),
                    display_name: "Peer A".into(),
                    role: "member".into(),
                },
                NewConversationMember {
                    peer_id: "peer-b".into(),
                    display_name: "Peer B".into(),
                    role: "member".into(),
                },
            ],
        )
        .await
        .unwrap();
        let peers = PeerManager::new();
        send_message(
            &pool,
            &peers,
            "group-recall-test",
            "group-recall-1",
            "withdraw from everyone",
            "text",
            vec![],
        )
        .await
        .unwrap();
        recall_message(
            &pool,
            &peers,
            "group-recall-test",
            "group-recall-1",
        )
        .await
        .unwrap();

        assert_eq!(
            db::get_pending_recalls_for_peer(&pool, "peer-a")
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            db::get_pending_recalls_for_peer(&pool, "peer-b")
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(db::get_pending_recalls_for_peer(&pool, &self_id)
            .await
            .unwrap()
            .is_empty());

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
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
