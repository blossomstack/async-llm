//! Native OpenAI Responses API protocol types.

use serde::{Deserialize, Deserializer, Serialize};

/// A function tool accepted by the OpenAI Responses API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionTool {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

impl FunctionTool {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            kind: "function".into(),
            name: name.into(),
            description: description.into(),
            parameters,
            strict: None,
        }
    }
}

/// Controls model reasoning in a Responses request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasoningControl {
    pub effort: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl ReasoningControl {
    #[must_use]
    pub fn new(effort: impl Into<String>) -> Self {
        Self {
            effort: effort.into(),
            summary: None,
        }
    }
}

/// A request sent to the OpenAI Responses API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<FunctionTool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    pub stream: bool,
    pub store: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

impl ResponsesRequest {
    #[must_use]
    pub fn new(model: impl Into<String>, input: Vec<serde_json::Value>) -> Self {
        Self {
            model: model.into(),
            input,
            instructions: None,
            tools: Vec::new(),
            tool_choice: None,
            parallel_tool_calls: None,
            stream: true,
            store: false,
            reasoning: None,
            include: vec!["reasoning.encrypted_content".into()],
            prompt_cache_key: None,
            max_output_tokens: None,
        }
    }

    #[must_use]
    pub fn for_text(model: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(
            model,
            vec![serde_json::json!({
                "role": "user",
                "content": [{"type": "input_text", "text": text.into()}],
            })],
        )
    }

    #[must_use]
    pub fn with_function_tool(mut self, tool: FunctionTool) -> Self {
        self.tools.push(tool);
        self
    }

    #[must_use]
    pub fn with_reasoning(mut self, reasoning: ReasoningControl) -> Self {
        self.reasoning = Some(reasoning);
        self
    }
}

/// A content item in a completed Responses output item.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ResponseContentItem {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub encrypted_content: Option<String>,
    #[serde(default)]
    pub refusal: Option<String>,
}

/// An output item in a completed Responses response.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ResponseOutputItem {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Vec<ResponseContentItem>,
    #[serde(default)]
    pub encrypted_content: Option<String>,
    #[serde(default)]
    pub summary: Vec<ResponseContentItem>,
    #[serde(default)]
    pub call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

/// Usage reported by a completed Responses response.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ResponsesUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

/// Completion data carried by `response.completed` events.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct CompletedResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub error: Option<ResponseErrorDetails>,
    #[serde(default)]
    pub output: Vec<ResponseOutputItem>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
}

/// Error details reported by a failed Response or an `error` SSE event.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ResponseErrorDetails {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub param: Option<String>,
}

/// Completion data carried by `response.incomplete` events.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct IncompleteResponse {
    #[serde(default)]
    pub incomplete_details: Option<IncompleteDetails>,
    #[serde(default)]
    pub output: Vec<ResponseOutputItem>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct IncompleteDetails {
    pub reason: String,
}

/// A server-sent event emitted by the OpenAI Responses API.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type")]
enum KnownResponsesStreamEvent {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        item_id: String,
        output_index: usize,
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        item_id: String,
        output_index: usize,
        content_index: usize,
        text: String,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        item_id: String,
        output_index: usize,
        #[serde(default)]
        call_id: Option<String>,
        delta: String,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone {
        item_id: String,
        output_index: usize,
        #[serde(default)]
        call_id: Option<String>,
        arguments: String,
    },
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta {
        item_id: String,
        output_index: usize,
        summary_index: usize,
        delta: String,
    },
    #[serde(rename = "response.reasoning.encrypted_content")]
    ReasoningEncryptedContent {
        item_id: String,
        output_index: usize,
        content_index: usize,
        encrypted_content: String,
    },
    #[serde(rename = "response.reasoning_text.delta")]
    ReasoningTextDelta {
        item_id: String,
        output_index: usize,
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        output_index: usize,
        item: ResponseOutputItem,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        output_index: usize,
        item: ResponseOutputItem,
    },
    #[serde(rename = "response.completed")]
    Completed { response: CompletedResponse },
    #[serde(rename = "error")]
    Error {
        code: String,
        message: String,
        #[serde(default)]
        param: Option<String>,
    },
    #[serde(rename = "response.failed")]
    Failed { response: CompletedResponse },
    #[serde(rename = "response.incomplete")]
    Incomplete { response: IncompleteResponse },
}

