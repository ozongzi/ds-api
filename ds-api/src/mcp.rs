//! MCP (Model Context Protocol) client support for `ds-api`.
//!
//! Enable with the `mcp` feature flag:
//!
//! ```toml
//! [dependencies]
//! ds-api = { version = "0.5", features = ["mcp"] }
//! ```
//!
//! # Usage
//!
//! [`McpTool`] wraps an MCP server — either a local process (stdio) or a remote
//! HTTP endpoint — and implements [`Tool`][crate::Tool] so it can be registered
//! directly with [`DeepseekAgent::add_tool`][crate::DeepseekAgent::add_tool].
//!
//! Every tool the MCP server advertises is forwarded to the agent automatically;
//! you do not need to know their names or schemas ahead of time.
//!
//! ## Stdio (local process)
//!
//! Spawns a child process and communicates over stdin/stdout.  Any MCP server
//! that ships as an `npx`, `uvx`, or plain binary command works here:
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use ds_api::{DeepseekAgent, McpTool};
//!
//! let agent = DeepseekAgent::new("sk-...")
//!     .add_tool(McpTool::stdio("npx", &["-y", "@playwright/mcp"]).await?)
//!     .add_tool(McpTool::stdio("uvx", &["mcp-server-git"]).await?);
//! # Ok(()) }
//! ```
//!
//! ## Streamable HTTP (remote server)
//!
//! Connects to a remote MCP server over HTTP + SSE:
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use ds_api::{DeepseekAgent, McpTool};
//!
//! let agent = DeepseekAgent::new("sk-...")
//!     .add_tool(McpTool::http("https://mcp.example.com/").await?);
//! # Ok(()) }
//! ```

use async_trait::async_trait;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    service::{Peer, RoleClient, RunningService},
    transport::{StreamableHttpClientTransport, TokioChildProcess},
};
use serde_json::Value;
use std::sync::Arc;
use tokio::process::Command;
use tracing::{error, instrument};

use crate::raw::request::message::ToolType;
use crate::raw::request::tool::{Function, Tool as RawTool};
use crate::tool_trait::Tool;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can occur while connecting to or communicating with an MCP server.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("failed to spawn MCP server process: {0}")]
    Spawn(#[from] std::io::Error),

    #[error("MCP client initialisation failed: {0}")]
    Init(String),

    #[error("failed to list tools from MCP server: {0}")]
    ListTools(String),

    #[error("MCP tool call failed: {0}")]
    Call(String),
}

// ── McpTool ───────────────────────────────────────────────────────────────────

/// An MCP server exposed as a [`Tool`] that can be registered with
/// [`DeepseekAgent`][crate::DeepseekAgent].
///
/// The tool list is fetched once at construction time and cached for the
/// lifetime of the struct.  All calls are forwarded to the underlying MCP
/// server via the `rmcp` client.
///
/// See the [module-level documentation][self] for usage examples.
pub struct McpTool {
    /// Cached tool definitions fetched from the MCP server at startup.
    tools: Vec<RawTool>,
    /// Live peer used to dispatch `tools/call` requests.
    peer: Arc<Peer<RoleClient>>,
    /// Keep the running service alive for as long as McpTool exists.
    _service: Arc<dyn std::any::Any + Send + Sync>,
}

impl McpTool {
    // ── Constructors ──────────────────────────────────────────────────────────

    /// Connect to an MCP server by spawning a child process.
    ///
    /// `program` is the executable (e.g. `"npx"`, `"uvx"`, `"python"`) and
    /// `args` are its arguments.  The process communicates over stdin/stdout
    /// using the MCP stdio transport.
    ///
    /// # Errors
    ///
    /// Returns [`McpError`] if the process cannot be spawned, the MCP
    /// handshake fails, or the initial `tools/list` call fails.
    #[instrument(skip(args), fields(program = program.as_ref()))]
    pub async fn stdio(
        program: impl AsRef<str>,
        args: &[impl AsRef<str>],
    ) -> Result<Self, McpError> {
        let mut cmd = Command::new(program.as_ref());
        for arg in args {
            cmd.arg(arg.as_ref());
        }

        let transport = TokioChildProcess::new(cmd)?;
        Self::from_service(
            ().serve(transport)
                .await
                .map_err(|e| McpError::Init(e.to_string()))?,
        )
        .await
    }

