use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_yaml::Value;

#[derive(Debug, Parser)]
#[command(author, version, about = "OpenAPI to AsyncAPI converter")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Openapi {
        #[arg(long = "in")]
        input: PathBuf,
        #[arg(long = "out")]
        output: PathBuf,
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
struct MappingRow {
    operation_id: String,
    command_operation: String,
}

#[derive(Debug, Clone, Serialize)]
struct WarningRow {
    operation_id: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
struct ConverterOutput {
    asyncapi: String,
    info: AsyncApiInfo,
    #[serde(rename = "defaultContentType")]
    default_content_type: String,
    channels: serde_yaml::Mapping,
    operations: serde_yaml::Mapping,
    components: serde_yaml::Mapping,
    #[serde(rename = "x-retasync")]
    retasync: RetasyncExtension,
}

#[derive(Debug, Clone, Serialize)]
struct AsyncApiInfo {
    title: String,
    version: String,
    description: String,
}

#[derive(Debug, Clone, Serialize)]
struct RetasyncExtension {
    operations: RetasyncOperations,
}

#[derive(Debug, Clone, Serialize)]
struct RetasyncOperations {
    commands: Vec<String>,
    events: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Openapi {
            input,
            output,
            profile,
        } => run_openapi_conversion(input, output, profile),
    }
}

fn run_openapi_conversion(input: PathBuf, output: PathBuf, profile: Option<String>) -> Result<()> {
    let source = std::fs::read_to_string(&input)
        .with_context(|| format!("failed to read {}", input.display()))?;
    let doc: Value = serde_yaml::from_str(&source).context("failed to parse OpenAPI YAML")?;

    let operation_ids = extract_operation_ids(&doc);
    let mut mappings = Vec::new();
    let mut warnings = Vec::new();
    let mut commands = BTreeSet::new();

    for operation_id in operation_ids {
        match map_operation_id(&operation_id) {
            Some(mapped) => {
                commands.insert(mapped.clone());
                mappings.push(MappingRow {
                    operation_id,
                    command_operation: mapped,
                });
            }
            None => warnings.push(WarningRow {
                operation_id,
                reason: "Unable to infer action/entity from operationId".to_string(),
            }),
        }
    }

    if let Some(profile_name) = profile
        .as_deref()
        .or_else(|| detect_profile_from_path(&input))
    {
        apply_profile(profile_name, &mappings, &mut warnings)?;
    }

    let events = derive_events(&commands);
    let commands_vec = commands.into_iter().collect::<Vec<_>>();

    let rendered = render_asyncapi(&commands_vec, &events)?;
    std::fs::write(&output, rendered)
        .with_context(|| format!("failed writing {}", output.display()))?;

    let mapping_path = output.with_extension("mapping.json");
    let warning_path = output.with_extension("warnings.json");

    let mapping_json = serde_json::to_string_pretty(&mappings).context("serialize mappings")?;
    std::fs::write(&mapping_path, mapping_json)
        .with_context(|| format!("failed writing {}", mapping_path.display()))?;

    let warnings_json = serde_json::to_string_pretty(&warnings).context("serialize warnings")?;
    std::fs::write(&warning_path, warnings_json)
        .with_context(|| format!("failed writing {}", warning_path.display()))?;

    println!(
        "Converted {} operations to {} command operations",
        mappings.len(),
        commands_vec.len()
    );
    println!("AsyncAPI: {}", output.display());
    println!("Mapping report: {}", mapping_path.display());
    println!("Warnings report: {}", warning_path.display());

    Ok(())
}

