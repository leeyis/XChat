use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_RECEIPT_BATCH_SIZE: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    pub peer_id: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "msg_type", rename_all = "snake_case")]
pub enum ProtocolMessage {
    GroupSync {
        group_id: String,
        title: String,
        created_by: String,
        members: Vec<GroupMember>,
        version: u64,
        timestamp: u64,
    },
    GroupMessage {
        group_id: String,
        client_message_id: String,
        from_id: String,
        from_name: String,
        content: String,
        content_type: String,
        mention_ids: Vec<String>,
        timestamp: u64,
    },
    MessageRecall {
        conversation_id: String,
        client_message_id: String,
        from_id: String,
        timestamp: u64,
    },
    DeliveryAck {
        conversation_id: String,
        from_id: String,
        message_ids: Vec<String>,
        timestamp: u64,
    },
    ReadAck {
        conversation_id: String,
        from_id: String,
        message_ids: Vec<String>,
        timestamp: u64,
    },
}

impl ProtocolMessage {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::GroupSync {
                group_id,
                created_by,
                members,
                ..
            } => {
                require_value(group_id, "group_id")?;
                require_value(created_by, "created_by")?;
                for member in members {
                    require_value(&member.peer_id, "members.peer_id")?;
                }
                Ok(())
            }
            Self::GroupMessage {
                group_id,
                client_message_id,
                from_id,
                content_type,
                mention_ids,
                ..
            } => {
                require_value(group_id, "group_id")?;
                require_value(client_message_id, "client_message_id")?;
                require_value(from_id, "from_id")?;
                require_value(content_type, "content_type")?;
                for mention_id in mention_ids {
                    require_value(mention_id, "mention_ids")?;
                }
                Ok(())
            }
            Self::MessageRecall {
                conversation_id,
                client_message_id,
                from_id,
                ..
            } => {
                require_value(conversation_id, "conversation_id")?;
                require_value(client_message_id, "client_message_id")?;
                require_value(from_id, "from_id")
            }
            Self::DeliveryAck {
                conversation_id,
                from_id,
                message_ids,
                ..
            }
            | Self::ReadAck {
                conversation_id,
                from_id,
                message_ids,
                ..
            } => {
                require_value(conversation_id, "conversation_id")?;
                require_value(from_id, "from_id")?;
                if message_ids.is_empty() || message_ids.len() > MAX_RECEIPT_BATCH_SIZE {
                    return Err(format!(
                        "message_ids must contain 1..={MAX_RECEIPT_BATCH_SIZE} items"
                    ));
                }
                for message_id in message_ids {
                    require_value(message_id, "message_ids")?;
                }
                Ok(())
            }
        }
    }
}

fn require_value(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

pub fn parse_protocol_message(raw: &str) -> Result<Option<ProtocolMessage>, String> {
    let value = serde_json::from_str(raw).map_err(|error| format!("invalid JSON: {error}"))?;
    parse_protocol_value(value)
}

pub fn parse_protocol_value(value: Value) -> Result<Option<ProtocolMessage>, String> {
    let message_type = match value.get("msg_type").and_then(Value::as_str) {
        Some("group_sync") => "group_sync",
        Some("group_message") => "group_message",
        Some("message_recall") => "message_recall",
        Some("delivery_ack") => "delivery_ack",
        Some("read_ack") => "read_ack",
        _ => return Ok(None),
    };

    let message: ProtocolMessage = serde_json::from_value(value)
        .map_err(|error| format!("invalid {message_type}: {error}"))?;
    message.validate()?;
    Ok(Some(message))
}

pub async fn send_protocol_message(
    peer_addr: &str,
    message: &ProtocolMessage,
) -> Result<(), String> {
    message.validate()?;
    let json =
        serde_json::to_string(message).map_err(|error| format!("serialize protocol: {error}"))?;
    super::messaging::send_json_via_ws(peer_addr, &json).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trip_keeps_stable_message_id_and_wire_type() {
        let message = ProtocolMessage::GroupMessage {
            group_id: "group-1".into(),
            client_message_id: "client-message-1".into(),
            from_id: "peer-1".into(),
            from_name: "Alice".into(),
            content: "hello".into(),
            content_type: "text".into(),
            mention_ids: vec![],
            timestamp: 42,
        };

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains(r#""msg_type":"group_message""#));
        assert_eq!(parse_protocol_message(&json).unwrap(), Some(message));
        assert_eq!(
            parse_protocol_message(r#"{"msg_type":"text","content":"legacy"}"#).unwrap(),
            None
        );
    }

    #[test]
    fn group_message_round_trip_keeps_mention_targets() {
        let raw = r#"{
            "msg_type":"group_message",
            "group_id":"group-1",
            "client_message_id":"client-message-1",
            "from_id":"peer-1",
            "from_name":"Alice",
            "content":"@Bob hello",
            "content_type":"text",
            "mention_ids":["peer-2"],
            "timestamp":42
        }"#;

        let message = parse_protocol_message(raw).unwrap().unwrap();
        let value = serde_json::to_value(message).unwrap();

        assert_eq!(value["mention_ids"], serde_json::json!(["peer-2"]));
        assert!(parse_protocol_message(&raw.replace(
            r#""mention_ids":["peer-2"]"#,
            r#""mention_ids":["peer-2","peer-2"]"#,
        ))
        .is_ok());
    }

    #[test]
    fn receipt_batch_is_bounded() {
        let message = ProtocolMessage::DeliveryAck {
            conversation_id: "direct:a:b".into(),
            from_id: "peer-b".into(),
            message_ids: (0..=MAX_RECEIPT_BATCH_SIZE)
                .map(|index| format!("message-{index}"))
                .collect(),
            timestamp: 42,
        };

        assert!(message.validate().is_err());
    }

    #[test]
    fn recall_round_trip_keeps_sender_and_message_identity() {
        let message = ProtocolMessage::MessageRecall {
            conversation_id: "group-1".into(),
            client_message_id: "message-1".into(),
            from_id: "peer-1".into(),
            timestamp: 42,
        };

        let json = serde_json::to_string(&message).unwrap();
        assert_eq!(parse_protocol_message(&json).unwrap(), Some(message));
    }
}
