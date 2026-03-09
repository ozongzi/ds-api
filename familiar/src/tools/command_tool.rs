use ds_api::tool;
use serde_json::json;
use tokio::process::Command;

use super::{MAX_OUTPUT_CHARS, truncate_output};

pub struct CommandTool;

#[tool]
impl Tool for CommandTool {
    /// 跨平台执行终端命令
    /// command: 需要执行的终端命令
    /// cwd: 工作目录（可选），不传则使用服务器默认目录
    async fn execute(&self, command: String, cwd: Option<String>) -> Value {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&command);

        if let Some(dir) = cwd {
            cmd.current_dir(&dir);
        }

        let output = match cmd.output().await {
            Ok(o) => o,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let stdout = truncate_output(
            String::from_utf8_lossy(&output.stdout).trim(),
            MAX_OUTPUT_CHARS,
        );
        let stderr = truncate_output(
            String::from_utf8_lossy(&output.stderr).trim(),
            MAX_OUTPUT_CHARS,
        );

        json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": output.status.code(),
        })
    }
}
