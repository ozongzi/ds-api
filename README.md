# ds-api

[![Crates.io](https://img.shields.io/crates/v/ds-api.svg)](https://crates.io/crates/ds-api)
[![Documentation](https://docs.rs/ds-api/badge.svg)](https://docs.rs/ds-api)
[![License](https://img.shields.io/crates/l/ds-api.svg)](https://crates.io/crates/ds-api)

一个 Rust 客户端库，用于与 DeepSeek API 进行交互。支持聊天补全、流式响应、工具调用等功能。

## 特性

- **完整的 API 支持**: 支持 DeepSeek API 的所有功能，包括聊天补全、流式响应、工具调用等
- **类型安全**: 使用 Rust 的强类型系统确保 API 请求和响应的正确性
- **异步支持**: 基于 `tokio` 和 `reqwest` 的异步实现
- **流式响应**: 支持 Server-Sent Events (SSE) 流式响应
- **工具调用**: 支持函数调用和工具选择
- **JSON 模式**: 支持 JSON 格式的响应
- **推理模式**: 支持 DeepSeek Reasoner 模型的推理功能

## 安装

在 `Cargo.toml` 中添加：

```toml
[dependencies]
ds-api = "0.1"
```

## 快速开始

### 基本使用

```rust
use ds_api::{Request, Message, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "your_deepseek_api_token".to_string();
    
    // 创建请求
    let request = Request::basic_query(vec![
        Message {
            role: Role::User,
            content: Some("Hello, how are you?".to_string()),
            ..Default::default()
        }
    ]);
    
    // 执行请求
    let response = request.execute_nostreaming(&token).await?;
    
    println!("Response: {}", response.content());
    Ok(())
}
```

### 使用 SimpleChatter（推荐）

```rust
use ds_api::SimpleChatter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "your_deepseek_api_token".to_string();
    let system_prompt = "You are a helpful assistant.".to_string();
    
    // 创建聊天客户端
    let mut chatter = SimpleChatter::new(token, system_prompt);
    
    // 发送消息
    let response = chatter.chat("What is the capital of France?").await?;
    println!("Assistant: {}", response);
    
    // 发送另一条消息（保持对话历史）
    let response = chatter.chat("Tell me more about it.").await?;
    println!("Assistant: {}", response);
    
    Ok(())
}
```

### 流式响应

```rust
use ds_api::{Request, Message, Role};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "your_deepseek_api_token".to_string();
    let client = reqwest::Client::new();
    
    let request = Request::basic_query(vec![
        Message {
            role: Role::User,
            content: Some("Tell me a story about Rust.".to_string()),
            ..Default::default()
        }
    ]);
    
    let mut stream = request.execute_client_streaming(&client, &token).await?;
    
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                if let Some(content) = chunk.choices[0].delta.content.as_ref() {
                    print!("{}", content);
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        }
    }
    
    Ok(())
}
```

### 使用工具调用

```rust
use ds_api::{Request, Message, Role, Tool, ToolChoice, ToolChoiceType};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "your_deepseek_api_token".to_string();
    
    let request = Request::basic_query(vec![
        Message {
            role: Role::User,
            content: Some("What's the weather like in Tokyo?".to_string()),
            ..Default::default()
        }
    ])
    .add_tool(Tool {
        r#type: ds_api::ToolType::Function,
        function: ds_api::Function {
            name: "get_weather".to_string(),
            description: Some("Get the current weather for a location".to_string()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "The city and country, e.g. Tokyo, Japan"
                    }
                },
                "required": ["location"]
            }),
            strict: Some(true),
        },
    })
    .tool_choice_type(ToolChoiceType::Auto);
    
    let response = request.execute_nostreaming(&token).await?;
    
    println!("Response: {:?}", response);
    Ok(())
}
```

### JSON 模式响应

```rust
use ds_api::{Request, Message, Role};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "your_deepseek_api_token".to_string();
    
    let request = Request::basic_query(vec![
        Message {
            role: Role::System,
            content: Some("You are a helpful assistant that always responds in valid JSON format.".to_string()),
            ..Default::default()
        },
        Message {
            role: Role::User,
            content: Some("Give me information about Paris in JSON format with fields: name, country, population, and landmarks.".to_string()),
            ..Default::default()
        }
    ])
    .json(); // 启用 JSON 模式
    
    let response = request.execute_nostreaming(&token).await?;
    
    // 解析 JSON 响应
    let json_value: Value = serde_json::from_str(response.content())?;
    println!("JSON response: {}", serde_json::to_string_pretty(&json_value)?);
    
    Ok(())
}
```

## 模块结构

### 主要模块

- `request::Request`: 高级请求构建器，提供类型安全的 API
- `response::Response`: 响应 trait，提供统一的内容访问接口
- `normal_chatter::NormalChatter`: 支持自定义历史记录管理的聊天客户端
- `simple_chatter::SimpleChatter`: 简化的聊天客户端，内置历史记录管理

### 原始数据结构（`raw` 模块）

- `raw::request`: 请求相关的数据结构
  - `ChatCompletionRequest`: 聊天补全请求
  - `Message`: 消息结构
  - `Model`: 模型枚举（DeepseekChat, DeepseekReasoner）
  - `Tool`: 工具定义
  - `ResponseFormat`: 响应格式
  - `Thinking`: 推理模式配置
- `raw::response`: 响应相关的数据结构
  - `ChatCompletionResponse`: 非流式响应
  - `ChatCompletionChunk`: 流式响应块

## 高级功能

### 自定义历史记录管理

```rust
use ds_api::{NormalChatter, History, Message, Role};

struct MyHistory {
    messages: Vec<Message>,
    max_length: usize,
}

impl History for MyHistory {
    fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        // 自动截断历史记录
        if self.messages.len() > self.max_length {
            self.messages.remove(0);
        }
    }
    
    fn get_history(&self) -> Vec<Message> {
        self.messages.clone()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "your_deepseek_api_token".to_string();
    let mut chatter = NormalChatter::new(token);
    let mut history = MyHistory {
        messages: vec![],
        max_length: 10,
    };
    
    let response = chatter.chat("Hello!", &mut history).await?;
    println!("Response: {}", response);
    
    Ok(())
}
```

### 使用 DeepSeek Reasoner 模型

```rust
use ds_api::{Request, Message, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "your_deepseek_api_token".to_string();
    
    let request = Request::basic_query_reasoner(vec![
        Message {
            role: Role::User,
            content: Some("Solve this math problem: What is 15% of 200?".to_string()),
            ..Default::default()
        }
    ]);
    
    let response = request.execute_nostreaming(&token).await?;
    println!("Response: {}", response.content());
    
    Ok(())
}
```

## 配置选项

### 请求参数

- `temperature`: 采样温度（0.0-2.0）
- `max_tokens`: 最大生成 token 数
- `top_p`: 核心采样参数（0.0-1.0）
- `frequency_penalty`: 频率惩罚（-2.0-2.0）
- `presence_penalty`: 存在惩罚（-2.0-2.0）
- `stop`: 停止词列表
- `logprobs`: 是否返回 token 对数概率
- `top_logprobs`: 返回 top N 的 token 对数概率

### 响应格式

- `text()`: 文本模式（默认）
- `json()`: JSON 模式

## 错误处理

库使用 `Box<dyn std::error::Error>` 作为错误类型，可以捕获以下类型的错误：

- 网络错误（reqwest）
- JSON 解析错误（serde_json）
- API 错误（HTTP 状态码非 200）
- 流式响应解析错误

## 示例

查看 `examples/` 目录获取更多示例：

```bash
# 运行基本示例
cargo run --example basic_usage

# 运行流式响应示例
cargo run --example streaming

# 运行工具调用示例
cargo run --example tools
```

## 文档

生成本地文档：

```bash
cargo doc --open
```

在线文档：https://docs.rs/ds-api

## 许可证

本项目采用 MIT 或 Apache-2.0 双重许可证。

## 贡献

欢迎提交 Issue 和 Pull Request！

## 相关项目

- [openai-rust](https://github.com/64bit/async-openai): OpenAI API 的 Rust 客户端
- [anthropic-rs](https://github.com/anthropics/anthropic-rs): Anthropic API 的 Rust 客户端

## 支持

如有问题，请：
1. 查看 [API 文档](https://api-docs.deepseek.com/zh-cn/api/create-chat-completion)
2. 提交 [GitHub Issue](https://github.com/yourusername/ds-api/issues)
3. 查看示例代码

---

**注意**: 使用本库需要有效的 DeepSeek API 密钥。请访问 [DeepSeek 平台](https://platform.deepseek.com/) 获取 API 密钥。