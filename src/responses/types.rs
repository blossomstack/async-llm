//! Native OpenAI Responses API protocol types.

use serde::{Deserialize, Serialize};

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
    pub output: Vec<ResponseOutputItem>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
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
pub enum ResponsesStreamEvent {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        item_id: String,
        output_index: usize,
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        item_id: String,
        output_index: usize,
        #[serde(default)]
        call_id: Option<String>,
        delta: String,
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
    #[serde(rename = "response.failed")]
    Failed { response: CompletedResponse },
    #[serde(rename = "response.incomplete")]
    Incomplete { response: IncompleteResponse },
    #[serde(other)]
    Other,
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
