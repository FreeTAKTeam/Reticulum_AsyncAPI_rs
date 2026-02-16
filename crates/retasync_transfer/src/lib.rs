use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    Queued,
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferUploadRequest {
    pub destination_identity: String,
    pub file_name: String,
    pub media_type: String,
    pub payload_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRecord {
    pub transfer_id: String,
    pub status: TransferStatus,
    pub submitted_at: String,
    pub updated_at: String,
    pub failure_reason: Option<String>,
}
