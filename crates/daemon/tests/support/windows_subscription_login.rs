//! Account-free acceptance through shipped public CLI and pinned real CPA.
//! No callback/code, browser, account, token endpoint or inference is fabricated.
//! The caller owns bounded native execution and a complete installed CPA package.
use hiroute_application_api::{
    COMPUTE_CONNECTION_OPTIONS_SCHEMA_V1, COMPUTE_SCAN_RESULT_SCHEMA_V1,
    ComputeConnectionOptionsResultV1, ComputeScanResultV1, ComputeSubscriptionLoginResultV1,
    ComputeSubscriptionLoginSessionV1, SUBSCRIPTION_LOGIN_RESULT_SCHEMA_V1,
    SubscriptionLoginProviderV1 as Provider, SubscriptionLoginStatusV1 as Status,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub struct Pending {
    sessions: Vec<(Provider, String)>,
    saved_sources: Vec<String>,
    prior_logins: Vec<ComputeSubscriptionLoginSessionV1>,
}

type PublicCli<'a> = dyn FnMut(&[&str], Option<&Value>) -> Value + 'a;

fn data<T: DeserializeOwned>(envelope: Value) -> T {
    // Never put the envelope, URL, state, code or credential into assertion text.
    assert!(
        envelope["status"] == "succeeded",
        "public OAuth operation did not succeed"
    );
    let Ok(result) = serde_json::from_value(envelope["data"].clone()) else {
        panic!("public OAuth result did not match its published schema");
    };
    result
}

fn login(cli: &mut PublicCli<'_>, request: Value) -> ComputeSubscriptionLoginResultV1 {
    let result: ComputeSubscriptionLoginResultV1 = data(cli(
        &[
            "compute",
            "connection",
            "login",
            "--request-stdin",
            "--output",
            "json",
        ],
        Some(&request),
    ));
    assert!(
        result.schema == SUBSCRIPTION_LOGIN_RESULT_SCHEMA_V1,
        "public login schema"
    );
    result
}

fn session(cli: &mut PublicCli<'_>, request: Value) -> ComputeSubscriptionLoginSessionV1 {
    let result = login(cli, request);
    assert!(
        result.sessions.len() == 1,
        "public login must return the addressed session"
    );
    result.sessions.into_iter().next().unwrap()
}

fn assert_state(
    session: &ComputeSubscriptionLoginSessionV1,
    provider: Provider,
    id: &str,
    expected: Status,
) {
    assert!(
        session.provider == provider && session.login_ref == id && session.status == expected,
        "public login addressed identity/state mismatch"
    );
    assert!(
        session.account_ref.is_none() && session.candidate.is_none(),
        "an account-free login must not grant a source/account candidate"
    );
    if expected == Status::Pending {
        let Some(candidate) = &session.callback_input_candidate else {
            panic!("pending callback reference absent")
        };
        assert!(
            candidate.candidate_revision == 1
                && candidate.candidate_ref == format!("candidate/subscription-login/{id}"),
            "pending callback reference differs"
        );
    } else {
        assert!(
            session.callback_input_candidate.is_none(),
            "terminal login retained a callback capability"
        );
    }
}

fn saved_sources(cli: &mut PublicCli<'_>) -> Vec<String> {
    let data: Value = data(cli(&["compute", "list", "--output", "json"], None));
    let Some(sources) = data["sources"].as_array() else {
        panic!("public compute list schema")
    };
    let mut ids = sources
        .iter()
        .map(|source| {
            let Some(id) = source["source_id"].as_str() else {
                panic!("public saved source identity")
            };
            id.to_owned()
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn assert_discovery(cli: &mut PublicCli<'_>) {
    let scan: ComputeScanResultV1 = data(cli(&["compute", "scan", "--output", "json"], None));
    assert!(
        scan.schema == COMPUTE_SCAN_RESULT_SCHEMA_V1 && !scan.items.is_empty(),
        "public scan returned no inventory"
    );
    let options: ComputeConnectionOptionsResultV1 = data(cli(
        &["compute", "connection", "options", "--output", "json"],
        None,
    ));
    assert!(
        options.schema == COMPUTE_CONNECTION_OPTIONS_SCHEMA_V1,
        "public options schema"
    );
    for id in ["connector.cpa.codex", "connector.cpa.claude"] {
        assert!(
            options
                .options
                .iter()
                .any(|option| option.connector_id == id),
            "complete package must expose both managed subscription providers"
        );
    }
}

pub fn run_before_restart(cli: &mut PublicCli<'_>) -> Pending {
    assert_discovery(cli);
    let original_sources = saved_sources(cli);
    let mut pending = Vec::new();
    let mut prior_logins = Vec::new();
    for provider in [Provider::Codex, Provider::Claude] {
        let initial = login(cli, json!({"action":"list", "provider":provider}));
        assert!(
            initial.sessions.iter().all(|session| {
                !matches!(session.status, Status::Pending | Status::Authorized)
                    && session.authorization_url.is_none()
                    && session.callback_input_candidate.is_none()
                    && session.account_ref.is_none()
                    && session.candidate.is_none()
            }),
            "OAuth gate refuses existing active or account-bearing login sessions"
        );
        // Ordinary-user uninstall preserves the store. Keep terminal receipts
        // from previous runs and prove they remain unchanged, instead of
        // clearing user state or requiring an unused profile on every replay.
        prior_logins.extend(initial.sessions);
        let first = session(cli, json!({"action":"start", "provider":provider}));
        assert_state(&first, provider, &first.login_ref, Status::Pending);
        let first_challenge = assert_url(first.authorization_url.as_deref(), provider);
        let observed = session(cli, json!({"action":"status", "login_ref":first.login_ref}));
        assert_state(&observed, provider, &first.login_ref, Status::Pending);
        assert!(
            observed.authorization_url.is_none(),
            "status must not replay the authorization URL"
        );
        let cancelled = session(cli, json!({"action":"cancel", "login_ref":first.login_ref}));
        assert_state(&cancelled, provider, &first.login_ref, Status::Cancelled);
        let forgotten = session(cli, json!({"action":"forget", "login_ref":first.login_ref}));
        assert_state(&forgotten, provider, &first.login_ref, Status::Forgotten);
        let repeated = session(cli, json!({"action":"forget", "login_ref":first.login_ref}));
        assert_state(&repeated, provider, &first.login_ref, Status::Forgotten);
        let next = session(cli, json!({"action":"start", "provider":provider}));
        assert_state(&next, provider, &next.login_ref, Status::Pending);
        assert!(
            next.login_ref != first.login_ref,
            "restart login reused its opaque identity"
        );
        let next_challenge = assert_url(next.authorization_url.as_deref(), provider);
        assert!(
            next_challenge[0] != first_challenge[0] && next_challenge[1] != first_challenge[1],
            "new login reused its PKCE challenge or state"
        );
        pending.push((provider, next.login_ref));
    }
    assert!(
        saved_sources(cli) == original_sources,
        "pending/cancel/forget changed saved routing sources"
    );
    Pending {
        sessions: pending,
        saved_sources: original_sources,
        prior_logins,
    }
}

pub fn assert_after_restart(cli: &mut PublicCli<'_>, pending: Pending) {
    // The production restore contract expires interrupted Pending sessions; it
    // does not resume OAuth or create another refresh writer without admission.
    for (provider, id) in pending.sessions {
        let restored = session(cli, json!({"action":"status", "login_ref":id}));
        assert_state(&restored, provider, &id, Status::Expired);
        assert!(
            restored.authorization_url.is_none(),
            "restart persisted/replayed authorization URL"
        );
        let listed = login(cli, json!({"action":"list", "provider":provider}));
        assert!(
            listed
                .sessions
                .iter()
                .any(|session| session.login_ref == id && session.status == Status::Expired),
            "public list lost the interrupted login receipt"
        );
        assert!(
            listed
                .sessions
                .iter()
                .all(|session| session.status != Status::Pending),
            "restart spawned a new login"
        );
        let forgotten = session(cli, json!({"action":"forget", "login_ref":id}));
        assert_state(&forgotten, provider, &id, Status::Forgotten);
    }
    for original in pending.prior_logins {
        let retained = session(
            cli,
            json!({"action":"status", "login_ref":original.login_ref}),
        );
        assert!(
            retained == original,
            "OAuth lifecycle changed a prior terminal login receipt"
        );
    }
    assert_discovery(cli);
    assert!(
        saved_sources(cli) == pending.saved_sources,
        "OAuth recovery changed saved routing sources"
    );
}

fn assert_url(raw: Option<&str>, provider: Provider) -> [[u8; 32]; 2] {
    let Some(raw) = raw else {
        panic!("real CPA returned no authorization URL")
    };
    let Ok(uri) = raw.parse::<http::Uri>() else {
        panic!("real CPA authorization URL is malformed")
    };
    let host = match provider {
        Provider::Codex => "auth.openai.com",
        Provider::Claude => "claude.ai",
    };
    assert!(
        uri.scheme_str() == Some("https")
            && uri
                .authority()
                .is_some_and(|authority| authority.as_str() == host)
            && uri.path() == "/oauth/authorize",
        "real CPA did not select its pinned official OAuth origin"
    );
    let mut query = BTreeMap::new();
    for pair in uri.query().unwrap_or_default().split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            panic!("OAuth query is malformed")
        };
        let key = decode(key);
        assert!(
            query.insert(key, decode(value)).is_none(),
            "OAuth query repeats a field"
        );
    }
    assert!(
        query
            .get("response_type")
            .is_some_and(|value| value == "code")
            && query
                .get("code_challenge_method")
                .is_some_and(|value| value == "S256"),
        "OAuth code/PKCE contract differs"
    );
    let Some(state) = query.get("state") else {
        panic!("OAuth state absent")
    };
    let Some(challenge) = query.get("code_challenge") else {
        panic!("OAuth PKCE challenge absent")
    };
    assert!(
        (16..=4096).contains(&state.len())
            && !state.bytes().any(|byte| byte.is_ascii_control())
            && challenge.len() == 43
            && challenge
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte)),
        "OAuth state/challenge are invalid"
    );
    let callback = match provider {
        Provider::Codex => "http://localhost:1455/auth/callback",
        Provider::Claude => "http://localhost:54545/callback",
    };
    assert!(
        query
            .get("redirect_uri")
            .is_some_and(|value| value == callback),
        "OAuth callback differs from pinned CPA"
    );
    [
        Sha256::digest(state.as_bytes()).into(),
        Sha256::digest(challenge.as_bytes()).into(),
    ]
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                assert!(index + 2 < bytes.len(), "OAuth query escape is truncated");
                let digit = |byte: u8| {
                    char::from(byte)
                        .to_digit(16)
                        .unwrap_or_else(|| panic!("OAuth query escape invalid"))
                };
                output.push((digit(bytes[index + 1]) * 16 + digit(bytes[index + 2])) as u8);
                index += 3;
            }
            byte => {
                output.push(if byte == b'+' { b' ' } else { byte });
                index += 1;
            }
        }
    }
    String::from_utf8(output).unwrap_or_else(|_| panic!("OAuth query is not UTF-8"))
}
