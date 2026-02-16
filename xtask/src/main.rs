use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use retasync_codegen::render_contracts_module;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] != "codegen" {
        print_usage();
        return Ok(());
    }

    let check_mode = args.iter().any(|arg| arg == "--check");

    let workspace_root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .context("CARGO_MANIFEST_DIR not set")?
            .trim_end_matches("\\xtask")
            .trim_end_matches("/xtask"),
    );

    let contract_path = workspace_root.join("contracts/retasyncapi-v1.asyncapi.yaml");
    let generated_path = workspace_root.join("crates/retasync_contract/src/generated/contracts.rs");

    let contract_source = std::fs::read_to_string(&contract_path)
        .with_context(|| format!("failed reading {}", contract_path.display()))?;
    let rendered = render_contracts_module(&contract_source)?;

    if check_mode {
        let existing = std::fs::read_to_string(&generated_path)
            .with_context(|| format!("failed reading {}", generated_path.display()))?;

        if normalize_newlines(&existing) != normalize_newlines(&rendered) {
            bail!(
                "generated contracts drift detected: run `cargo xtask codegen` to refresh {}",
                generated_path.display()
            );
        }

        println!("codegen check passed");
        return Ok(());
    }

    std::fs::write(&generated_path, rendered)
        .with_context(|| format!("failed writing {}", generated_path.display()))?;
    println!("generated {}", generated_path.display());
    Ok(())
}

fn normalize_newlines(input: &str) -> String {
    input
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
}

fn print_usage() {
    eprintln!("Usage: cargo xtask codegen [--check]");
}
