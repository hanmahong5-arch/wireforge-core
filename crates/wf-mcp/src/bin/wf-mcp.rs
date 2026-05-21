//! `wf-mcp` binary: starts the Wireforge MCP server over stdio.
//!
//! Wire it into an MCP-aware client (Claude Code, Cursor, hermes-agent)
//! by pointing the client at the absolute path of this binary. Logs go
//! to stderr to keep stdout clean for the JSON-RPC framing.

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;
use wf_mcp::WireforgeServer;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Wireforge MCP server starting (stdio transport)");

    let service = WireforgeServer::new()
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("serve init failed: {e:?}"))?;

    service.waiting().await?;
    Ok(())
}
