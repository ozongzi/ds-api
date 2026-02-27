use ds_api::error::Result;
use ds_api::simple_chatter::SimpleChatter;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<()> {
    let token = match std::env::var("DEEPSEEK_API_KEY") {
        Ok(t) => t,
        Err(_) => {
            let mut token = String::new();
            print!("Input the token:");
            std::io::stdout().flush()?;
            std::io::stdin().read_line(&mut token)?;
            token.trim().to_string()
        }
    };
    let system_prompt = "You are a helpful assistant.".to_string();
    let mut chatter = SimpleChatter::new(token, system_prompt);

    loop {
        let mut user_input = String::new();
        print!("User: ");
        std::io::stdout().flush()?;
        std::io::stdin().read_line(&mut user_input)?;
        let user_input = user_input.trim();
        if user_input.is_empty() {
            break;
        }

        let response = chatter.chat(user_input).await?;
        println!("Assistant: {}", response);
    }

    Ok(())
}
