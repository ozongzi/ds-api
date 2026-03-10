use ds_api::McpTool;
use ds_api::tool;
use serde_json::{json, Value};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use toml_edit::{Array, DocumentMut, Item, Table};

const CONFIG_PATH: &str = "config.toml";

pub struct ManageMcpSpell {
    /// All currently running MCP tools, shared with AppState.
    pub mcp_tools: Arc<Mutex<Vec<(String, McpTool)>>>,
    /// Set to true after install/uninstall so run_generation drops the
    /// recovered agent; the next start_generation will rebuild it with the
    /// updated tool list.
    pub agent_stale: Arc<AtomicBool>,
    /// Sum of raw_tools().len() for all built-in spells. Used for limit check.
    pub builtin_tool_count: usize,
    /// Maximum total tool definitions (built-in + all MCP).
    pub max_tools: usize,
}

#[tool]
impl Tool for ManageMcpSpell {
    /// 列出 config.toml 中预设的可用 MCP 服务器（可直接安装，无需手动填写参数）
    async fn list_available_mcp(&self) -> Value {
        match read_catalog().await {
            Ok(entries) => json!({ "catalog": entries }),
            Err(e) => json!({ "catalog": [], "error": e.to_string() }),
        }
    }

    /// 列出当前已安装并运行的 MCP 服务器名称
    async fn list_installed_mcp(&self) -> Value {
        let tools = self.mcp_tools.lock().await;
        let entries: Vec<Value> = tools
            .iter()
            .map(|(name, tool)| {
                json!({ "name": name, "tool_count": tool.raw_tools().len() })
            })
            .collect();
        json!({ "installed": entries })
    }

    /// 安装并激活 MCP 服务器。使用 list_available_mcp 查看可用预设。
    /// name: 服务器唯一标识符（用于后续卸载）
    /// command: 启动命令（如 npx、uvx、mcp-language-server）
    /// args: 命令参数列表
    async fn install_mcp(&self, name: String, command: String, args: Vec<String>) -> Value {
        // Duplicate check
        {
            let tools = self.mcp_tools.lock().await;
            if tools.iter().any(|(n, _)| n == &name) {
                return json!({ "error": format!("MCP '{}' 已在运行，请先卸载", name) });
            }
        }

        // Start the subprocess
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let tool = match McpTool::stdio(&command, &args_ref).await {
            Ok(t) => t,
            Err(e) => return json!({ "error": format!("启动失败: {e}") }),
        };
        let new_tool_count = tool.raw_tools().len();

        // Limit check: count existing MCP tools + new ones + built-in
        {
            let tools = self.mcp_tools.lock().await;
            let mcp_total: usize = tools.iter().map(|(_, t)| t.raw_tools().len()).sum();
            let total = self.builtin_tool_count + mcp_total + new_tool_count;
            if total > self.max_tools {
                return json!({
                    "error": format!(
                        "安装后工具总数 {} 将超过上限 {}（内置 {} + 现有 MCP {} + 新增 {}）",
                        total, self.max_tools,
                        self.builtin_tool_count, mcp_total, new_tool_count
                    )
                });
            }
        }

        // Add to running tools
        {
            let mut tools = self.mcp_tools.lock().await;
            tools.push((name.clone(), tool));
        }

        // Mark agent stale — next generation rebuilds with updated MCP list
        self.agent_stale.store(true, Ordering::Relaxed);

        // Persist to config.toml
        if let Err(e) = persist_install(&name, &command, &args).await {
            return json!({
                "status": "partial",
                "message": format!("MCP '{}' 已启动（{} 个工具）但写入 config.toml 失败: {}", name, new_tool_count, e)
            });
        }

        json!({
            "status": "ok",
            "message": format!("MCP '{}' 已安装（{} 个工具）并持久化，下次对话时生效。", name, new_tool_count)
        })
    }

    /// 停止并卸载 MCP 服务器，同时从 config.toml 中移除
    /// name: 要卸载的服务器标识符
    async fn uninstall_mcp(&self, name: String) -> Value {
        let removed = {
            let mut tools = self.mcp_tools.lock().await;
            if let Some(idx) = tools.iter().position(|(n, _)| n == &name) {
                tools.remove(idx); // Drop kills the subprocess
                true
            } else {
                false
            }
        };

        if !removed {
            return json!({ "error": format!("MCP '{}' 未在运行列表中", name) });
        }

        self.agent_stale.store(true, Ordering::Relaxed);

        if let Err(e) = persist_uninstall(&name).await {
            return json!({
                "status": "partial",
                "message": format!("MCP '{}' 已停止但从 config.toml 移除失败: {}", name, e)
            });
        }

        json!({ "status": "ok", "message": format!("MCP '{}' 已卸载。", name) })
    }
}

// ── Config persistence ────────────────────────────────────────────────────────

async fn persist_install(name: &str, command: &str, args: &[String]) -> anyhow::Result<()> {
    let content = tokio::fs::read_to_string(CONFIG_PATH).await?;
    let mut doc: DocumentMut = content.parse()?;

    let mut tbl = Table::new();
    tbl.insert("name", toml_edit::value(name));
    tbl.insert("command", toml_edit::value(command));
    if !args.is_empty() {
        let mut arr = Array::new();
        for a in args {
            arr.push(a.as_str());
        }
        tbl.insert("args", Item::Value(arr.into()));
    }

    match doc.get_mut("mcp") {
        Some(Item::ArrayOfTables(aot)) => {
            aot.push(tbl);
        }
        _ => {
            let mut aot = toml_edit::ArrayOfTables::new();
            aot.push(tbl);
            doc.insert("mcp", Item::ArrayOfTables(aot));
        }
    }

    tokio::fs::write(CONFIG_PATH, doc.to_string()).await?;
    Ok(())
}

async fn persist_uninstall(name: &str) -> anyhow::Result<()> {
    let content = tokio::fs::read_to_string(CONFIG_PATH).await?;
    let mut doc: DocumentMut = content.parse()?;

    if let Some(Item::ArrayOfTables(aot)) = doc.get_mut("mcp") {
        let to_remove: Vec<usize> = aot
            .iter()
            .enumerate()
            .filter(|(_, t)| t.get("name").and_then(|v| v.as_str()) == Some(name))
            .map(|(i, _)| i)
            .collect();
        for i in to_remove.into_iter().rev() {
            aot.remove(i);
        }
    }

    tokio::fs::write(CONFIG_PATH, doc.to_string()).await?;
    Ok(())
}

async fn read_catalog() -> anyhow::Result<Vec<Value>> {
    let content = tokio::fs::read_to_string(CONFIG_PATH).await?;
    let doc: DocumentMut = content.parse()?;

    let Some(Item::ArrayOfTables(aot)) = doc.get("mcp_catalog") else {
        return Ok(vec![]);
    };

    let entries = aot
        .iter()
        .map(|t| {
            let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let description = t
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let command = t
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args: Vec<String> = t
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            json!({ "name": name, "description": description, "command": command, "args": args })
        })
        .collect();

    Ok(entries)
}
