use ds_api::{DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::json;

struct WeatherTool {
    client: reqwest::Client,
}

#[tool]
impl Tool for WeatherTool {
    /// 获取城市实时天气
    /// city: 城市名称
    /// unit: 温度单位，celsius 或 fahrenheit（可选）
    async fn get_weather(&self, city: String, unit: Option<String>) -> Value {
        let url = format!("https://wttr.in/{}?format=3", city);
        let _ = unit;
        let text = match self.client.get(&url).send().await {
            Ok(response) => match response.text().await {
                Ok(body) => body,
                Err(e) => e.to_string(),
            },
            Err(e) => e.to_string(),
        };
        json!({ "city": city, "weather": text })
    }
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DEEPSEEK_API_KEY").expect("需要设置 DEEPSEEK_API_KEY");

    let agent = DeepseekAgent::new(token)
        .add_tool(WeatherTool {
            client: reqwest::Client::new(),
        })
        .with_system_prompt("你是一个助手");

    let mut stream = agent.chat("帮我看看北京和上海的天气");

    while let Some(response) = stream.next().await {
        if let Some(content) = &response.content {
            println!("💬 {}", content);
        }
        for tc in &response.tool_calls {
            println!("🔧 调用 {}({}) → {}", tc.name, tc.args, tc.result);
        }
    }
}
