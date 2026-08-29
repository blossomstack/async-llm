//! The wire shape of image and document blocks, for both protocols.
//!
//! These assert against the JSON each API documents, because that is the only
//! thing that matters here — a block that round-trips through serde but names
//! its fields differently from the API is still wrong.

use async_llm::{
    openai::{ChatContent, ChatContentPart, ChatMessage, ImageUrl},
    types::{
        Base64Source, Document, Image, MediaSource, MessageContent, Text, ToolResult,
        ToolResultBlock, ToolResultContent,
    },
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

// --- OpenAI Chat Completions ----------------------------------------------

/// Text content stays a bare string. A message that suddenly serialized as a
/// one-element array would still be accepted by the API but would change every
/// request this library has ever sent.
#[test]
fn openai_text_message_is_still_a_bare_string() {
    let message = ChatMessage::user("hello");

    assert_eq!(
        serde_json::to_value(&message).unwrap(),
        json!({ "role": "user", "content": "hello" })
    );
}

#[test]
fn openai_image_part_is_a_data_url() {
    let message = ChatMessage::parts(
        "user",
        vec![
            ChatContentPart::text("what is this?"),
            ChatContentPart::image_base64("image/png", PNG_B64),
        ],
    );

    assert_eq!(
        serde_json::to_value(&message).unwrap(),
        json!({
            "role": "user",
            "content": [
                { "type": "text", "text": "what is this?" },
                {
                    "type": "image_url",
                    "image_url": { "url": format!("data:image/png;base64,{PNG_B64}") },
                },
            ],
        })
    );
}

#[test]
fn openai_file_part_carries_a_filename() {
    // The API uses `filename` to decide how to parse inline file data, so it
    // is not optional in practice for a base64 document.
    let message = ChatMessage::parts(
        "user",
        vec![ChatContentPart::file_base64(
            "report.pdf",
            "application/pdf",
            PNG_B64,
        )],
    );

    let json = serde_json::to_value(&message).unwrap();
    assert_eq!(json["content"][0]["type"], "file");
    assert_eq!(json["content"][0]["file"]["filename"], "report.pdf");
    assert_eq!(
        json["content"][0]["file"]["file_data"],
        format!("data:application/pdf;base64,{PNG_B64}")
    );
}

#[test]
fn openai_content_round_trips_both_shapes() {
    for content in [
        ChatContent::Text("plain".to_string()),
        ChatContent::Parts(vec![ChatContentPart::image_base64("image/png", PNG_B64)]),
    ] {
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(
            serde_json::from_value::<ChatContent>(json).unwrap(),
            content
        );
    }
}

#[test]
fn openai_content_text_reads_through_both_shapes() {
    assert_eq!(
        ChatContent::Text("plain".to_string()).text(),
        Some("plain".to_string())
    );
    assert_eq!(
        ChatContent::Parts(vec![
            ChatContentPart::text("one"),
            ChatContentPart::image_base64("image/png", PNG_B64),
            ChatContentPart::text("two"),
        ])
        .text(),
        Some("one\ntwo".to_string()),
        "part texts join; an image contributes nothing"
    );
    assert_eq!(
        ChatContent::Parts(vec![ChatContentPart::image_base64("image/png", PNG_B64)]).text(),
        None,
        "an image-only message has no text at all, which is not the same as empty text"
    );
}

#[test]
fn openai_image_detail_is_omitted_unless_set() {
    let with_detail = ChatContentPart::ImageUrl {
        image_url: ImageUrl {
            url: "https://example.com/a.png".to_string(),
            detail: Some("high".to_string()),
        },
    };
    assert_eq!(
        serde_json::to_value(&with_detail).unwrap()["image_url"]["detail"],
        "high"
    );

    let without = ChatContentPart::image_base64("image/png", PNG_B64);
    let json = serde_json::to_value(&without).unwrap();
    assert!(
        json["image_url"].get("detail").is_none(),
        "an unset detail must not be sent as null"
    );
}
