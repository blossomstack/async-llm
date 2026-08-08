use async_llm::{types::CreateMessagesRequestBuilder, Client};

#[test]
fn root_anthropic_exports_remain_available() {
    let _client = Client::default();
    let _builder = CreateMessagesRequestBuilder::default();
}
