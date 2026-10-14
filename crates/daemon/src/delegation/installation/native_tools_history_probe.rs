//! Closed facts for the real-client tool-history gate. Never emit native text, arbitrary
//! IDs, file names or prompts: those may contain credentials or borrowed user content.
use super::{CALL_IDS, Value};
use std::io::Read;
use std::path::Path;

const TRANSCRIPT_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Debug, Default, Eq, PartialEq)]
struct Shape {
    messages: usize,
    string_content: usize,
    expected_calls: [usize; 5],
    expected_results: [usize; 5],
    expected_errors: [usize; 5],
    results_other_role: usize,
    other_calls: usize,
    other_results: usize,
    input_array: bool,
    messages_array: bool,
    response_results: [usize; 5],
    other_response_results: usize,
}

impl Shape {
    fn request(body: &Value) -> Self {
        let mut shape = Self::default();
        if let Some(messages) = body["messages"].as_array() {
            shape.messages_array = true;
            for message in messages {
                shape.message(message);
            }
        }
        if let Some(input) = body["input"].as_array() {
            shape.input_array = true;
            for item in input {
                if matches!(
                    item["type"].as_str(),
                    Some("function_call_output" | "custom_tool_call_output")
                ) {
                    match expected_index(&item["call_id"]) {
                        Some(index) => shape.response_results[index] += 1,
                        None => shape.other_response_results += 1,
                    }
                }
            }
        }
        shape
    }

    fn message(&mut self, message: &Value) {
        self.messages += 1;
        self.string_content += usize::from(message["content"].is_string());
        let Some(blocks) = message["content"].as_array() else {
            return;
        };
        for block in blocks {
            match block["type"].as_str() {
                Some("tool_use") => match expected_index(&block["id"]) {
                    Some(index) => self.expected_calls[index] += 1,
                    None => self.other_calls += 1,
                },
                Some("tool_result") => {
                    if message["role"] != "user" {
                        self.results_other_role += 1;
                    }
                    match expected_index(&block["tool_use_id"]) {
                        Some(index) => {
                            self.expected_results[index] += 1;
                            self.expected_errors[index] += usize::from(block["is_error"] == true);
                        }
                        None => self.other_results += 1,
                    }
                }
                _ => {}
            }
        }
    }
}

fn expected_index(value: &Value) -> Option<usize> {
    CALL_IDS.iter().position(|id| value == *id)
}

pub(super) fn observe_request(body: &Value) {
    eprintln!(
        "native tools continuation closed history shape: {:?}",
        Shape::request(body)
    );
}

pub(super) fn observe_transcript(path: &Path) {
    let evidence = transcript(path);
    match evidence {
        Some((shape, invalid_records)) => eprintln!(
            "native tools pre-load closed transcript shape: {shape:?} invalid_records={invalid_records}"
        ),
        None => {
            eprintln!("native tools pre-load closed transcript shape: unreadable-or-over-bound")
        }
    }
}

fn transcript(path: &Path) -> Option<(Shape, usize)> {
    #[cfg(windows)]
    let file = hiroute_platform::windows::security::open_identity(path, false).ok()?;
    #[cfg(not(windows))]
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    file.take(TRANSCRIPT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > TRANSCRIPT_LIMIT {
        return None;
    }
    let mut shape = Shape::default();
    let mut invalid = 0;
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(record) if record["message"].is_object() => shape.message(&record["message"]),
            Ok(_) => {}
            Err(_) => invalid += 1,
        }
    }
    Some((shape, invalid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn closed_history_shape_distinguishes_missing_failed_and_alternate_results() {
        let body = json!({"messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"native_shell"}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"native_shell","is_error":true}]},
            {"role":"assistant","content":[{"type":"tool_result","tool_use_id":"foreign"}]},
            {"role":"user","content":"private native output"}
        ],"input":[{"type":"custom_tool_call_output","call_id":"native_shell","output":"private native output"}]});
        let shape = Shape::request(&body);
        assert_eq!(shape.messages, 4);
        assert_eq!(shape.expected_calls, [0, 0, 0, 0, 1]);
        assert_eq!(shape.expected_results, [0, 0, 0, 0, 1]);
        assert_eq!(shape.expected_errors, [0, 0, 0, 0, 1]);
        assert_eq!(shape.results_other_role, 1);
        assert_eq!(shape.other_results, 1);
        assert_eq!(shape.string_content, 1);
        assert_eq!(shape.response_results, [0, 0, 0, 0, 1]);
        assert!(shape.input_array && shape.messages_array);
        assert_eq!(Shape::request(&json!({})), Shape::default());
    }

    #[test]
    fn closed_history_diagnostics_cannot_include_arbitrary_native_values() {
        let private = "credential-and-native-output-must-remain-private";
        let body = json!({"messages":[{"role":"user","content":[
            {"type":"tool_result","tool_use_id":private,"content":private}
        ]}],"input":[{"type":"function_call_output","call_id":private,"output":private}]});
        let shape = Shape::request(&body);
        assert_eq!(shape.other_results, 1);
        assert_eq!(shape.other_response_results, 1);
        let diagnostic = format!("{shape:?}");
        assert!(!diagnostic.contains(private));
        assert!(!diagnostic.contains("credential-and-native-output"));
    }
}
