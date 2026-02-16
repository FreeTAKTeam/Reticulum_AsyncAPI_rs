use async_trait::async_trait;
use chrono::Utc;
use retasync_contract::{
    CorrelationId, MeshCommandEnvelope, MeshEventEnvelope, MeshResultEnvelope, MeshTransferEnvelope,
    TransferHint,
};
use serde_json::Value;
use thiserror::Error;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeReceipt {
    pub message_id: String,
    pub accepted_at: String,
    pub transport: TransportSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportSelection {
    Link,
    Lxmf,
}

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("daemon RPC unavailable")]
    DaemonUnavailable,
    #[error("bridge send failure: {0}")]
    SendFailed(String),
    #[error("invalid payload: {0}")]
    InvalidPayload(String),
}

#[async_trait]
pub trait RpcMeshBridge: Send + Sync {
    async fn send_command(
        &self,
        envelope: MeshCommandEnvelope<Value>,
    ) -> Result<MeshResultEnvelope<Value>, BridgeError>;

    async fn publish_event(
        &self,
        envelope: MeshEventEnvelope<Value>,
    ) -> Result<BridgeReceipt, BridgeError>;

    async fn start_transfer(
        &self,
        envelope: MeshTransferEnvelope<Value>,
    ) -> Result<BridgeReceipt, BridgeError>;

    async fn query_receipt(&self, message_id: &str) -> Result<Option<BridgeReceipt>, BridgeError>;

    async fn poll_events(&self, limit: usize) -> Result<Vec<MeshEventEnvelope<Value>>, BridgeError>;
}

#[derive(Debug, Clone)]
pub struct InMemoryRpcMeshBridge {
    pub prefer_link: bool,
    pub link_available: bool,
}

impl InMemoryRpcMeshBridge {
    pub fn new(prefer_link: bool, link_available: bool) -> Self {
        Self {
            prefer_link,
            link_available,
        }
    }

    pub fn select_transport(&self, hint: Option<TransferHint>) -> TransportSelection {
        match hint {
            Some(TransferHint::Link) if self.link_available => TransportSelection::Link,
            Some(TransferHint::Lxmf) => TransportSelection::Lxmf,
            Some(TransferHint::Link) => TransportSelection::Lxmf,
            None if self.prefer_link && self.link_available => TransportSelection::Link,
            _ => TransportSelection::Lxmf,
        }
    }
}

#[async_trait]
impl RpcMeshBridge for InMemoryRpcMeshBridge {
    async fn send_command(
        &self,
        envelope: MeshCommandEnvelope<Value>,
    ) -> Result<MeshResultEnvelope<Value>, BridgeError> {
        if envelope.operation.trim().is_empty() {
            return Err(BridgeError::InvalidPayload(
                "operation cannot be empty".to_string(),
            ));
        }

        let transport = self.select_transport(envelope.transport_hint.clone());
        info!(
            operation = %envelope.operation,
            message_id = %envelope.message_id,
            transport = ?transport,
            "dispatching command with at-most-once semantics"
        );

        Ok(MeshResultEnvelope {
            message_id: Uuid::now_v7().to_string(),
            correlation_id: envelope.message_id,
            operation: envelope.operation,
            sent_at: Utc::now(),
            source_identity: envelope.destination_identity,
            destination_identity: envelope.source_identity,
            content_type: "application/msgpack".to_string(),
            payload: serde_json::json!({
                "status": "accepted",
                "transport": match transport {
                    TransportSelection::Link => "link",
                    TransportSelection::Lxmf => "lxmf",
                }
            }),
            ttl_ms: envelope.ttl_ms,
            transport_hint: Some(match transport {
                TransportSelection::Link => TransferHint::Link,
                TransportSelection::Lxmf => TransferHint::Lxmf,
            }),
        })
    }

    async fn publish_event(
        &self,
        envelope: MeshEventEnvelope<Value>,
    ) -> Result<BridgeReceipt, BridgeError> {
        let transport = self.select_transport(envelope.transport_hint);
        Ok(BridgeReceipt {
            message_id: envelope.message_id,
            accepted_at: Utc::now().to_rfc3339(),
            transport,
        })
    }

    async fn start_transfer(
        &self,
        envelope: MeshTransferEnvelope<Value>,
    ) -> Result<BridgeReceipt, BridgeError> {
        let transport = self.select_transport(envelope.transport_hint);
        Ok(BridgeReceipt {
            message_id: envelope.message_id,
            accepted_at: Utc::now().to_rfc3339(),
            transport,
        })
    }

    async fn query_receipt(&self, message_id: &str) -> Result<Option<BridgeReceipt>, BridgeError> {
        let correlation = if message_id.trim().is_empty() {
            return Err(BridgeError::InvalidPayload(
                "message_id cannot be empty".to_string(),
            ));
        } else {
            message_id.to_string()
        };

        Ok(Some(BridgeReceipt {
            message_id: correlation,
            accepted_at: Utc::now().to_rfc3339(),
            transport: if self.link_available {
                TransportSelection::Link
            } else {
                TransportSelection::Lxmf
            },
        }))
    }

    async fn poll_events(&self, _limit: usize) -> Result<Vec<MeshEventEnvelope<Value>>, BridgeError> {
        Ok(Vec::new())
    }
}
