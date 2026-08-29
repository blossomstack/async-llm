use std::{
    ops::{Deref, DerefMut},
    pin::Pin,
};

use derive_builder::Builder;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use tokio_stream::Stream;

use super::{errors::AnthropicError, messages};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct Usage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    /// Tokens written to the prompt cache on this request.
    /// Populated when one or more `cache_control` markers caused a cache miss.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    /// Tokens served from the prompt cache on this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

/// Marker placed on a content block / tool / system block to define a cache breakpoint.
///
/// A breakpoint caches the entire prefix that precedes it (in serialized order:
/// `tools → system → messages`). Up to four breakpoints per request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CacheControl {
    Ephemeral {
        /// Cache TTL — `"5m"` (default) or `"1h"`. `None` means the API default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ttl: Option<String>,
    },
}

impl CacheControl {
    /// Default 5-minute ephemeral breakpoint.
    #[must_use]
    pub fn ephemeral() -> Self {
        CacheControl::Ephemeral { ttl: None }
    }

    /// Ephemeral breakpoint with an explicit TTL (e.g. `"5m"`, `"1h"`).
    #[must_use]
    pub fn ephemeral_with_ttl(ttl: impl Into<String>) -> Self {
        CacheControl::Ephemeral {
            ttl: Some(ttl.into()),
        }
    }
}

/// A thinking block returned by extended-thinking models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Thinking {
    pub thinking: String,
    /// Provider replay signature. `None` when the provider did not supply one,
    /// or when the caller deliberately dropped it — genuine Anthropic validates
    /// this on replay, but Anthropic-compatible endpoints generally do not.
    /// Omitted from the request when `None`; an empty string is *not* a valid
    /// substitute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl From<Thinking> for MessageContent {
    fn from(thinking: Thinking) -> Self {
        MessageContent::Thinking(thinking)
    }
}

impl From<Thinking> for MessageContentList {
    fn from(thinking: Thinking) -> Self {
        MessageContentList(vec![thinking.into()])
    }
}

/// Configuration for extended thinking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingConfig {
    Enabled { budget_tokens: u32 },
    Disabled,
}

/// Output-shaping controls. Currently just `effort`, which sets reasoning depth
/// on models that support it (`low`/`medium`/`high`/`xhigh`/`max` on current
/// Anthropic models). The accepted set is model-specific, so this is an
/// unvalidated string and the caller owns the vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OutputConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub enum ToolChoice {
    Auto,
    Any,
    Tool(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder, PartialEq, Default)]
#[builder(setter(into, strip_option), default)]
pub struct Message {
    pub role: MessageRole,
    pub content: MessageContentList,
}

impl Message {
    /// Returns all the tool uses in the message
    pub fn tool_uses(&self) -> Vec<ToolUse> {
        self.content
            .0
            .iter()
            .filter(|c| matches!(c, MessageContent::ToolUse(_)))
            .map(|c| match c {
                MessageContent::ToolUse(tool_use) => tool_use.clone(),
                _ => unreachable!(),
            })
            .collect()
    }

