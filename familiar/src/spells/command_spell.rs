use ds_api::tool;
use serde_json::{Value, json};
use std::{env, path::PathBuf};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use super::{MAX_OUTPUT_CHARS, truncate_output};

fn get_cwd(cwd: Option<String>) -> Result<PathBuf, Value> {
    // Determine default home dir: HOME, then USERPROFILE, otherwise /root
    let default_home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .unwrap_or_else(|_| String::from("/root"));

    let current_dir: PathBuf;

    if let Some(dir) = cwd {
        // If provided cwd is relative, resolve it relative to the default_home and canonicalize.
        let path = std::path::Path::new(&dir);
        if path.is_relative() {
            let joined = std::path::Path::new(&default_home).join(path);
            match std::fs::canonicalize(&joined) {
                Ok(p) => {
                    current_dir = PathBuf::from(p);
                }
                Err(e) => {
                    return Err(json!({
                        "error": format!("failed to canonicalize cwd '{}': {}", joined.display(), e)
                    }));
                }
            }
        } else {
            // absolute path: use as-is
            current_dir = PathBuf::from(path);
        }
    } else {
        // No cwd provided: use resolved default_home
        current_dir = PathBuf::from(default_home);
    }

    Ok(current_dir)
}

pub struct CommandSpell;

#[tool]
impl Tool for CommandSpell {
    /// 跨平台执行终端命令
    /// command: 需要执行的终端命令
    /// cwd: 工作目录（可选），不传则使用服务器默认目录
    /// timeout_secs: 超时时间（秒，可选，默认为 20）
    async fn execute(
        &self,
        command: String,
        cwd: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Value {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&command);
        cmd.current_dir(match get_cwd(cwd) {
            Ok(p) => p,
            Err(e) => return json!({ "error": e.to_string() }),
        });

        let timeout_secs = timeout_secs.unwrap_or(20);

        let result = match timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
            Ok(output_res) => output_res,
            Err(_) => return json!({ "error": "command timed out" }),
        };

        let output = match result {
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
