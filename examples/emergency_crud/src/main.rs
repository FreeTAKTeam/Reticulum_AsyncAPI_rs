use chrono::Utc;
use retasync_contract::{encode_canonical, MeshCommandEnvelope};
use serde_json::json;
use uuid::Uuid;

fn main() {
    let payload = json!({
        "callsign": "ALPHA-1",
        "groupName": "North Team",
        "medicalStatus": "green"
    });

    let envelope = MeshCommandEnvelope {
        message_id: Uuid::now_v7().to_string(),
        operation: "emergency_action_message.create".to_string(),
        sent_at: Utc::now(),
        source_identity: "local-node".to_string(),
        destination_identity: "remote-node".to_string(),
        content_type: "application/msgpack".to_string(),
        payload,
        ttl_ms: Some(30_000),
        transport_hint: None,
    };

    let encoded = encode_canonical(&envelope).expect("failed to encode envelope");
    println!("Encoded Emergency CRUD command bytes: {}", encoded.len());
}
