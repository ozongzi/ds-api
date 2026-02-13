//! 综合示例：展示 ds-api 库的所有主要功能
//!
//! 这个示例展示了如何使用 ds-api 库的各种功能，包括：
//! 1. 基本请求构建
//! 2. 流式响应
//! 3. 工具调用
//! 4. JSON 模式
//! 5. 使用 SimpleChatter
//! 6. 使用 NormalChatter 和自定义历史记录
//! 7. 使用 DeepSeek Reasoner 模型

use ds_api::{
    ChatCompletionResponse, History, Message, Model, NormalChatter, Request, Response, Role,
    SimpleChatter, Tool, ToolChoiceType,
};
use futures::StreamExt;
use reqwest::Client;
use serde_json::json;
use std::error::Error;

const TOKEN_ENV_VAR: &str = "DEEPSEEK_API_KEY";

/// 获取 API 令牌
fn get_token() -> Result<String, Box<dyn Error>> {
    std::env::var(TOKEN_ENV_VAR)
        .map_err(|_| format!("请设置环境变量 {} 或修改代码中的 TOKEN 常量", TOKEN_ENV_VAR).into())
}

/// 示例 1: 基本请求构建和执行
async fn example_basic_request() -> Result<(), Box<dyn Error>> {
    println!("=== 示例 1: 基本请求构建和执行 ===");

    let token = get_token()?;

    // 使用构建器模式创建请求
    let request = Request::builder()
        .add_message(Message::new(Role::System, "You are a helpful assistant."))
        .add_message(Message::new(Role::User, "What is the capital of France?"))
        .model(Model::DeepseekChat)
        .temperature(0.7)
        .max_tokens(100);

    // 执行请求
    let response: ChatCompletionResponse = request.execute_nostreaming(&token).await?;

    println!("响应内容: {}", response.content());
    println!("模型: {:?}", response.model);
    println!("Token 使用: {:?}", response.usage);
    println!();

    Ok(())
}

/// 示例 2: 流式响应
async fn example_streaming_response() -> Result<(), Box<dyn Error>> {
    println!("=== 示例 2: 流式响应 ===");

    let token = get_token()?;
    let client = Client::new();

    // 创建流式请求
    let request = Request::basic_query(vec![Message::new(
        Role::User,
        "用中文讲一个关于 Rust 编程语言的简短故事。",
    )]);

    println!("开始接收流式响应...");

    let stream = request.execute_client_streaming(&client, &token).await?;
    let mut full_response = String::new();

    // 使用 pin! 宏来固定流
    use futures::pin_mut;
    pin_mut!(stream);

    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                if let Some(content) = chunk.choices[0].delta.content.as_ref() {
                    print!("{}", content);
                    full_response.push_str(content);
                }
            }
            Err(e) => eprintln!("流式响应错误: {}", e),
        }
    }

    println!("\n\n完整响应长度: {} 字符", full_response.len());
    println!();

    Ok(())
}

/// 示例 3: 工具调用
async fn example_tool_calling() -> Result<(), Box<dyn Error>> {
    println!("=== 示例 3: 工具调用 ===");

    let token = get_token()?;

    // 定义工具（函数）
    let weather_tool = Tool {
        r#type: ds_api::ToolType::Function,
        function: ds_api::Function {
            name: "get_weather".to_string(),
            description: Some("获取指定城市的天气信息".to_string()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "城市名称，例如：北京、上海"
                    },
                    "unit": {
                        "type": "string",
                        "enum": ["celsius", "fahrenheit"],
                        "description": "温度单位"
                    }
                },
                "required": ["location"]
            }),
            strict: Some(true),
        },
    };

    let request = Request::basic_query(vec![Message::new(Role::User, "北京现在的天气怎么样？")])
        .add_tool(weather_tool)
        .tool_choice_type(ToolChoiceType::Auto);

    let response = request.execute_nostreaming(&token).await?;

    println!("响应: {:?}", response);

    // 检查是否有工具调用
    if let Some(tool_calls) = &response.choices[0].message.tool_calls {
        println!("\n检测到工具调用:");
        for tool_call in tool_calls {
            println!("  函数: {}", tool_call.function.name);
            println!("  参数: {}", tool_call.function.arguments);
        }
    } else {
        println!("\n没有工具调用，直接回复: {}", response.content());
    }

    println!();

    Ok(())
}

/// 示例 4: JSON 模式响应
async fn example_json_mode() -> Result<(), Box<dyn Error>> {
    println!("=== 示例 4: JSON 模式响应 ===");

    let token = get_token()?;

    let request = Request::builder()
        .add_message(Message::new(
            Role::System,
            "你是一个有用的助手，总是以有效的 JSON 格式响应。",
        ))
        .add_message(Message::new(
            Role::User,
            "以 JSON 格式提供巴黎的信息，包含字段：name、country、population、landmarks。",
        ))
        .json() // 启用 JSON 模式
        .temperature(0.3);

    let response = request.execute_nostreaming(&token).await?;

    // 解析 JSON 响应
    let json_value: serde_json::Value = serde_json::from_str(response.content())?;

    println!("JSON 响应:");
    println!("{}", serde_json::to_string_pretty(&json_value)?);
    println!();

    Ok(())
}

