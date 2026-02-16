use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use futures::stream::StreamExt;
use retasync_contract::MeshCommandEnvelope;
use retasync_mesh_bridge::RpcMeshBridge;
use retasync_storage::{JobRecord, RetasyncStorage};
use retasync_transfer::TransferUploadRequest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub rpc_endpoint: String,
    pub http_bind: String,
    pub http_auth_token: Option<String>,
    pub sqlite_path: String,
    pub acl_mode: String,
    pub prefer_link: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub healthy: bool,
    pub ready: bool,
    pub daemon_connected: bool,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseUpdate {
    pub event_type: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    pub since: Option<String>,
    pub level: Option<String>,
    pub contains: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct AddAllowlistRequest {
    identity_hash: String,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    limit: Option<i64>,
}

#[derive(Clone)]
pub struct AppState {
    pub storage: RetasyncStorage,
    pub bridge: Arc<dyn RpcMeshBridge>,
    pub node_config: Arc<RwLock<NodeConfig>>,
    pub contract_doc: Arc<String>,
    pub sse_bus: broadcast::Sender<SseUpdate>,
    pub log_buffer: Arc<RwLock<Vec<LogLine>>>,
    pub require_bearer: bool,
}

impl AppState {
    pub fn new(
        storage: RetasyncStorage,
        bridge: Arc<dyn RpcMeshBridge>,
        node_config: NodeConfig,
        contract_doc: String,
        require_bearer: bool,
    ) -> Self {
        let (sse_bus, _) = broadcast::channel(256);
        Self {
            storage,
            bridge,
            node_config: Arc::new(RwLock::new(node_config)),
            contract_doc: Arc::new(contract_doc),
            sse_bus,
            log_buffer: Arc::new(RwLock::new(Vec::new())),
            require_bearer,
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health/live", get(health_live))
        .route("/health/ready", get(health_ready))
        .route("/v1/node/status", get(node_status))
        .route("/v1/node/config", get(node_config).put(update_node_config))
        .route("/v1/contracts/asyncapi", get(get_contract))
        .route("/v1/jobs/{job_id}", get(get_job))
        .route("/v1/jobs/{job_id}/result", get(get_job_result))
        .route("/v1/jobs/commands/{operation}", post(post_command_job))
        .route("/v1/jobs/transfers/upload", post(post_transfer_job))
        .route("/v1/transfers/{transfer_id}", get(get_transfer))
        .route("/v1/cache/events", get(get_cached_events))
        .route("/v1/cache/messages", get(get_cached_messages))
        .route("/v1/logs", get(get_logs))
        .route("/v1/logs/stream", get(stream_logs))
        .route(
            "/v1/security/allowlist",
            get(get_allowlist).post(add_allowlist),
        )
        .route(
            "/v1/security/allowlist/{identity_hash}",
            delete(delete_allowlist),
        )
        .with_state(state)
}

async fn health_live() -> impl IntoResponse {
    Json(json!({
        "status": "live",
        "timestamp": Utc::now().to_rfc3339()
    }))
}

async fn health_ready(State(state): State<AppState>) -> impl IntoResponse {
    let ready = state.bridge.query_receipt("readiness-probe").await.is_ok();
    let payload = Json(json!({
        "status": if ready { "ready" } else { "degraded" },
        "timestamp": Utc::now().to_rfc3339()
    }));

    if ready {
        (StatusCode::OK, payload).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, payload).into_response()
    }
}

async fn node_status(State(state): State<AppState>) -> impl IntoResponse {
    let ready = state.bridge.query_receipt("status-probe").await.is_ok();
    Json(NodeStatus {
        healthy: true,
        ready,
        daemon_connected: ready,
        timestamp: Utc::now().to_rfc3339(),
    })
}

async fn node_config(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.node_config.read().await.clone();
    Json(cfg)
}

async fn update_node_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<NodeConfig>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    authorize(&state, &headers, true).await?;

    {
        let mut guard = state.node_config.write().await;
        *guard = payload.clone();
    }

    let serialized = serde_json::to_string(&payload).map_err(|e| internal_error(e.into()))?;
    state
        .storage
        .append_node_config_revision(&serialized)
        .await
        .map_err(internal_error)?;

    write_log(&state, "info", "node config updated").await;
    emit(
        &state,
        "node.config.updated",
        json!({ "updated_at": Utc::now().to_rfc3339() }),
    );

    Ok((StatusCode::OK, Json(payload)))
}

async fn get_contract(State(state): State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/yaml")],
        state.contract_doc.as_ref().clone(),
    )
}

async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let job = state.storage.get_job(&job_id).await.map_err(internal_error)?;
    match job {
        Some(record) => Ok((StatusCode::OK, Json(record))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"job_not_found"})),
        )),
    }
}

