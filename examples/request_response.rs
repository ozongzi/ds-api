use ds_api::error::Result;
use ds_api::request::*;
use ds_api::response::*;

#[tokio::main]
async fn main() -> Result<()> {
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

    let content = response.content()?;
    println!("Response :{}", content);

    Ok(())
}
