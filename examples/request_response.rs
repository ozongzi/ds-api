use ds_api::Response;
use ds_api::error::Result;
use ds_api::{DeepseekClient, Message, Request, Role};

#[tokio::main]
async fn main() -> Result<()> {
    let mut token = String::new();
    println!("Please input your API token:");
    std::io::stdin().read_line(&mut token).unwrap();
    let token = token.trim().to_string();

    let request = Request::basic_query(vec![
        Message::new(Role::System, "You are a helpful assistant."),
        Message::new(Role::User, "What is the capital of France?"),
    ]);

    let client = DeepseekClient::new(token.clone());
    let response = client.send(request).await?;

    let content = response.content()?;
    println!("Response :{}", content);

    Ok(())
}