    /// Connect to a remote MCP server over Streamable HTTP.
    ///
    /// `url` is the base URL of the MCP server
    /// (e.g. `"https://mcp.example.com/"`).
    ///
    /// # Errors
    ///
    /// Returns [`McpError`] if the HTTP connection fails, the MCP handshake
    /// fails, or the initial `tools/list` call fails.
    #[instrument(fields(url = url.as_ref()))]
    pub async fn http(url: impl AsRef<str>) -> Result<Self, McpError> {
        let transport = StreamableHttpClientTransport::from_uri(url.as_ref());
        Self::from_service(
            ().serve(transport)
                .await
                .map_err(|e| McpError::Init(e.to_string()))?,
        )
        .await
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    async fn from_service<S>(running: RunningService<RoleClient, S>) -> Result<Self, McpError>
    where
        S: rmcp::service::Service<RoleClient> + Send + Sync + 'static,
    {
        let peer = running.peer().clone();
        let tools = Self::fetch_tools(&peer).await?;

        Ok(Self {
            tools,
            peer: Arc::new(peer),
            _service: Arc::new(running),
        })
    }

    /// Call `tools/list` (paginating automatically) and convert the MCP tool
    /// definitions into [`RawTool`]s understood by `ds-api`.
    async fn fetch_tools(peer: &Peer<RoleClient>) -> Result<Vec<RawTool>, McpError> {
        let mcp_tools = peer
            .list_all_tools()
            .await
            .map_err(|e| McpError::ListTools(e.to_string()))?;

        let tools = mcp_tools
            .into_iter()
            .map(|mcp_tool| {
                // `input_schema` is an `Arc<JsonObject>` (alias for
                // `serde_json::Map<String, Value>`).  Wrap it in a
                // `Value::Object` so we can pass it as the `parameters` field
                // that the DeepSeek API expects (a plain JSON Schema object).
                let parameters = Value::Object(mcp_tool.input_schema.as_ref().clone());

                RawTool {
                    r#type: ToolType::Function,
                    function: Function {
                        name: mcp_tool.name.to_string(),
                        description: mcp_tool.description.as_deref().map(str::to_string),
                        parameters,
                        strict: None,
                    },
                }
            })
            .collect();

        Ok(tools)
    }
}

// ── Tool impl ─────────────────────────────────────────────────────────────────

#[async_trait]
impl Tool for McpTool {
    fn raw_tools(&self) -> Vec<RawTool> {
        self.tools.clone()
    }

    async fn call(&self, name: &str, args: Value) -> Value {
        let arguments = args.as_object().cloned().map(|m| m.into_iter().collect());
        let owned_name: std::borrow::Cow<'static, str> = name.to_string().into();

        let params = match arguments {
            Some(args) => CallToolRequestParams::new(owned_name).with_arguments(args),
            None => CallToolRequestParams::new(owned_name),
        };

        match self.peer.call_tool(params).await {
            Ok(result) => {
                // MCP returns a list of content items; flatten them into a
                // single JSON value that the model can read.
                let contents: Vec<Value> = result
                    .content
                    .into_iter()
                    .filter_map(|item| serde_json::to_value(item).ok())
                    .collect();

                match contents.len() {
                    0 => serde_json::json!({ "result": null }),
                    1 => contents.into_iter().next().unwrap(),
                    _ => serde_json::json!({ "content": contents }),
                }
            }
            Err(e) => {
                error!(tool = %name, error = %e, "MCP tool call failed");
                serde_json::json!({ "error": e.to_string() })
            }
        }
    }
}
