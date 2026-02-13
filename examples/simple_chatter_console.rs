use ds_api::simple_chatter::SimpleChatter;
use std::io::Write;

#[tokio::main]
async fn main() {
    let token = std::env::var("DEEPSEEK_API_KEY").unwrap_or({
        let mut token = String::new();
        print!("Input the token:");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut token).unwrap();
        token.trim().to_string()
    });
    let system_prompt = "You are a helpful assistant.".to_string();
    let mut chatter = SimpleChatter::new(token, system_prompt);

    loop {
        let mut user_input = String::new();
        print!("User: ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut user_input).unwrap();
        let user_input = user_input.trim();
        if user_input.is_empty() {
            break;
        }

        let response = chatter.chat(user_input).await.unwrap();
        println!("Assistant: {}", response);
    }
}
