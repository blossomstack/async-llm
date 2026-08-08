#[tokio::main]
async fn main() {
    async_llm::mock::run_cli().await;
}
