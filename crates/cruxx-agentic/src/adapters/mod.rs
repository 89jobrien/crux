pub mod anthropic;
pub mod container_client;
pub mod ollama;
pub mod openai;
pub mod terminal_approval;

pub use anthropic::AnthropicAdapter;
pub use ollama::OllamaAdapter;
pub use openai::OpenAiAdapter;
