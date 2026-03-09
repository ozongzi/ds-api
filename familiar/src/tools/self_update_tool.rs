use ds_api::tool;
use serde_json::json;
use tokio::process::Command;

/// Working directory of the ds-api-workspace on the server.
/// Adjust this path if the workspace lives somewhere else.
const WORKSPACE: &str = "/root/ds-api-workspace";

pub struct SelfUpdateTool;

#[tool]
impl Tool for SelfUpdateTool {
    /// 构建 familiar 后端（不部署）。
    /// 用于在修改源码后验证代码能否编译通过。
    /// 返回编译器输出（stdout + stderr）和退出码。
    async fn build_familiar(&self) -> Value {
        run_make("build", WORKSPACE).await
    }

    /// 构建前端 React/Vite 客户端（不部署）。
    /// 返回构建工具输出和退出码。
    async fn build_client(&self) -> Value {
        run_make("build-client", WORKSPACE).await
    }

    /// 完整构建（前端 + 后端）并部署到生产服务器，然后重启 familiar 服务。
    /// 注意：重启会导致当前 WebSocket 连接短暂中断，客户端会自动重连。
    /// 只在编译验证通过后调用此工具。
    async fn deploy_familiar(&self) -> Value {
        run_make("deploy", WORKSPACE).await
    }

    /// 读取 familiar 当前运行的 systemd 服务日志（最近 N 行）。
    /// 用于在部署后确认新版本启动正常，或排查运行时错误。
    /// lines: 要获取的日志行数，默认 50
    async fn service_logs(&self, lines: Option<u32>) -> Value {
        let n = lines.unwrap_or(50).to_string();
        let output = Command::new("journalctl")
            .args(["-u", "familiar", "-n", &n, "--no-pager"])
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                json!({
                    "logs": stdout,
                    "stderr": stderr,
                    "exit_code": out.status.code(),
                })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 查看 familiar 服务当前状态（是否运行、PID、uptime 等）。
    async fn service_status(&self) -> Value {
        let output = Command::new("systemctl")
            .args(["status", "familiar", "--no-pager"])
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                json!({
                    "status": stdout,
                    "exit_code": out.status.code(),
                })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 获取当前运行的 familiar 二进制的版本信息（来自 Cargo.toml）。
    async fn current_version(&self) -> Value {
        let cargo_toml_path = format!("{WORKSPACE}/familiar/Cargo.toml");
        match tokio::fs::read_to_string(&cargo_toml_path).await {
            Ok(content) => {
                // 从 Cargo.toml 里解析 version 字段
                let version = content
                    .lines()
                    .find(|l| l.trim_start().starts_with("version"))
                    .and_then(|l| l.splitn(2, '=').nth(1))
                    .map(|v| v.trim().trim_matches('"').to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                json!({
                    "version": version,
                    "cargo_toml": cargo_toml_path,
                })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}

/// Run `make <target>` in `cwd`, capturing stdout + stderr.
async fn run_make(target: &str, cwd: &str) -> serde_json::Value {
    let output = Command::new("make")
        .arg(target)
        .current_dir(cwd)
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let exit_code = out.status.code().unwrap_or(-1);
            let success = out.status.success();
            json!({
                "success": success,
                "exit_code": exit_code,
                "stdout": stdout,
                "stderr": stderr,
            })
        }
        Err(e) => json!({
            "success": false,
            "error": e.to_string(),
        }),
    }
}