/// A server-sent event emitted by the OpenAI Responses API.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponsesStreamEvent {
    OutputTextDelta {
        item_id: String,
        output_index: usize,
        content_index: usize,
        delta: String,
    },
    OutputTextDone {
        item_id: String,
        output_index: usize,
        content_index: usize,
        text: String,
    },
    FunctionCallArgumentsDelta {
        item_id: String,
        output_index: usize,
        call_id: Option<String>,
        delta: String,
    },
    FunctionCallArgumentsDone {
        item_id: String,
        output_index: usize,
        call_id: Option<String>,
        arguments: String,
    },
    ReasoningSummaryTextDelta {
        item_id: String,
        output_index: usize,
        summary_index: usize,
        delta: String,
    },
    ReasoningEncryptedContent {
        item_id: String,
        output_index: usize,
        content_index: usize,
        encrypted_content: String,
    },
    ReasoningTextDelta {
        item_id: String,
        output_index: usize,
        content_index: usize,
        delta: String,
    },
    OutputItemAdded {
        output_index: usize,
        item: ResponseOutputItem,
    },
    OutputItemDone {
        output_index: usize,
        item: ResponseOutputItem,
    },
    Completed {
        response: CompletedResponse,
    },
    Error {
        code: String,
        message: String,
        param: Option<String>,
    },
    Failed {
        response: CompletedResponse,
    },
    Incomplete {
        response: IncompleteResponse,
    },
    Other {
        event_type: String,
        data: serde_json::Value,
    },
}

impl From<KnownResponsesStreamEvent> for ResponsesStreamEvent {
    fn from(event: KnownResponsesStreamEvent) -> Self {
        match event {
            KnownResponsesStreamEvent::OutputTextDelta {
                item_id,
                output_index,
                content_index,
                delta,
            } => Self::OutputTextDelta {
                item_id,
                output_index,
                content_index,
                delta,
            },
            KnownResponsesStreamEvent::OutputTextDone {
                item_id,
                output_index,
                content_index,
                text,
            } => Self::OutputTextDone {
                item_id,
                output_index,
                content_index,
                text,
            },
            KnownResponsesStreamEvent::FunctionCallArgumentsDelta {
                item_id,
                output_index,
                call_id,
                delta,
            } => Self::FunctionCallArgumentsDelta {
                item_id,
                output_index,
                call_id,
                delta,
            },
            KnownResponsesStreamEvent::FunctionCallArgumentsDone {
                item_id,
                output_index,
                call_id,
                arguments,
            } => Self::FunctionCallArgumentsDone {
                item_id,
                output_index,
                call_id,
                arguments,
            },
            KnownResponsesStreamEvent::ReasoningSummaryTextDelta {
                item_id,
                output_index,
                summary_index,
                delta,
            } => Self::ReasoningSummaryTextDelta {
                item_id,
                output_index,
                summary_index,
                delta,
            },
            KnownResponsesStreamEvent::ReasoningEncryptedContent {
                item_id,
                output_index,
                content_index,
                encrypted_content,
            } => Self::ReasoningEncryptedContent {
                item_id,
                output_index,
                content_index,
                encrypted_content,
            },
            KnownResponsesStreamEvent::ReasoningTextDelta {
                item_id,
                output_index,
                content_index,
                delta,
            } => Self::ReasoningTextDelta {
                item_id,
                output_index,
                content_index,
                delta,
            },
            KnownResponsesStreamEvent::OutputItemAdded { output_index, item } => {
                Self::OutputItemAdded { output_index, item }
            }
            KnownResponsesStreamEvent::OutputItemDone { output_index, item } => {
                Self::OutputItemDone { output_index, item }
            }
            KnownResponsesStreamEvent::Completed { response } => Self::Completed { response },
            KnownResponsesStreamEvent::Error {
                code,
                message,
                param,
            } => Self::Error {
                code,
                message,
                param,
            },
            KnownResponsesStreamEvent::Failed { response } => Self::Failed { response },
            KnownResponsesStreamEvent::Incomplete { response } => Self::Incomplete { response },
        }
    }
}

impl<'de> Deserialize<'de> for ResponsesStreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = serde_json::Value::deserialize(deserializer)?;
        let event_type = data
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| serde::de::Error::missing_field("type"))?
            .to_owned();
        Ok(
            serde_json::from_value::<KnownResponsesStreamEvent>(data.clone())
                .map(Self::from)
                .unwrap_or(Self::Other { event_type, data }),
        )
    }
}

impl ResponsesStreamEvent {
    #[must_use]
    pub fn is_max_output_tokens(&self) -> bool {
        matches!(
            self,
            Self::Incomplete {
                response: IncompleteResponse {
                    incomplete_details: Some(IncompleteDetails { reason, .. }),
                    ..
                },
            } if reason == "max_output_tokens"
        )
    }
}
