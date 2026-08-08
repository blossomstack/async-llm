//! Native OpenAI Chat Completions protocol types.

use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    /// A JSON string, as required by the Chat Completions protocol.
    pub arguments: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    #[must_use]
    pub fn new(role: impl Into<String>, content: Option<String>) -> Self {
        Self {
            role: role.into(),
            content,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", Some(content.into()))
    }

    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", Some(content.into()))
    }

    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", Some(content.into()))
    }

    #[must_use]
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionDef,
}

impl ToolDef {
    #[must_use]
    pub fn function(function: FunctionDef) -> Self {
        Self {
            kind: "function".to_string(),
            function,
        }
    }
}

/// Selects whether Chat Completions may call tools and, if so, which function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    /// The model decides whether to call a tool.
    Auto,
    /// The model must call one or more tools.
    Required,
    /// The model must not call a tool.
    None,
    /// The model must call the named function.
    Function { name: String },
}

impl Serialize for ToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Required => serializer.serialize_str("required"),
            Self::None => serializer.serialize_str("none"),
            Self::Function { name } => NamedFunctionToolChoice::new(name).serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ToolChoice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match ToolChoiceRepr::deserialize(deserializer)? {
            ToolChoiceRepr::Selector(selector) => match selector.as_str() {
                "auto" => Ok(Self::Auto),
                "required" => Ok(Self::Required),
                "none" => Ok(Self::None),
                _ => Err(DeError::custom("unknown OpenAI tool choice selector")),
            },
            ToolChoiceRepr::Function { kind, function } if kind == "function" => {
                Ok(Self::Function {
                    name: function.name,
                })
            }
            ToolChoiceRepr::Function { .. } => Err(DeError::custom(
                "named OpenAI tool choice must be a function",
            )),
        }
    }
}

#[derive(Serialize)]
struct NamedFunctionToolChoice<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: NamedFunction<'a>,
}

impl<'a> NamedFunctionToolChoice<'a> {
    fn new(name: &'a str) -> Self {
        Self {
            kind: "function",
            function: NamedFunction { name },
        }
    }
}

#[derive(Serialize)]
struct NamedFunction<'a> {
    name: &'a str,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ToolChoiceRepr {
    Selector(String),
    Function {
        #[serde(rename = "type")]
        kind: String,
        function: NamedFunctionOwned,
    },
}

#[derive(Deserialize)]
struct NamedFunctionOwned {
    name: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DeltaFunction {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DeltaToolCall {
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<DeltaFunction>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<DeltaToolCall>>,
}

impl Delta {
    #[must_use]
    pub fn reasoning_trace(&self) -> Option<&str> {
        self.reasoning_content
            .as_deref()
            .or(self.reasoning.as_deref())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Choice {
    #[serde(default)]
    pub delta: Delta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

impl WireUsage {
    #[must_use]
    pub fn cached_tokens(&self) -> Option<u32> {
        self.prompt_tokens_details.as_ref()?.cached_tokens
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionChunk {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<WireUsage>,
}
