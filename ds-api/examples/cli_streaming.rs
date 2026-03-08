use ds_api::{AgentEvent, DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::json;
use std::io::{self, Write};
use std::process::Stdio;
use tokio::process::Command;

struct ShellTool;

#[tool]
impl Tool for ShellTool {
    async fn run(&self, command: String) -> Value {
        let output = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(out) => {
                json!({
                    "command": command,
                    "status": out.status.code(),
                    "stdout": String::from_utf8_lossy(&out.stdout),
                    "stderr": String::from_utf8_lossy(&out.stderr),
                })
            }
            Err(e) => json!({
                "command": command,
                "error": e.to_string(),
            }),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("DEEPSEEK_API_KEY")?;

    // The agent is created per-prompt below to avoid moving `agent` (chat consumes self).
    // Creating it once and calling `.chat(...)` repeatedly would move the agent on first use.

    println!("DeepSeek REPL");
    println!("Type a prompt and press Enter. Ctrl+C to exit.\n");

    let mut line = String::new();

    // Create the agent once and recover it after each `.chat(...)` via `AgentStream::into_agent`.
    // `chat(self, ...)` consumes the agent, but `AgentStream::into_agent` can return it back.
    let mut agent = DeepseekAgent::new(&token)
        .with_streaming()
        .add_tool(ShellTool)
        .with_system_prompt(
            "You may call shell.run(command) to execute shell commands. \
             Avoid destructive operations.",
        );

    loop {
        print!("> ");
        io::stdout().flush()?;

        line.clear();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }

        let prompt = line.trim();

        if prompt.is_empty() {
            continue;
        }

        let mut stream = agent.chat(prompt);

        while let Some(event_res) = stream.next().await {
            match event_res {
                Err(e) => {
                    eprintln!("\nError: {}", e);
                    break;
                }

                Ok(AgentEvent::Token(fragment)) => {
                    print!("{fragment}");
                    io::stdout().flush().ok();
                }
                Ok(AgentEvent::ToolCall(c)) => {
                    println!("\n[calling {}({})]", c.name, c.args);
                }
                Ok(AgentEvent::ToolResult(r)) => {
                    println!("\n[tool result] {} -> {}", r.name, r.result);
                }
            }
        }

        // Recover the agent from the stream so it can be reused for the next prompt.
        if let Some(a) = stream.into_agent() {
            agent = a;
        } else {
            eprintln!("Agent was not returned from stream; exiting.");
            break;
        }

        println!("\n");
    }

    Ok(())
}
