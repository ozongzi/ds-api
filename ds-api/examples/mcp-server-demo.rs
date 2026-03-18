use ds_api::{McpServer, ToolBundle, tool};

struct Calculator;

#[tool]
impl ds_api::Tool for Calculator {
    /// Add two numbers.
    /// a: first operand
    /// b: second operand
    async fn add(&self, a: f64, b: f64) -> f64 {
        a + b
    }

    /// Multiply two numbers.
    /// a: first operand
    /// b: second operand
    async fn multiply(&self, a: f64, b: f64) -> f64 {
        a * b
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    McpServer::new(ToolBundle::new().add(Calculator))
        .with_name("my-calc-server")
        .serve_stdio()
        .await?;
    Ok(())
}