/// 示例 5: 使用 SimpleChatter
async fn example_simple_chatter() -> Result<(), Box<dyn Error>> {
    println!("=== 示例 5: 使用 SimpleChatter ===");

    let token = get_token()?;
    let system_prompt = "你是一个有用的助手，专门回答关于编程的问题。".to_string();

    let mut chatter = SimpleChatter::new(token, system_prompt);

    // 第一次对话
    let response1 = chatter.chat("Rust 语言的主要特点是什么？").await?;
    println!("助手: {}", response1);

    // 第二次对话（保持历史）
    let response2 = chatter.chat("这些特点如何帮助编写安全的系统软件？").await?;
    println!("助手: {}", response2);

    // 查看历史记录长度
    println!("历史记录消息数: {}", chatter.history.len());
    println!();

    Ok(())
}

/// 自定义历史记录实现
struct LimitedHistory {
    messages: Vec<Message>,
    max_messages: usize,
}

impl History for LimitedHistory {
    fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        // 自动截断，保持最多 max_messages 条消息
        if self.messages.len() > self.max_messages {
            self.messages.remove(0);
        }
    }

    fn get_history(&self) -> Vec<Message> {
        self.messages.clone()
    }
}

/// 示例 6: 使用 NormalChatter 和自定义历史记录
async fn example_normal_chatter() -> Result<(), Box<dyn Error>> {
    println!("=== 示例 6: 使用 NormalChatter 和自定义历史记录 ===");

    let token = get_token()?;
    let mut chatter = NormalChatter::new(token);

    // 创建自定义历史记录，最多保存 5 条消息
    let mut history = LimitedHistory {
        messages: vec![Message::new(
            Role::System,
            "你是一个简洁的助手，回答要简短。",
        )],
        max_messages: 5,
    };

    // 进行多次对话
    let questions = [
        "什么是人工智能？",
        "机器学习有哪些类型？",
        "深度学习是什么？",
        "神经网络如何工作？",
        "Transformer 模型是什么？",
        "BERT 模型有什么特点？", // 这条会触发历史记录截断
    ];

    for (i, question) in questions.iter().enumerate() {
        let response = chatter.chat(question, &mut history).await?;
        println!("问题 {}: {}", i + 1, question);
        println!("回答: {}", response);
        println!("当前历史记录长度: {}", history.messages.len());
        println!();
    }

    Ok(())
}

/// 示例 7: 使用 DeepSeek Reasoner 模型
async fn example_reasoner_model() -> Result<(), Box<dyn Error>> {
    println!("=== 示例 7: 使用 DeepSeek Reasoner 模型 ===");

    let token = get_token()?;

    let request = Request::basic_query_reasoner(vec![
        Message::new(Role::System, "你是一个数学助手，请展示你的推理过程。"),
        Message::new(Role::User, "一个水池有一个进水管和一个出水管。进水管单独注满水池需要6小时，出水管单独排空水池需要8小时。如果两个水管同时打开，需要多少小时才能注满水池？"),
    ])
    .max_tokens(300);

    let response = request.execute_nostreaming(&token).await?;

    println!("Reasoner 模型响应:");
    println!("{}", response.content());
    println!();

    Ok(())
}

/// 示例 8: 错误处理
async fn example_error_handling() -> Result<(), Box<dyn Error>> {
    println!("=== 示例 8: 错误处理 ===");

    // 使用无效的令牌测试错误处理
    let invalid_token = "invalid_token".to_string();

    let request = Request::basic_query(vec![Message::new(Role::User, "Hello")]);

    match request.execute_nostreaming(&invalid_token).await {
        Ok(response) => {
            println!("意外成功: {}", response.content());
        }
        Err(e) => {
            println!("预期中的错误: {}", e);
            println!("错误类型: {}", e.to_string());
        }
    }

    println!();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("ds-api 库综合示例");
    println!("==================\n");

    // 检查环境变量
    if std::env::var(TOKEN_ENV_VAR).is_err() {
        println!("注意: 请设置环境变量 {} 来运行所有示例", TOKEN_ENV_VAR);
        println!("部分示例可能需要有效的 API 令牌才能运行\n");
    }

    // 运行示例
    example_basic_request().await?;
    example_streaming_response().await?;
    example_tool_calling().await?;
    example_json_mode().await?;
    example_simple_chatter().await?;
    example_normal_chatter().await?;
    example_reasoner_model().await?;
    example_error_handling().await?;

    println!("所有示例完成！");
    Ok(())
}
