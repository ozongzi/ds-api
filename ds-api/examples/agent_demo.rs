use ds_api::{DeepseekAgent, tool};
use futures::StreamExt;
use reqwest::Client;
use serde_json::{Value, json};

struct WeatherTool {
    client: Client,
}

#[tool]
impl Tool for WeatherTool {
    /// Get current weather for a city.
    /// city: city name
    /// unit: temperature unit, e.g., "celsius" or "fahrenheit" (optional)
    async fn get_weather(&self, city: String, unit: Option<String>) -> Value {
        let _ = unit;
        let url = format!("https://wttr.in/{}?format=3", city);
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
    // Ensure DEEPSEEK_API_KEY is set in your environment before running this example.
    let token = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY must be set");

    let agent = DeepseekAgent::new(token)
        .add_tool(WeatherTool {
            client: Client::new(),
        })
        .with_system_prompt("You are a helpful assistant.");

    // Ask the agent to check weather for two cities.
    let mut stream = agent.chat("Check the weather for Beijing and Shanghai");

    while let Some(response) = stream.next().await {
        if let Some(content) = &response.content {
            println!("Assistant: {}", content);
        }
        for tc in &response.tool_calls {
            println!("Tool call {}({})", tc.name, tc.args);
            if tc.result != Value::Null {
                println!("-> {}", tc.result);
            }
        }
    }
}
