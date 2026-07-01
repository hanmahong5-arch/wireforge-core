//! `wf-mcp` binary: starts the Wireforge MCP server over stdio.
//!
//! Wire it into an MCP-aware client by pointing the client at this
//! binary's absolute path. Logs go to stderr to keep stdout clean for the
//! JSON-RPC framing.

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use wf_mcp::WireforgeServer;

#[tokio::main]
async fn main() -> Result<()> {
    // stderr-only subscriber (default INFO, RUST_LOG-overridable); stdout is
    // reserved for JSON-RPC framing.
    wf_obs::init_server_subscriber();

    tracing::info!("Wireforge MCP server starting (stdio transport)");

    let service = WireforgeServer::new()
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("serve init failed: {e:?}"))?;

    service.waiting().await?;
    Ok(())
}
