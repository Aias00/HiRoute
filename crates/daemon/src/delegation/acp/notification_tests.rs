//! Session startup/replay notifications are separate from current prompt output.
use super::tests::{Journal, input};
use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn notify(
    writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    session: &str,
    text: &str,
) {
    let message = json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId":session,"update":{"sessionUpdate":"agent_message_chunk",
            "content":{"type":"text","text":text}}
    }});
    writer
        .write_all(format!("{message}\n").as_bytes())
        .await
        .unwrap();
}

async fn scenario(load: bool, wrong_prompt_session: bool) -> AcpRunOutcome {
    let (client, server) = tokio::io::duplex(16_384);
    let server = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(server);
        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            let request: Value = serde_json::from_str(&line).unwrap();
            let result = match request["method"].as_str().unwrap() {
                "initialize" => {
                    json!({"protocolVersion":1,"agentCapabilities":{"loadSession":true}})
                }
                "session/new" | "session/load" => {
                    // Real Claude sends updates before new resolves; native load replays old
                    // history before its response. Wait for that independent callback to run.
                    notify(&mut writer, "acp-a", "old-history").await;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    if request["method"] == "session/new" {
                        json!({"sessionId":"acp-a","_meta":{"agentSessionId":"native-a"}})
                    } else {
                        json!({"_meta":{"agentSessionId":"native-a"}})
                    }
                }
                "session/prompt" => {
                    if wrong_prompt_session {
                        notify(&mut writer, "unrelated-session", "unrelated-current-output").await;
                    }
                    notify(&mut writer, "acp-a", "current-output").await;
                    json!({"stopReason":"end_turn"})
                }
                other => panic!("unexpected fixture method: {other}"),
            };
            writer
                .write_all(
                    format!(
                        "{}\n",
                        json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
    });
    let mut request = input();
    if load {
        request.session = AcpSessionStart::Load(AcpSessionBinding {
            acp_session_id: "acp-a".into(),
            native_session_id: Some("native-a".into()),
        });
    }
    let (read, write) = tokio::io::split(client);
    let outcome = run_acp(read, write, request, Arc::new(Journal::default()))
        .await
        .unwrap();
    server.await.unwrap();
    outcome
}

#[tokio::test]
async fn startup_and_loaded_history_notifications_do_not_make_current_output_incomplete() {
    for load in [false, true] {
        let outcome = scenario(load, false).await;
        assert_eq!(outcome.text, "current-output");
        assert!(!outcome.content_incomplete);
    }
}

#[tokio::test]
async fn wrong_session_notification_during_prompt_still_marks_output_incomplete() {
    let outcome = scenario(false, true).await;
    assert_eq!(outcome.text, "current-output");
    assert!(outcome.content_incomplete);
}