async fn get_job_result(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let result = state
        .storage
        .get_job_result(&job_id)
        .await
        .map_err(internal_error)?;
    match result {
        Some(record) => Ok((StatusCode::OK, Json(record))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"job_result_not_found"})),
        )),
    }
}

async fn post_command_job(
    State(state): State<AppState>,
    Path(operation): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    authorize(&state, &headers, true).await?;

    let job = state
        .storage
        .create_job(&operation, payload.clone())
        .await
        .map_err(internal_error)?;
    let job_id = job.job_id.clone();
    let submitted_at = job.submitted_at.clone();

    write_log(
        &state,
        "info",
        &format!("job submitted for operation {}", operation),
    )
    .await;

    emit(
        &state,
        "job.status.changed",
        json!({
            "job_id": job_id.clone(),
            "status": "queued"
        }),
    );

    let state_for_task = state.clone();
    let operation_for_task = operation.clone();
    let job_id_for_task = job_id.clone();
    let payload_for_task = payload.clone();
    tokio::spawn(async move {
        if let Err(err) = process_command_job(
            state_for_task,
            &job_id_for_task,
            &operation_for_task,
            payload_for_task,
        )
        .await
        {
            error!(job_id = %job_id_for_task, error = %err, "job processing failed");
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "job_id": job_id.clone(),
            "submitted_at": submitted_at,
            "status_url": format!("/v1/jobs/{}", job_id)
        })),
    ))
}

async fn process_command_job(
    state: AppState,
    job_id: &str,
    operation: &str,
    payload: Value,
) -> anyhow::Result<()> {
    state
        .storage
        .update_job_status(job_id, "running", None)
        .await?;

    emit(
        &state,
        "job.status.changed",
        json!({
            "job_id": job_id,
            "status": "running"
        }),
    );

    let envelope = MeshCommandEnvelope {
        message_id: Uuid::now_v7().to_string(),
        operation: operation.to_string(),
        sent_at: Utc::now(),
        source_identity: "local-node".to_string(),
        destination_identity: payload
            .get("destination_identity")
            .and_then(Value::as_str)
            .unwrap_or("mesh")
            .to_string(),
        content_type: "application/msgpack".to_string(),
        payload,
        ttl_ms: None,
        transport_hint: None,
    };

    match state.bridge.send_command(envelope).await {
        Ok(result) => {
            state
                .storage
                .insert_job_result(job_id, result.payload)
                .await?;
            state
                .storage
                .update_job_status(job_id, "success", None)
                .await?;
            emit(
                &state,
                "job.status.changed",
                json!({ "job_id": job_id, "status": "success" }),
            );
            write_log(&state, "info", &format!("job {} completed", job_id)).await;
        }
        Err(error) => {
            state
                .storage
                .update_job_status(job_id, "failed", Some(&error.to_string()))
                .await?;
            emit(
                &state,
                "job.status.changed",
                json!({ "job_id": job_id, "status": "failed", "reason": error.to_string() }),
            );
            write_log(&state, "error", &format!("job {} failed", job_id)).await;
        }
    }

    Ok(())
}

async fn post_transfer_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<TransferUploadRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    authorize(&state, &headers, true).await?;

    let transfer = state
        .storage
        .create_transfer(json!({
            "destination_identity": payload.destination_identity,
            "file_name": payload.file_name,
            "media_type": payload.media_type,
            "payload_size": payload.payload_base64.len()
        }))
        .await
        .map_err(internal_error)?;
    let transfer_id = transfer.transfer_id.clone();
    let transfer_submitted_at = transfer.submitted_at.clone();

    emit(
        &state,
        "transfer.progress",
        json!({
            "transfer_id": transfer_id.clone(),
            "status": "queued"
        }),
    );

    let state_for_task = state.clone();
    let transfer_id_for_task = transfer_id.clone();
    tokio::spawn(async move {
        if let Err(err) = process_transfer_job(state_for_task, &transfer_id_for_task).await {
            error!(transfer_id = %transfer_id_for_task, error = %err, "transfer processing failed");
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "job_id": transfer_id.clone(),
            "transfer_id": transfer_id.clone(),
            "submitted_at": transfer_submitted_at,
            "status_url": format!("/v1/transfers/{}", transfer_id)
        })),
    ))
}

async fn process_transfer_job(state: AppState, transfer_id: &str) -> anyhow::Result<()> {
    state
        .storage
        .update_transfer_status(transfer_id, "running", None)
        .await?;

    emit(
        &state,
        "transfer.progress",
        json!({ "transfer_id": transfer_id, "status": "running" }),
    );

    state
        .storage
        .update_transfer_status(transfer_id, "success", None)
        .await?;
    emit(
        &state,
        "transfer.completed",
        json!({ "transfer_id": transfer_id, "status": "success" }),
    );
    write_log(&state, "info", &format!("transfer {} completed", transfer_id)).await;
    Ok(())
}

