//! Canonical capture preserves delivered content without duplicating the
//! nonstream usage retained by the production response driver.
use super::*;
use crate::server::core_runtime::profiles::fixed_reasoning;
use serde_json::json;

fn capture(ingress: IngressProtocol, streaming: bool) -> CanonicalResponseTracker {
    let profile = Arc::new(CandidateProtocolProfile::exact_portable_path(
        ingress,
        IngressProtocol::ChatCompletions,
        "physical",
        fixed_reasoning("fixed"),
    ));
    let mut capture = CanonicalResponseTracker::new(
        profile,
        ToolIdProjection::new(ingress),
        None,
        streaming,
        "alias".into(),
    );
    capture.observe(CapturedNativeEvent::Head(200)).unwrap();
    capture
}

fn accepted_delivery(capture: &mut CanonicalResponseTracker) -> CanonicalFrameDelivery {
    let wire: Vec<_> = capture
        .pending
        .iter()
        .flat_map(|unit| unit.bytes.iter().copied())
        .collect();
    assert!(!wire.is_empty());
    capture.correlate_accepted_output(&wire, true).unwrap()
}

#[test]
fn converted_nonstream_capture_leaves_usage_to_provider_completion() {
    for ingress in [IngressProtocol::Responses, IngressProtocol::Messages] {
        let mut capture = capture(ingress, false);
        let body = json!({
            "id": "usage", "object": "chat.completion", "created": 1, "model": "physical",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 32, "completion_tokens": 45, "total_tokens": 77}
        });
        capture
            .observe(CapturedNativeEvent::Body(
                serde_json::to_vec(&body).unwrap(),
            ))
            .unwrap();
        capture.observe(CapturedNativeEvent::EndStream).unwrap();
        let delivery = accepted_delivery(&mut capture);
        assert!(
            !delivery.events.is_empty(),
            "delivered content must still be captured"
        );
        assert!(
            delivery.usage.is_none(),
            "nonstream usage has one producer even when capture is processed first"
        );
    }
}

#[test]
fn converted_stream_capture_keeps_delivered_usage() {
    for ingress in [IngressProtocol::Responses, IngressProtocol::Messages] {
        let mut capture = capture(ingress, true);
        let event = json!({
            "id": "usage", "object": "chat.completion.chunk", "created": 1, "model": "physical",
            "choices": [{"index": 0, "delta": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 32, "completion_tokens": 45, "total_tokens": 77}
        });
        capture
            .observe(CapturedNativeEvent::Body(
                format!("data: {event}\n\ndata: [DONE]\n\n").into_bytes(),
            ))
            .unwrap();
        capture.observe(CapturedNativeEvent::EndStream).unwrap();
        let delivery = accepted_delivery(&mut capture);
        assert!(!delivery.events.is_empty());
        let usage = delivery.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(32));
        assert_eq!(usage.output_tokens, Some(45));
    }
}
