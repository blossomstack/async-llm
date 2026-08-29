//! The wire shape of OpenAI Chat Completions content parts.
//!
//! Its own file because `openai` is a non-default feature: these tests living
//! beside the Anthropic ones broke CI's default-features job, which compiles
//! neither `openai` nor `responses`.
#![cfg(feature = "openai")]

use async_llm::openai::{ChatContent, ChatContentPart, ChatMessage, ImageUrl};
use serde_json::json;

/// One-pixel PNG, base64. Short enough to read in a failure message.
const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUg==";

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
