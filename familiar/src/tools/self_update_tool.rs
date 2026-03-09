use ds_api::tool;
use serde_json::json;
use tokio::process::Command;

/// Root of the workspace on the server.
const WORKSPACE: &str = "/root";

/// Path to cargo binary.
const CARGO: &str = "/root/.cargo/bin/cargo";

pub struct SelfUpdateTool;

#[tool]
impl Tool for SelfUpdateTool {
    /// 构建 familiar 后端（不部署）。
    /// 用于在修改源码后验证代码能否编译通过。
    /// 返回编译器输出（stdout + stderr）和退出码。
    async fn build_familiar(&self) -> Value {
        run_cargo(&["build", "-p", "familiar", "--release"], WORKSPACE).await
    }

    /// 完整构建后端并替换二进制，然后重启 familiar 服务。
    /// 注意：重启会导致当前 WebSocket 连接短暂中断，客户端会自动重连。
    /// 只在 build_familiar 验证编译通过后调用此工具。
    async fn deploy_familiar(&self) -> Value {
        // 1. build
        let build = run_cargo(&["build", "-p", "familiar", "--release"], WORKSPACE).await;
        if !build
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return build;
        }

        // 2. stop service, copy binary, start service
        let steps: &[(&str, &[&str])] = &[
            ("systemctl", &["stop", "familiar"]),
            (
                "cp",
                &["/root/target/release/familiar", "/usr/local/bin/familiar"],
            ),
            ("systemctl", &["start", "familiar"]),
        ];

        for (cmd, args) in steps {
            let out = Command::new(cmd).args(*args).output().await;
            match out {
                Ok(o) if !o.status.success() => {
                    return json!({
                        "success": false,
                        "step": cmd,
                        "stderr": String::from_utf8_lossy(&o.stderr).to_string(),
                        "exit_code": o.status.code(),
                    });
                }
                Err(e) => {
                    return json!({ "success": false, "step": cmd, "error": e.to_string() });
                }
                _ => {}
            }
        }

        json!({ "success": true, "message": "deployed and restarted" })
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

/// Run cargo with the given args in `cwd`, capturing stdout + stderr.
async fn run_cargo(args: &[&str], cwd: &str) -> serde_json::Value {
    let output = Command::new(CARGO)
        .args(args)
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
