use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ReceiptMessage {
    pub id: i64,
    pub content: String,
    pub sender_name: Option<String>,
    pub sender_ip: Option<String>,
    pub image_url: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PendingResponse {
    pub status: String,
    pub messages: Vec<ReceiptMessage>,
}

#[derive(Debug, Clone)]
pub enum PollEvent {
    MessagesReceived(Vec<ReceiptMessage>),
    Error(String),
    Connected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_pending_response() {
        let json = r#"{
            "status": "ok",
            "messages": [
                {
                    "id": 42,
                    "content": "Hello from the web!",
                    "sender_name": "Bob",
                    "sender_ip": "192.168.1.5",
                    "status": "pending",
                    "created_at": "2025-02-19T14:30:00Z"
                }
            ]
        }"#;

        let resp: PendingResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.messages.len(), 1);
        assert_eq!(resp.messages[0].id, 42);
        assert_eq!(resp.messages[0].content, "Hello from the web!");
        assert_eq!(resp.messages[0].sender_name.as_deref(), Some("Bob"));
        assert!(resp.messages[0].image_url.is_none());
    }

    #[test]
    fn deserialize_message_with_image_url() {
        let json = r#"{
            "id": 43,
            "content": "Check this out",
            "sender_name": null,
            "sender_ip": "10.0.0.1",
            "image_url": "/api/receipt_messages/43/image",
            "status": "pending",
            "created_at": "2025-02-19T15:00:00Z"
        }"#;

        let msg: ReceiptMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.id, 43);
        assert_eq!(
            msg.image_url.as_deref(),
            Some("/api/receipt_messages/43/image")
        );
    }
}
