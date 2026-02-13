use ds_api::simple_chatter::SimpleChatter;

const TOKEN: &str = "YOUR TOKEN HERE OR SET ENV VAR DEEPSEEK_API_KEY";

#[tokio::main]
async fn main() {
    let token = std::env::var("DEEPSEEK_API_KEY").unwrap_or(TOKEN.to_string());
    let system_prompt = "You are a helpful assistant.".to_string();

    // Initialize the SimpleChatter with the token and system prompt
    let mut chatter = SimpleChatter::new(token, system_prompt);

    // chat and get response
    let response = chatter
        .chat_json("Return a json with name=John, grade=1")
        .await
        .unwrap();
    println!("{}", response);
}
