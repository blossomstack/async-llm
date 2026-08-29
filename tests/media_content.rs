//! The wire shape of image and document blocks, for both protocols.
//!
//! These assert against the JSON each API documents, because that is the only
//! thing that matters here — a block that round-trips through serde but names
//! its fields differently from the API is still wrong.

use async_llm::types::{
    Base64Source, Document, Image, MediaSource, MessageContent, Text, ToolResult, ToolResultBlock,
    ToolResultContent,
};
use serde_json::json;

/// One-pixel PNG, base64. Short enough to read in a failure message.
const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUg==";

// --- Anthropic ------------------------------------------------------------

#[test]
fn anthropic_image_block_matches_the_documented_shape() {
    let block = MessageContent::Image(Image::base64("image/png", PNG_B64));

    assert_eq!(
        serde_json::to_value(&block).unwrap(),
        json!({
            "type": "image",
            "source": { "type": "base64", "media_type": "image/png", "data": PNG_B64 },
        })
    );
}

#[test]
fn anthropic_document_block_matches_the_documented_shape() {
    let block = MessageContent::Document(Document::base64("application/pdf", PNG_B64));

    assert_eq!(
        serde_json::to_value(&block).unwrap(),
        json!({
            "type": "document",
            "source": { "type": "base64", "media_type": "application/pdf", "data": PNG_B64 },
        })
    );
}

#[test]
fn anthropic_media_source_can_also_be_a_url() {
    let block = MessageContent::Image(Image {
        source: MediaSource::Url(async_llm::types::UrlSource {
            url: "https://example.com/a.png".to_string(),
        }),
        cache_control: None,
    });

    assert_eq!(
        serde_json::to_value(&block).unwrap(),
        json!({
            "type": "image",
            "source": { "type": "url", "url": "https://example.com/a.png" },
        })
    );
}

#[test]
fn anthropic_blocks_round_trip() {
    for block in [
        MessageContent::Image(Image::base64("image/png", PNG_B64)),
        MessageContent::Document(Document::base64("application/pdf", PNG_B64)),
    ] {
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(
            serde_json::from_value::<MessageContent>(json).unwrap(),
            block
        );
    }
}

/// A plain-text tool result must still be a bare string on the wire — this is
/// what every existing caller sends, and an untagged enum is what keeps it so.
#[test]
fn anthropic_text_tool_result_is_still_a_bare_string() {
    let result = ToolResult {
        tool_use_id: "call_1".to_string(),
        content: Some(ToolResultContent::from("done")),
        is_error: false,
        cache_control: None,
    };

    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({ "tool_use_id": "call_1", "content": "done", "is_error": false })
    );
}

/// The reason `content` became a list: an image can now ride inside the tool
/// result itself, rather than needing a separate message.
#[test]
fn anthropic_tool_result_can_carry_an_image() {
    let result = ToolResult {
        tool_use_id: "call_1".to_string(),
        content: Some(ToolResultContent::Blocks(vec![
            ToolResultBlock::Text(Text::from("here is the screenshot")),
            ToolResultBlock::Image(Image::base64("image/png", PNG_B64)),
        ])),
        is_error: false,
        cache_control: None,
    };

    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({
            "tool_use_id": "call_1",
            "content": [
                { "type": "text", "text": "here is the screenshot" },
                {
                    "type": "image",
                    "source": { "type": "base64", "media_type": "image/png", "data": PNG_B64 },
                },
            ],
            "is_error": false,
        })
    );
}

#[test]
fn anthropic_tool_result_content_round_trips_both_ways() {
    for content in [
        ToolResultContent::Text("plain".to_string()),
        ToolResultContent::Blocks(vec![ToolResultBlock::Image(Image::base64(
            "image/png",
            PNG_B64,
        ))]),
    ] {
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(
            serde_json::from_value::<ToolResultContent>(json).unwrap(),
            content
        );
    }
}

#[test]
fn anthropic_base64_source_carries_no_data_url_prefix() {
    // The API wants raw base64 in `data`, unlike OpenAI's data URL.
    let source = Base64Source {
        media_type: "image/png".to_string(),
        data: PNG_B64.to_string(),
    };
    let json = serde_json::to_value(&source).unwrap();
    assert!(!json["data"].as_str().unwrap().starts_with("data:"));
}
