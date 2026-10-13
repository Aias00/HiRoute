use crate::request_input::{MAX_REQUEST_BYTES, RequestInput, valid_positional};
use hiroute_application_api::{
    DelegationCancelRequestV1, DelegationContinueRequestV1, DelegationGetRequestV1,
    DelegationListRequestV1, DelegationResultRequestV1, DelegationStartRequestV1,
    DelegationWaitRequestV1, ErrorCode,
};

pub(crate) fn payload(command: &str, options: &[String]) -> Result<serde_json::Value, ErrorCode> {
    let source_start = if options.last().is_some_and(|flag| flag == "--request-stdin") {
        options.len() - 1
    } else if options.len() >= 2 && options[options.len() - 2] == "--request-file" {
        options.len() - 2
    } else {
        return Err(ErrorCode::InvalidArguments);
    };
    let input = RequestInput::parse(&options[source_start..])?;
    let selectors = &options[..source_start];
    validate_selectors(command, selectors, None)?;
    let value = parse_value(command, input.read(MAX_REQUEST_BYTES)?)?;
    validate_selectors(command, selectors, Some(&value))?;
    Ok(value)
}

fn validate_selectors(
    command: &str,
    options: &[String],
    value: Option<&serde_json::Value>,
) -> Result<(), ErrorCode> {
    let matches =
        |key: &str, expected: &str| value.is_none_or(|value| value[key].as_str() == Some(expected));
    match (command, options) {
        ("tasks.list" | "tasks.start" | "tasks.continue", []) => Ok(()),
        ("tasks.wait" | "tasks.result" | "tasks.cancel", [flag, run])
            if flag == "--run" && valid_positional(run) && matches("run_id", run) =>
        {
            Ok(())
        }
        ("tasks.show", [flag, task])
            if flag == "--task"
                && valid_positional(task)
                && matches("task_id", task)
                && value.is_none_or(|value| value.get("run_id").is_none()) =>
        {
            Ok(())
        }
        ("tasks.show", [task_flag, task, run_flag, run])
            if task_flag == "--task"
                && run_flag == "--run"
                && valid_positional(task)
                && valid_positional(run)
                && matches("task_id", task)
                && matches("run_id", run) =>
        {
            Ok(())
        }
        ("tasks.show", [key_flag, key, operation_flag, operation])
            if key_flag == "--submission-key"
                && operation_flag == "--operation"
                && !key.is_empty()
                && matches!(operation.as_str(), "start" | "continue")
                && matches("submission_key", key)
                && matches("submission_operation", operation) =>
        {
            Ok(())
        }
        _ => Err(ErrorCode::InvalidArguments),
    }
}

#[cfg(test)]
fn parse(command: &str, bytes: &[u8]) -> Result<serde_json::Value, ErrorCode> {
    parse_value(
        command,
        crate::request_input::read_json(bytes, MAX_REQUEST_BYTES)?,
    )
}

fn parse_value(command: &str, value: serde_json::Value) -> Result<serde_json::Value, ErrorCode> {
    let invalid = |_| ErrorCode::InvalidArguments;
    match command {
        "tasks.list" => typed::<DelegationListRequestV1>(value, invalid),
        "tasks.show" => typed::<DelegationGetRequestV1>(value, invalid),
        "tasks.start" => {
            let request: DelegationStartRequestV1 = crate::request_input::deserialize(value)?;
            if !request.valid() {
                return Err(ErrorCode::InvalidArguments);
            }
            serde_json::to_value(request).map_err(invalid)
        }
        "tasks.wait" => typed::<DelegationWaitRequestV1>(value, invalid),
        "tasks.result" => {
            let request: DelegationResultRequestV1 = crate::request_input::deserialize(value)?;
            if !request.valid() {
                return Err(ErrorCode::InvalidArguments);
            }
            serde_json::to_value(request).map_err(invalid)
        }
        "tasks.cancel" => typed::<DelegationCancelRequestV1>(value, invalid),
        "tasks.continue" => typed::<DelegationContinueRequestV1>(value, invalid),
        _ => Err(ErrorCode::UnknownCommand),
    }
}

fn typed<T>(
    value: serde_json::Value,
    invalid: fn(serde_json::Error) -> ErrorCode,
) -> Result<serde_json::Value, ErrorCode>
where
    T: serde::de::DeserializeOwned + serde::Serialize + ValidRequest,
{
    let request: T = crate::request_input::deserialize(value)?;
    if !request.valid() {
        return Err(ErrorCode::InvalidArguments);
    }
    serde_json::to_value(request).map_err(invalid)
}

trait ValidRequest {
    fn valid(&self) -> bool;
}

macro_rules! valid_request {
    ($($type:ty),+ $(,)?) => {$(
        impl ValidRequest for $type {
            fn valid(&self) -> bool { <$type>::valid(self) }
        }
    )+};
}

valid_request!(
    DelegationListRequestV1,
    DelegationGetRequestV1,
    DelegationWaitRequestV1,
    DelegationCancelRequestV1,
    DelegationContinueRequestV1,
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn caller() -> serde_json::Value {
        json!({
            "workspace_id": "personal/default",
            "context_id": "owner",
            "grant_id": "collaboration-grant/one",
            "grant_generation": 1
        })
    }

    #[test]
    fn show_submission_selector_must_match_the_typed_request() {
        let bytes = serde_json::to_vec(&json!({
            "caller": caller(),
            "submission_key": "continue/one",
            "submission_operation": "continue"
        }))
        .unwrap();
        let value = parse("tasks.show", &bytes).unwrap();
        assert!(
            validate_selectors(
                "tasks.show",
                &[
                    "--submission-key".into(),
                    "continue/one".into(),
                    "--operation".into(),
                    "continue".into(),
                ],
                Some(&value),
            )
            .is_ok()
        );
        assert_eq!(
            validate_selectors(
                "tasks.show",
                &[
                    "--submission-key".into(),
                    "different".into(),
                    "--operation".into(),
                    "continue".into(),
                ],
                Some(&value),
            ),
            Err(ErrorCode::InvalidArguments)
        );
    }

    #[test]
    fn wait_run_selector_must_match_the_typed_request() {
        let bytes = serde_json::to_vec(&json!({
            "caller": caller(),
            "run_id": "run/one",
            "after_revision": 2,
            "wait_ms": 1
        }))
        .unwrap();
        let value = parse("tasks.wait", &bytes).unwrap();
        assert!(
            validate_selectors(
                "tasks.wait",
                &["--run".into(), "run/one".into()],
                Some(&value),
            )
            .is_ok()
        );
        assert_eq!(
            validate_selectors(
                "tasks.wait",
                &["--run".into(), "run/two".into()],
                Some(&value),
            ),
            Err(ErrorCode::InvalidArguments)
        );
    }
}
