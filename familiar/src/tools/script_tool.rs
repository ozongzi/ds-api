use ds_api::tool;
use serde_json::json;
use tokio::process::Command;

use super::{MAX_OUTPUT_CHARS, truncate_output};

pub struct ScriptTool;

#[tool]
impl Tool for ScriptTool {
    /// 运行 TypeScript 脚本（使用 Bun 作为运行时）
    /// script: 脚本内容
    async fn run_ts(&self, script: String) -> Value {
        let tmp_path = "/tmp/familiar_script.ts";
        if let Err(e) = tokio::fs::write(tmp_path, &script).await {
            return json!({ "error": format!("写入脚本失败: {}", e) });
        }

        match Command::new("bun").arg(tmp_path).output().await {
            Ok(output) => {
                let stdout = truncate_output(
                    String::from_utf8_lossy(&output.stdout).trim(),
                    MAX_OUTPUT_CHARS,
                );
                let stderr = truncate_output(
                    String::from_utf8_lossy(&output.stderr).trim(),
                    MAX_OUTPUT_CHARS,
                );
                json!({
                    "exit_code": output.status.code(),
                    "stdout": stdout,
                    "stderr": stderr,
                })
            }
            Err(e) => json!({ "error": format!("Failed to run script: {}", e) }),
        }
    }

    /// 运行 Python 脚本（使用 uv run 作为运行时）
    /// script: 脚本内容
    async fn run_py(&self, script: String) -> Value {
        let tmp_path = "/tmp/familiar_script.py";
        if let Err(e) = tokio::fs::write(tmp_path, &script).await {
            return json!({ "error": format!("写入脚本失败: {}", e) });
        }

        match Command::new("uv").args(["run", tmp_path]).output().await {
            Ok(output) => {
                let stdout = truncate_output(
                    String::from_utf8_lossy(&output.stdout).trim(),
                    MAX_OUTPUT_CHARS,
                );
                let stderr = truncate_output(
                    String::from_utf8_lossy(&output.stderr).trim(),
                    MAX_OUTPUT_CHARS,
                );
                json!({
                    "exit_code": output.status.code(),
                    "stdout": stdout,
                    "stderr": stderr,
                })
            }
            Err(e) => json!({ "error": format!("Failed to run script: {}", e) }),
        }
    }
}
