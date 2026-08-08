#[cfg(feature = "mock")]
use async_llm::mock::MockLlmServer;
#[cfg(feature = "openai")]
use async_llm::openai::Client as OpenAiClient;
#[cfg(feature = "responses")]
use async_llm::responses::Client as ResponsesClient;

#[test]
fn feature_gated_public_modules_compile() {
    #[cfg(feature = "openai")]
    let _ = std::any::TypeId::of::<OpenAiClient>();

    #[cfg(feature = "responses")]
    let _ = std::any::TypeId::of::<ResponsesClient>();

    #[cfg(feature = "mock")]
    let _ = std::any::TypeId::of::<MockLlmServer>();
}
