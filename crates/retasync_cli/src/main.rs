use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use retasync_control_plane::{build_router, AppState, NodeConfig};
use retasync_mesh_bridge::InMemoryRpcMeshBridge;
use retasync_storage::{RetasyncStorage, StorageConfig};
use serde::Deserialize;
use tracing::{info, warn};

#[derive(Debug, Parser)]
#[command(author, version, about = "Reticulum AsyncAPI control-plane daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve {
        #[arg(long, default_value = "config/node.toml")]
        config: PathBuf,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct RuntimeConfig {
    rpc: RpcSection,
    http: HttpSection,
    storage: StorageSection,
    acl: AclSection,
    transport: TransportSection,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcSection {
    endpoint: String,
}

#[derive(Debug, Clone, Deserialize)]
struct HttpSection {
    bind: String,
    auth_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct StorageSection {
    sqlite_path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AclSection {
    mode: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TransportSection {
    prefer_link: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve { config } => serve(config).await,
    }
}

async fn serve(config_path: PathBuf) -> Result<()> {
    let config_source = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config file {}", config_path.display()))?;
    let config: RuntimeConfig = toml::from_str(&config_source)
        .with_context(|| format!("invalid config TOML at {}", config_path.display()))?;

    let storage = RetasyncStorage::connect(&StorageConfig {
        sqlite_path: config.storage.sqlite_path.clone(),
    })
    .await?;

    let contract_doc = std::fs::read_to_string("contracts/retasyncapi-v1.asyncapi.yaml")
        .context("failed to load contracts/retasyncapi-v1.asyncapi.yaml")?;

    let require_bearer = requires_token(&config.http.bind);
    if require_bearer && config.http.auth_token.is_none() {
        return Err(anyhow!(
            "non-loopback bind {} requires http.auth_token",
            config.http.bind
        ));
    }

    if !require_bearer {
        info!("loopback bind detected: bearer auth optional");
    } else {
        warn!("non-loopback bind detected: bearer auth enforced");
    }

    let node_config = NodeConfig {
        rpc_endpoint: config.rpc.endpoint.clone(),
        http_bind: config.http.bind.clone(),
        http_auth_token: config.http.auth_token.clone(),
        sqlite_path: config.storage.sqlite_path.clone(),
        acl_mode: config.acl.mode.clone(),
        prefer_link: config.transport.prefer_link,
    };

    let bridge = Arc::new(InMemoryRpcMeshBridge::new(config.transport.prefer_link, true));
    let state = AppState::new(storage, bridge, node_config, contract_doc, require_bearer);
    let app = build_router(state);

    let socket: SocketAddr = config
        .http
        .bind
        .parse()
        .with_context(|| format!("invalid socket address {}", config.http.bind))?;

    let listener = tokio::net::TcpListener::bind(socket)
        .await
        .with_context(|| format!("failed to bind {}", config.http.bind))?;

    info!(bind = %config.http.bind, "retasyncd control-plane listening");
    axum::serve(listener, app).await.context("axum server failed")
}

fn requires_token(bind: &str) -> bool {
    match bind.parse::<SocketAddr>() {
        Ok(addr) => !addr.ip().is_loopback(),
        Err(_) => true,
    }
}