    /// Returns the first text content in the message
    pub fn text(&self) -> Option<String> {
        self.content
            .0
            .iter()
            .filter_map(|c| match c {
                MessageContent::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .next()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MessageContentList(pub Vec<MessageContent>);

impl Deref for MessageContentList {
    type Target = Vec<MessageContent>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MessageContentList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    #[default]
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct CreateMessagesRequest {
    pub messages: Vec<Message>,
    pub model: String,
    #[builder(default = messages::DEFAULT_MAX_TOKENS)]
    pub max_tokens: i32,
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub stop_sequences: Option<Vec<String>>,
    #[builder(default = "false")]
    pub stream: bool, // Optional default false
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub temperature: Option<f32>, // 0 < x < 1
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub tool_choice: Option<ToolChoice>,
    // TODO: Type this
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub tools: Option<Vec<serde_json::Map<String, Value>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub top_k: Option<u32>, // > 0
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub top_p: Option<f32>, // 0 < x < 1
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub output_config: Option<OutputConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct CreateMessagesResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub content: Option<Vec<MessageContent>>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

impl CreateMessagesResponse {
    /// Returns the content as Messages so they are more easily reusable
    pub fn messages(&self) -> Vec<Message> {
        let Some(content) = &self.content else {
            return vec![];
        };
        content
            .iter()
            .map(|c| Message {
                role: MessageRole::Assistant,
                content: c.clone().into(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    ToolUse(ToolUse),
    ToolResult(ToolResult),
    Text(Text),
    Thinking(Thinking),
    Image(Image),
    Document(Document),
}

impl MessageContent {
    pub fn as_tool_use(&self) -> Option<&ToolUse> {
        if let MessageContent::ToolUse(tool_use) = self {
            Some(tool_use)
        } else {
            None
        }
    }

    pub fn as_tool_result(&self) -> Option<&ToolResult> {
        if let MessageContent::ToolResult(tool_result) = self {
            Some(tool_result)
        } else {
            None
        }
    }

    pub fn as_text(&self) -> Option<&Text> {
        if let MessageContent::Text(text) = self {
            Some(text)
        } else {
            None
        }
    }

    pub fn as_thinking(&self) -> Option<&Thinking> {
        if let MessageContent::Thinking(thinking) = self {
            Some(thinking)
        } else {
            None
        }
    }

    pub fn as_image(&self) -> Option<&Image> {
        if let MessageContent::Image(image) = self {
            Some(image)
        } else {
            None
        }
    }

    pub fn as_document(&self) -> Option<&Document> {
        if let MessageContent::Document(document) = self {
            Some(document)
        } else {
            None
        }
    }
}

/// Where a media block's bytes come from.
///
/// Anthropic accepts the bytes inline as base64, or a URL it fetches itself.
/// The two are a tagged union on the wire rather than two optional fields, so
/// a block carrying neither — or both — cannot be built.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSource {
    Base64(Base64Source),
    Url(UrlSource),
}

impl Default for MediaSource {
    fn default() -> Self {
        MediaSource::Base64(Base64Source::default())
    }
}

/// Bytes inline. `media_type` is what the API dispatches on, so it must be the
/// real type of `data` — "image/png", "application/pdf" and so on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct Base64Source {
    pub media_type: String,
    /// Standard base64, no data-URL prefix.
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct UrlSource {
    pub url: String,
}

/// An image the model can see.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct Image {
    pub source: MediaSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl Image {
    /// An image from inline bytes, already base64-encoded.
    pub fn base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Image {
            source: MediaSource::Base64(Base64Source {
                media_type: media_type.into(),
                data: data.into(),
            }),
            cache_control: None,
        }
    }
}

impl From<Image> for MessageContent {
    fn from(image: Image) -> Self {
        MessageContent::Image(image)
    }
}

/// A document — a PDF — the model can read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct Document {
    pub source: MediaSource,
    /// Shown to the model as the document's name, when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl Document {
    /// A document from inline bytes, already base64-encoded.
    pub fn base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Document {
            source: MediaSource::Base64(Base64Source {
                media_type: media_type.into(),
                data: data.into(),
            }),
            title: None,
            cache_control: None,
        }
    }
}

impl From<Document> for MessageContent {
    fn from(document: Document) -> Self {
        MessageContent::Document(document)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct ToolUse {
    pub id: String,
    pub input: Value,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl From<ToolUse> for MessageContent {
    fn from(tool_use: ToolUse) -> Self {
        MessageContent::ToolUse(tool_use)
    }
}

impl From<ToolUse> for MessageContentList {
    fn from(tool_use: ToolUse) -> Self {
        MessageContentList(vec![tool_use.into()])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: Option<ToolResultContent>,
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// What a tool answered with.
///
/// A bare string for the common case, or a block list when the tool produced
/// something that is not text — a screenshot, say. Untagged, so a plain string
/// still goes over the wire as a plain string and existing callers are
/// unaffected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
}

impl Default for ToolResultContent {
    fn default() -> Self {
        ToolResultContent::Text(String::new())
    }
}

// Concrete rather than a blanket `AsRef<str>`: a blanket impl would overlap
// with `From<Vec<ToolResultBlock>>` under coherence, since a future `Vec` could
// implement `AsRef<str>`.
impl From<String> for ToolResultContent {
    fn from(s: String) -> Self {
        ToolResultContent::Text(s)
    }
}

impl From<&str> for ToolResultContent {
    fn from(s: &str) -> Self {
        ToolResultContent::Text(s.to_string())
    }
}

impl From<Vec<ToolResultBlock>> for ToolResultContent {
    fn from(blocks: Vec<ToolResultBlock>) -> Self {
        ToolResultContent::Blocks(blocks)
    }
}

/// A block inside a `tool_result`.
///
/// Only text and images: Anthropic accepts nothing else there, and reusing
/// [`MessageContent`] would let a caller nest a tool result inside a tool
/// result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultBlock {
    Text(Text),
    Image(Image),
}

impl From<Text> for ToolResultBlock {
    fn from(text: Text) -> Self {
        ToolResultBlock::Text(text)
    }
}

impl From<Image> for ToolResultBlock {
    fn from(image: Image) -> Self {
        ToolResultBlock::Image(image)
    }
}

impl From<ToolResult> for MessageContent {
    fn from(tool_result: ToolResult) -> Self {
        MessageContent::ToolResult(tool_result)
    }
}

impl From<ToolResult> for MessageContentList {
    fn from(tool_result: ToolResult) -> Self {
        MessageContentList(vec![tool_result.into()])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct Text {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl<S: AsRef<str>> From<S> for Text {
    fn from(s: S) -> Self {
        Text {
            text: s.as_ref().to_string(),
            ..Default::default()
        }
    }
}

impl From<Text> for MessageContent {
    fn from(text: Text) -> Self {
        MessageContent::Text(text)
    }
}

impl From<Text> for MessageContentList {
    fn from(text: Text) -> Self {
        MessageContentList(vec![text.into()])
    }
}

impl<S: AsRef<str>> From<S> for MessageContent {
    fn from(s: S) -> Self {
        MessageContent::Text(Text {
            text: s.as_ref().to_string(),
            ..Default::default()
        })
    }
}

impl<S: AsRef<str>> From<S> for Message {
    fn from(s: S) -> Self {
        MessageBuilder::default()
            .role(MessageRole::User)
            .content(s.as_ref().to_string())
            .build()
            .expect("infallible")
    }
}

// Any single AsRef<str> can be converted to a MessageContent, in a list as a single item
impl<S: AsRef<str>> From<S> for MessageContentList {
    fn from(s: S) -> Self {
        MessageContentList(vec![s.as_ref().into()])
    }
}

impl From<MessageContent> for MessageContentList {
    fn from(content: MessageContent) -> Self {
        MessageContentList(vec![content])
    }
}

impl Serialize for ToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ToolChoice::Auto => {
                serde::Serialize::serialize(&serde_json::json!({"type": "auto"}), serializer)
            }
            ToolChoice::Any => {
                serde::Serialize::serialize(&serde_json::json!({"type": "any"}), serializer)
            }
            ToolChoice::Tool(name) => serde::Serialize::serialize(
                &serde_json::json!({"type": "tool", "name": name}),
                serializer,
            ),
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ContentBlockDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
    ThinkingDelta { thinking: String },
    SignatureDelta { signature: String },
}

#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct MessageDelta {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum MessagesStreamEvent {
    MessageStart {
        message: MessageStart,
        usage: Option<Usage>,
    },
    ContentBlockStart {
        index: usize,
        content_block: MessageContent,
    },
    ContentBlockDelta {
        index: usize,
        delta: ContentBlockDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessageDelta,
        #[serde(default)]
        usage: Option<Usage>,
    },
    MessageStop,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct MessageStart {
    pub id: String,
    pub model: String,
    pub role: String,
    pub content: Vec<MessageContent>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

pub type CreateMessagesResponseStream =
    Pin<Box<dyn Stream<Item = Result<MessagesStreamEvent, AnthropicError>> + Send>>;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ListModelsResponse {
    #[serde(default)]
    pub data: Vec<Model>,

    #[serde(default)]
    pub first_id: Option<String>,
    pub has_more: bool,
    #[serde(default)]
    pub last_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Model {
    pub created_at: String,
    pub display_name: String,
    pub id: String,
    #[serde(rename = "type")]
    pub model_type: String,
}

pub type GetModelResponse = Model;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test_log::test(tokio::test)]
    async fn test_deserialize_response() {
        let response = json!({
        "id":"msg_01KkaCASJuaAgTWD2wqdbwC8",
        "type":"message",
        "role":"assistant",
        "model":"claude-3-5-sonnet-20241022",
        "content":[
            {"type":"text",
        "text":"Hi! How can I help you today?"}],
        "stop_reason":"end_turn",
        "stop_sequence":null,
        "usage":{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":12}}).to_string();

        let response = serde_json::from_str::<CreateMessagesResponse>(&response).unwrap();

        let usage = response.usage.as_ref().unwrap();

        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(12));
        assert_eq!(usage.cache_creation_input_tokens, Some(0));
        assert_eq!(usage.cache_read_input_tokens, Some(0));
        assert_eq!(
            response.id,
            Some("msg_01KkaCASJuaAgTWD2wqdbwC8".to_string())
        );
        assert_eq!(
            response.model,
            Some("claude-3-5-sonnet-20241022".to_string())
        );
        assert_eq!(response.stop_reason, Some("end_turn".to_string()));
        assert_eq!(response.stop_sequence, None);
        assert_eq!(
            response
                .messages()
                .first()
                .unwrap()
                .content
                .first()
                .unwrap()
                .as_text(),
            Some(&Text {
                text: "Hi! How can I help you today?".to_string(),
                cache_control: None,
            })
        );
    }

    #[test_log::test(tokio::test)]
    async fn test_from_str() {
        let message: Message = "Hello world!".into();

        assert_eq!(
            message,
            Message {
                role: MessageRole::User,
                content: MessageContentList(vec![MessageContent::Text(Text {
                    text: "Hello world!".to_string(),
                    cache_control: None,
                })]),
            }
        );

        assert_eq!(message.text(), Some("Hello world!".to_string()));
    }
}
