use ds_api::tool;
use serde_json::json;
use tokio::process::Command;
use uuid::Uuid;

use super::{MAX_OUTPUT_CHARS, truncate_output};

pub struct ScriptSpell;

#[tool]
impl Tool for ScriptSpell {
    /// 运行 TypeScript 脚本（使用 Bun 作为运行时）。
    /// 可直接在脚本顶部用 import 引入 npm 包，Bun 会自动安装，无需额外声明依赖。
    /// 示例：import { format } from "date-fns";
    /// script: 脚本内容
    async fn run_ts(&self, script: String) -> Value {
        let id = Uuid::new_v4().simple();
        let tmp_path = format!("/tmp/familiar_{id}.ts");

        if let Err(e) = tokio::fs::write(&tmp_path, &script).await {
            return json!({ "error": format!("写入脚本失败: {}", e) });
        }

        let result = Command::new("bun").arg(&tmp_path).output().await;
        let _ = tokio::fs::remove_file(&tmp_path).await;

        match result {
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
            Err(e) => json!({ "error": format!("执行脚本失败: {}", e) }),
        }
    }

    /// 运行 Python 脚本（使用 uv run 作为运行时）。
    /// 可在脚本顶部用 PEP 723 inline metadata 声明依赖，uv 会自动安装：
    ///
    /// # /// script
    /// # requires-python = ">=3.11"
    /// # dependencies = ["requests", "rich>=13"]
    /// # ///
    ///
    /// script: 脚本内容
    async fn run_py(&self, script: String) -> Value {
        let id = Uuid::new_v4().simple();
        let tmp_path = format!("/tmp/familiar_{id}.py");

        if let Err(e) = tokio::fs::write(&tmp_path, &script).await {
            return json!({ "error": format!("写入脚本失败: {}", e) });
        }

        let result = Command::new("uv").args(["run", &tmp_path]).output().await;
        let _ = tokio::fs::remove_file(&tmp_path).await;

        match result {
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
            Err(e) => json!({ "error": format!("执行脚本失败: {}", e) }),
        }
    }
}