async fn get_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let record = state
        .storage
        .get_transfer(&transfer_id)
        .await
        .map_err(internal_error)?;
    match record {
        Some(record) => Ok((StatusCode::OK, Json(record))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"transfer_not_found"})),
        )),
    }
}

async fn get_cached_events(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let limit = query.limit.unwrap_or(100);
    let events = state
        .storage
        .list_cached_events(limit)
        .await
        .map_err(internal_error)?;
    Ok((StatusCode::OK, Json(events)))
}

async fn get_cached_messages(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let limit = query.limit.unwrap_or(100);
    let messages = state
        .storage
        .list_cached_messages(limit)
        .await
        .map_err(internal_error)?;
    Ok((StatusCode::OK, Json(messages)))
}

async fn get_logs(
    State(state): State<AppState>,
    Query(query): Query<LogQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(200);
    let level_filter = query.level.as_deref().map(str::to_ascii_lowercase);
    let contains_filter = query.contains.as_deref().map(str::to_owned);

    let logs = state.log_buffer.read().await;
    let mut items: Vec<LogLine> = logs
        .iter()
        .filter(|entry| {
            if let Some(level) = &level_filter {
                if entry.level.to_ascii_lowercase() != *level {
                    return false;
                }
            }

            if let Some(needle) = &contains_filter {
                if !entry.message.contains(needle) {
                    return false;
                }
            }

            if let Some(since) = &query.since {
                if entry.timestamp < *since {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();

    if items.len() > limit {
        let start = items.len().saturating_sub(limit);
        items = items.split_off(start);
    }

    Json(json!({ "items": items }))
}

async fn stream_logs(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let receiver = state.sse_bus.subscribe();
    let stream = BroadcastStream::new(receiver).filter_map(|item| async move {
        match item {
            Ok(update) => {
                let data = serde_json::to_string(&update.data).unwrap_or_else(|_| "{}".to_string());
                Some(Ok(SseEvent::default().event(update.event_type).data(data)))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}

async fn get_allowlist(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let items = state.storage.list_allowlist().await.map_err(internal_error)?;
    Ok((StatusCode::OK, Json(json!({ "identities": items }))))
}

async fn add_allowlist(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AddAllowlistRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    authorize(&state, &headers, true).await?;
    state
        .storage
        .add_allowlist(&payload.identity_hash, payload.note.as_deref())
        .await
        .map_err(internal_error)?;

    emit(
        &state,
        "security.allowlist.updated",
        json!({ "identity_hash": payload.identity_hash }),
    );
    Ok((StatusCode::CREATED, Json(json!({ "status": "ok" }))))
}

async fn delete_allowlist(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(identity_hash): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    authorize(&state, &headers, true).await?;
    let deleted = state
        .storage
        .delete_allowlist(&identity_hash)
        .await
        .map_err(internal_error)?;

    if deleted {
        emit(
            &state,
            "security.allowlist.updated",
            json!({ "identity_hash": identity_hash, "deleted": true }),
        );
        Ok((StatusCode::NO_CONTENT, Json(json!({}))))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"identity_not_found"})),
        ))
    }
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    write_operation: bool,
) -> Result<(), (StatusCode, Json<Value>)> {
    if !write_operation || !state.require_bearer {
        return Ok(());
    }

    let configured = state.node_config.read().await.http_auth_token.clone();
    let token = configured.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"auth_token_required_but_not_configured"})),
        )
    })?;

    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    let expected = format!("Bearer {token}");
    if provided == expected {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"invalid_or_missing_bearer_token"})),
        ))
    }
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    error!(error = %error, "request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal_error", "detail": error.to_string() })),
    )
}

fn emit(state: &AppState, event_type: &str, data: Value) {
    let _ = state.sse_bus.send(SseUpdate {
        event_type: event_type.to_string(),
        data,
    });
}

async fn write_log(state: &AppState, level: &str, message: &str) {
    info!(level = %level, message = %message, "control-plane log entry");
    let mut buffer = state.log_buffer.write().await;
    buffer.push(LogLine {
        timestamp: Utc::now().to_rfc3339(),
        level: level.to_string(),
        message: message.to_string(),
    });

    if buffer.len() > 500 {
        let keep_start = buffer.len() - 500;
        let trimmed = buffer.split_off(keep_start);
        *buffer = trimmed;
    }
}

#[allow(dead_code)]
fn _job_record_to_value(job: JobRecord) -> Value {
    json!({
        "job_id": job.job_id,
        "operation": job.operation,
        "status": job.status,
        "submitted_at": job.submitted_at,
        "updated_at": job.updated_at,
        "failure_reason": job.failure_reason,
    })
}
