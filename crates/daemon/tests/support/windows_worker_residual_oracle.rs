//! Closed diagnostics for the actual residual tool. No output, paths or locators
//! enter its receipt; native failure never becomes a successful model answer.
use serde_json::{Value, json};
use std::path::Path;

const PREFIX: &str = "HIRouteResidual/v1 ";
const OUTPUT_BOUND: usize = 1024 * 1024;

pub(super) fn script(binding: &Path, late: &Path, nonce: &str) -> String {
    let literal = |text: &str| format!("'{}'", text.replace('\'', "''"));
    format!(
        "$ErrorActionPreference='Stop'; $hirouteResidualStage='encoding'; try {{ \
         $encoding=[Text.UTF8Encoding]::new($false); $hirouteResidualStage='binding-json'; \
         $binding=@{{pid=$PID;temp=$env:TEMP;receipt={}}}|ConvertTo-Json -Compress; \
         $hirouteResidualStage='binding-write'; [IO.File]::WriteAllText({},$binding,$encoding); \
         $hirouteResidualStage='waiting'; Start-Sleep -Seconds 60; \
         $hirouteResidualStage='late-write'; [IO.File]::WriteAllText({},{},$encoding) \
         }} catch {{ $native=$_.Exception; for($i=0;$i-lt 8 -and $null-ne $native.InnerException;$i++) \
         {{$native=$native.InnerException}}; $kind='unknown'; \
         if($native-is [UnauthorizedAccessException]){{$kind='access'}} \
         elseif($native-is [IO.IOException]){{$kind='io'}} \
         elseif($native-is [ArgumentException]){{$kind='argument'}}; \
         [Console]::WriteLine('{PREFIX}stage='+$hirouteResidualStage+' kind='+$kind+' hresult='+([int]$native.HResult)); exit 1 }}",
        literal(&format!("started-{nonce}")),
        literal(binding.to_str().unwrap()),
        literal(late.to_str().unwrap()),
        literal(&format!("late-{nonce}")),
    )
}

pub(super) fn returned(body: &Value) -> Option<(String, bool)> {
    let item = body["input"].as_array()?.iter().find(|item| {
        matches!(
            item["type"].as_str(),
            Some("function_call_output" | "custom_tool_call_output")
        ) && item["call_id"] == "worker_residual"
    })?;
    Some((
        super::native_text(&item["output"]),
        item["is_error"] == true,
    ))
}

pub(super) fn facts(output: &str, is_error: bool) -> Value {
    let mut candidates = Vec::new();
    let mut caption_count = 0;
    let within_bound = output.len() <= OUTPUT_BOUND;
    if within_bound {
        for line in output.lines() {
            let Some(line) = line.strip_prefix(PREFIX) else {
                continue;
            };
            caption_count += 1;
            if let Some(caption) = caption(line) {
                candidates.push(caption);
            }
        }
    }
    let valid = caption_count == 1 && candidates.len() == 1;
    let diagnostic = valid.then(|| candidates[0]);
    json!({"schema":"hiroute.windows-residual-tool-facts/v1", "tool_returned":true,
        "tool_error_flag":is_error,"output_bytes":output.len().min(OUTPUT_BOUND),
        "output_bound_exceeded":!within_bound,"closed_caption_seen":caption_count>0,
        "closed_caption_valid":valid,"closed_caption_ambiguous":caption_count>1,
        "script_stage":diagnostic.map(|item|item.0),"exception_kind":diagnostic.map(|item|item.1),
        "hresult":diagnostic.map(|item|item.2)})
}

fn caption(line: &str) -> Option<(&'static str, &'static str, i32)> {
    let fields: Vec<_> = line.split(' ').collect();
    if fields.len() != 3 {
        return None;
    }
    let stage = match fields[0].strip_prefix("stage=")? {
        "encoding" => "encoding",
        "binding-json" => "binding-json",
        "binding-write" => "binding-write",
        "waiting" => "waiting",
        "late-write" => "late-write",
        _ => return None,
    };
    let kind = match fields[1].strip_prefix("kind=")? {
        "access" => "access",
        "io" => "io",
        "argument" => "argument",
        "unknown" => "unknown",
        _ => return None,
    };
    let number = fields[2].strip_prefix("hresult=")?;
    let hresult = number.parse::<i32>().ok()?;
    if hresult.to_string() != number {
        return None;
    }
    Some((stage, kind, hresult))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn residual_failure_caption_is_closed_and_keeps_unknown_vendor_output_unknown() {
        let private = "private-native-output-path-account";
        let receipt = facts(
            &format!("{private}\n{PREFIX}stage=binding-write kind=access hresult=-2147024891\n"),
            true,
        );
        assert_eq!(receipt["script_stage"], "binding-write");
        assert_eq!(receipt["exception_kind"], "access");
        assert_eq!(receipt["hresult"], -2147024891i32);
        assert!(!receipt.to_string().contains(private));
        for suffix in [
            "stage=private kind=access hresult=5",
            "stage=waiting kind=account hresult=5",
            "stage=waiting kind=io hresult=+5",
            "stage=waiting kind=io hresult=05",
            "stage=waiting kind=io hresult=4294967295",
            "stage=waiting kind=io hresult=5 PRIVATE",
        ] {
            let rejected = facts(&format!("{PREFIX}{suffix}"), false);
            assert_eq!(rejected["closed_caption_valid"], false);
            assert!(rejected["script_stage"].is_null());
            assert!(rejected["hresult"].is_null());
        }
        let unknown = facts(private, false);
        assert_eq!(unknown["closed_caption_seen"], false);
        assert!(unknown["hresult"].is_null());
    }

    #[test]
    fn residual_tool_error_and_multiple_captions_cannot_disappear_as_no_result() {
        let body = json!({"input":[{"type":"function_call_output","call_id":"worker_residual",
            "is_error":true,"output":"native-failure"}]});
        // The exact source21 predicate filtered this real error return out, so
        // the residual branch incorrectly treated it as no correlated result.
        assert!(super::super::correlated(&body, "worker_residual").is_none());
        assert_eq!(returned(&body), Some(("native-failure".into(), true)));
        let ambiguous = facts(
            &format!(
                "{PREFIX}stage=encoding kind=io hresult=5\n{PREFIX}stage=binding-write kind=io hresult=5"
            ),
            true,
        );
        assert_eq!(ambiguous["closed_caption_ambiguous"], true);
        assert!(ambiguous["script_stage"].is_null());
        let mixed = facts(
            &format!("{PREFIX}stage=encoding kind=io hresult=5\n{PREFIX}unknown-caption"),
            false,
        );
        assert_eq!(mixed["closed_caption_valid"], false);
        assert!(mixed["hresult"].is_null());
    }

    #[test]
    fn residual_script_keeps_literal_paths_and_original_sleep_before_late_write() {
        let script = script(
            Path::new("C:/private % ! ' &/binding.json"),
            Path::new("C:/private % ! ' &/late.txt"),
            "fixture-nonce",
        );
        assert!(script.contains("'C:/private % ! '' &/binding.json'"));
        assert!(script.contains("pid=$PID;temp=$env:TEMP;receipt='started-fixture-nonce'"));
        assert!(
            script.find("Start-Sleep -Seconds 60").unwrap()
                < script.find("$hirouteResidualStage='late-write'").unwrap()
        );
        assert!(!script.contains("WriteLine($native"));
    }
}