fn extract_operation_ids(doc: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(paths) = doc.get("paths").and_then(Value::as_mapping) else {
        return out;
    };

    for path_item in paths.values() {
        let Some(methods) = path_item.as_mapping() else {
            continue;
        };

        for method in ["get", "put", "post", "delete", "patch", "head", "options", "trace"] {
            let Some(op) = methods.get(&Value::from(method)) else {
                continue;
            };

            if let Some(operation_id) = op
                .as_mapping()
                .and_then(|mapping| mapping.get(&Value::from("operationId")))
                .and_then(Value::as_str)
            {
                out.push(operation_id.to_string());
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

fn map_operation_id(operation_id: &str) -> Option<String> {
    const PREFIXES: [(&str, &str); 6] = [
        ("Create", "create"),
        ("List", "list"),
        ("Put", "put"),
        ("Retrieve", "retrieve"),
        ("Delete", "delete"),
        ("Stream", "stream"),
    ];

    for (prefix, action) in PREFIXES {
        if let Some(rest) = operation_id.strip_prefix(prefix) {
            if rest.is_empty() {
                return None;
            }
            let entity = pascal_to_snake(rest);
            return Some(format!("{}.{}", entity, action));
        }
    }

    None
}

fn derive_events(commands: &BTreeSet<String>) -> Vec<String> {
    let mut events = BTreeSet::new();

    for command in commands {
        if let Some((entity, action)) = command.rsplit_once('.') {
            let event = match action {
                "create" => format!("{}.created", entity),
                "put" => format!("{}.updated", entity),
                "delete" => format!("{}.deleted", entity),
                _ => format!("{}.changed", entity),
            };
            events.insert(event);
        }
    }

    events.into_iter().collect()
}

fn render_asyncapi(commands: &[String], events: &[String]) -> Result<String> {
    let mut channels = serde_yaml::Mapping::new();
    channels.insert(
        Value::from("commandChannel"),
        serde_yaml::to_value(serde_json::json!({
            "address": "commands/{operation}",
            "description": "Mesh command submission channel",
            "messages": {
                "commandEnvelope": {
                    "$ref": "#/components/messages/MeshCommand"
                }
            }
        }))?,
    );
    channels.insert(
        Value::from("resultChannel"),
        serde_yaml::to_value(serde_json::json!({
            "address": "results/{operation}",
            "description": "Mesh command result channel",
            "messages": {
                "resultEnvelope": {
                    "$ref": "#/components/messages/MeshResult"
                }
            }
        }))?,
    );
    channels.insert(
        Value::from("eventChannel"),
        serde_yaml::to_value(serde_json::json!({
            "address": "events/{event}",
            "description": "Mesh event publication channel",
            "messages": {
                "eventEnvelope": {
                    "$ref": "#/components/messages/MeshEvent"
                }
            }
        }))?,
    );

    let mut operations = serde_yaml::Mapping::new();
    operations.insert(
        Value::from("sendCommand"),
        serde_yaml::to_value(serde_json::json!({
            "action": "send",
            "channel": { "$ref": "#/channels/commandChannel" },
            "messages": [{ "$ref": "#/channels/commandChannel/messages/commandEnvelope" }]
        }))?,
    );
    operations.insert(
        Value::from("receiveResult"),
        serde_yaml::to_value(serde_json::json!({
            "action": "receive",
            "channel": { "$ref": "#/channels/resultChannel" },
            "messages": [{ "$ref": "#/channels/resultChannel/messages/resultEnvelope" }]
        }))?,
    );
    operations.insert(
        Value::from("receiveEvent"),
        serde_yaml::to_value(serde_json::json!({
            "action": "receive",
            "channel": { "$ref": "#/channels/eventChannel" },
            "messages": [{ "$ref": "#/channels/eventChannel/messages/eventEnvelope" }]
        }))?,
    );

    let components = serde_yaml::to_value(serde_json::json!({
        "messages": {
            "MeshCommand": {
                "name": "MeshCommand",
                "contentType": "application/msgpack",
                "payload": { "$ref": "#/components/schemas/MeshCommandEnvelope" }
            },
            "MeshResult": {
                "name": "MeshResult",
                "contentType": "application/msgpack",
                "payload": { "$ref": "#/components/schemas/MeshResultEnvelope" }
            },
            "MeshEvent": {
                "name": "MeshEvent",
                "contentType": "application/msgpack",
                "payload": { "$ref": "#/components/schemas/MeshEventEnvelope" }
            }
        },
        "schemas": {
            "MeshCommandEnvelope": {
                "type": "object",
                "required": ["message_id", "operation", "sent_at", "source_identity", "destination_identity", "content_type", "payload"],
                "properties": {
                    "message_id": {"type": "string"},
                    "operation": {"type": "string"},
                    "sent_at": {"type": "string", "format": "date-time"},
                    "source_identity": {"type": "string"},
                    "destination_identity": {"type": "string"},
                    "content_type": {"type": "string", "const": "application/msgpack"},
                    "payload": {"type": "object"}
                }
            },
            "MeshResultEnvelope": {
                "type": "object",
                "required": ["message_id", "correlation_id", "operation", "sent_at", "source_identity", "destination_identity", "content_type", "payload"],
                "properties": {
                    "message_id": {"type": "string"},
                    "correlation_id": {"type": "string"},
                    "operation": {"type": "string"},
                    "sent_at": {"type": "string", "format": "date-time"},
                    "source_identity": {"type": "string"},
                    "destination_identity": {"type": "string"},
                    "content_type": {"type": "string", "const": "application/msgpack"},
                    "payload": {"type": "object"}
                }
            },
            "MeshEventEnvelope": {
                "type": "object",
                "required": ["message_id", "event", "sent_at", "source_identity", "destination_identity", "content_type", "payload"],
                "properties": {
                    "message_id": {"type": "string"},
                    "event": {"type": "string"},
                    "sent_at": {"type": "string", "format": "date-time"},
                    "source_identity": {"type": "string"},
                    "destination_identity": {"type": "string"},
                    "content_type": {"type": "string", "const": "application/msgpack"},
                    "payload": {"type": "object"}
                }
            }
        }
    }))?;

    let doc = ConverterOutput {
        asyncapi: "3.0.0".to_string(),
        info: AsyncApiInfo {
            title: "Reticulum AsyncAPI Contract".to_string(),
            version: "1.0.0".to_string(),
            description: "Generated from OpenAPI source using retasync-convert".to_string(),
        },
        default_content_type: "application/msgpack".to_string(),
        channels,
        operations,
        components: components
            .as_mapping()
            .cloned()
            .context("internal error building components mapping")?,
        retasync: RetasyncExtension {
            operations: RetasyncOperations {
                commands: commands.to_vec(),
                events: events.to_vec(),
            },
        },
    };

    serde_yaml::to_string(&doc).context("serialize AsyncAPI YAML")
}

fn detect_profile_from_path(path: &PathBuf) -> Option<&'static str> {
    let candidate = path.file_name()?.to_str()?;
    if candidate.contains("EmergencyActionMessageManagement-OAS") {
        Some("emergency-management")
    } else {
        None
    }
}

fn apply_profile(
    profile_name: &str,
    mappings: &[MappingRow],
    warnings: &mut Vec<WarningRow>,
) -> Result<()> {
    if profile_name != "emergency-management" {
        anyhow::bail!(
            "unsupported profile: {} (supported: emergency-management)",
            profile_name
        );
    }

    let expected = [
        "CreateEmergencyActionMessage",
        "ListEmergencyActionMessage",
        "PutEmergencyActionMessage",
        "RetrieveEmergencyActionMessage",
        "DeleteEmergencyActionMessage",
        "CreateEvent",
        "ListEvent",
        "PutEvent",
        "RetrieveEvent",
        "DeleteEvent",
        "StreamNotifications",
    ];

    let present: BTreeSet<&str> = mappings.iter().map(|row| row.operation_id.as_str()).collect();
    for operation in expected {
        if !present.contains(operation) {
            warnings.push(WarningRow {
                operation_id: operation.to_string(),
                reason: "missing operation required by emergency-management profile".to_string(),
            });
        }
    }

    Ok(())
}

fn pascal_to_snake(input: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if ch.is_uppercase() {
            if idx > 0 {
                out.push('_');
            }
            for low in ch.to_lowercase() {
                out.push(low);
            }
        } else {
            out.push(ch);
        }
    }
    out
}
