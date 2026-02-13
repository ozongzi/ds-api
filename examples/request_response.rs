use ds_api::request::*;
use ds_api::response::*;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut token = String::new();
    println!("Please input your API token:");
    std::io::stdin().read_line(&mut token).unwrap();
    token = token.trim().to_string();

    let response = Request::basic_query(vec![
        Message::new(Role::System, "You are a helpful assistant."),
        Message::new(Role::User, "What is the capital of France?"),
    ])
    .execute_nostreaming(&token)
    .await?;

    println!("Response :{}", response.content());

    Ok(())
}
