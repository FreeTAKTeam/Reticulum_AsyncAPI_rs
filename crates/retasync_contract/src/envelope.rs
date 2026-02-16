use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type MessageId = String;
pub type CorrelationId = String;
pub type IdentityHash = String;
pub type OperationName = String;
pub type EventName = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferHint {
    Link,
    Lxmf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshCommandEnvelope<T>
where
    T: Serialize,
{
    pub message_id: MessageId,
    pub operation: OperationName,
    pub sent_at: DateTime<Utc>,
    pub source_identity: IdentityHash,
    pub destination_identity: IdentityHash,
    pub content_type: String,
    pub payload: T,
    pub ttl_ms: Option<u64>,
    pub transport_hint: Option<TransferHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshResultEnvelope<T>
where
    T: Serialize,
{
    pub message_id: MessageId,
    pub correlation_id: CorrelationId,
    pub operation: OperationName,
    pub sent_at: DateTime<Utc>,
    pub source_identity: IdentityHash,
    pub destination_identity: IdentityHash,
    pub content_type: String,
    pub payload: T,
    pub ttl_ms: Option<u64>,
    pub transport_hint: Option<TransferHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshEventEnvelope<T>
where
    T: Serialize,
{
    pub message_id: MessageId,
    pub event: EventName,
    pub sent_at: DateTime<Utc>,
    pub source_identity: IdentityHash,
    pub destination_identity: IdentityHash,
    pub content_type: String,
    pub payload: T,
    pub ttl_ms: Option<u64>,
    pub transport_hint: Option<TransferHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Upload,
    Download,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshTransferEnvelope<T>
where
    T: Serialize,
{
    pub message_id: MessageId,
    pub correlation_id: Option<CorrelationId>,
    pub operation: OperationName,
    pub sent_at: DateTime<Utc>,
    pub source_identity: IdentityHash,
    pub destination_identity: IdentityHash,
    pub content_type: String,
    pub direction: TransferDirection,
    pub payload: T,
    pub ttl_ms: Option<u64>,
    pub transport_hint: Option<TransferHint>,
}
