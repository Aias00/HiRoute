//! Qoder 1.1.65 ordinary user models from the officially prepared private account.
//! Real public Source/Plan/settings commands, ordinary saved routes and exact
//! observations are distinct from provider-account inference and Worker overrides.
#[path = "windows_qoder_user_models_native.rs"]
mod native;
#[path = "windows_qoder_user_models_oracle.rs"]
mod oracle;
use super::windows_user_model_receipts as receipts;
#[path = "windows_qoder_user_models_settings.rs"]
mod settings;
pub use native::Contexts;

use super::InstalledProduct;
use super::windows_business_http as http;
use super::windows_headless_business::{Business, assert_operation, cli};
use super::windows_routing_business::{Plan, grant};
use hiroute_domain::UpstreamProtocol;
use serde_json::{Value, json};
use settings::Snapshot;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::PathBuf;

pub struct UserModels {
    context: String,
    namespace: String,
    connection: String,
    file: PathBuf,
    skill: PathBuf,
    original: Option<Snapshot>,
    routes: Vec<Route>,
    configured: Value,
    retained_file: Option<Snapshot>,
    retained_skill: Option<Snapshot>,
}
struct Route {
    protocol: UpstreamProtocol,
    plan: Plan,
    source: Business,
    oracle: oracle::Oracle,
    nonce: String,
    retained: BTreeSet<String>,
}
impl UserModels {
    pub fn run(product: &mut InstalledProduct, contexts: &Contexts, gateway: SocketAddr) -> Self {
        contexts.verify();
        let file = contexts.config.join("settings.json");
        let skill = contexts
            .config
            .join("skills/hiroute-collaboration/SKILL.md");
        assert!(
            !skill.exists(),
            "the fresh official account must have no preexisting HiRoute Skill"
        );
        let original = Snapshot::external_optional(&file);
        let scan = cli(product, &["agents", "scan"], None);
        let scanned = scan["data"]["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["agent_id"] == "agent_qoder_default")
            .expect("the fixed actual Qoder image must be publicly discovered");
        let context = scanned["context_id"].as_str().unwrap().to_owned();
        assert!(
            status(product, &context)["state"] == "not_configured",
            "fresh Qoder context must be unconfigured"
        );
        contexts.check_collaboration(product);
        let expected_cli = cli(product, &["schema", "list"], None);
        assert!(
            expected_cli["status"] == "succeeded"
                && expected_cli["data"]["descriptor_digest"]
                    .as_str()
                    .is_some_and(|digest| digest.starts_with("sha256:")),
            "public schema must supply the actual trusted CLI baseline"
        );
        let mut routes = Vec::new();
        for protocol in [UpstreamProtocol::Responses, UpstreamProtocol::Messages] {
            let label = label(protocol);
            let mut nonce = [0u8; 16];
            getrandom::fill(&mut nonce).unwrap();
            let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
            let oracle = oracle::Oracle::start(
                protocol,
                skill.clone(),
                product.stable.clone(),
                expected_cli.clone(),
                nonce.clone(),
            );
            let source = Business::save_on_with_protocol(
                product,
                &format!("candidate/native/windows-qoder-user-{label}"),
                &format!("windows-qoder-user-{label}-source"),
                oracle.server.address,
                131072,
                protocol,
            );
            let plan = Plan::create(
                product,
                source.binding_id(),
                &format!("windows-qoder-user-{label}-plan"),
            );
            assert!(
                oracle.ledger.lock().unwrap().probe == 1,
                "actual declared Native protocol must pass its source probe"
            );
            routes.push(Route {
                protocol,
                plan,
                source,
                oracle,
                nonce,
                retained: BTreeSet::new(),
            });
        }
        product.preserve_on_failure = true;
        let preview = apply(
            product,
            "connect",
            &model_spec(&context, &routes, false),
            "windows-qoder-user-configure",
        );
        let namespace = preview["model_effect"]["provider_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let connection = format!("agent-connection/{context}");
        let configured = status(product, &context);
        assert_configured(&configured, &routes);
        let mut models = Self {
            context,
            namespace,
            connection,
            file,
            skill,
            original,
            routes,
            configured,
            retained_file: None,
            retained_skill: None,
        };
        let old = grant(product, &models.connection);
        models.assert_provider(&old);
        models.invoke(contexts, "configured");
        apply(
            product,
            "connect",
            &model_spec(&models.context, &models.routes, true),
            "windows-qoder-user-rotate",
        );
        let current = grant(product, &models.connection);
        assert!(
            *current != *old,
            "public rotation must issue fresh private model authority"
        );
        models.assert_rejected(gateway, &old);
        models.assert_provider(&current);
        models.invoke(contexts, "rotated");
        models.collaboration(product, contexts);
        models.assert_provider(&current);
        models.configured = status(product, &models.context);
        assert_configured(&models.configured, &models.routes);
        for route in &mut models.routes {
            route.retained = route.receipts(product);
        }
        models.retained_file = Some(Snapshot::read(&models.file));
        models.retained_skill = Some(Snapshot::read(&models.skill));
        contexts.verify();
        models
    }
    /// The caller performs an actual daemon stop/start between these two methods.
    pub fn recovered(
        &mut self,
        product: &InstalledProduct,
        contexts: &Contexts,
        gateway: SocketAddr,
    ) {
        contexts.verify();
        let current = status(product, &self.context);
        assert!(
            current["current_selection"] == self.configured["current_selection"]
                && current["restore_point_ref"] == self.configured["restore_point_ref"],
            "actual restart must retain the same saved model selection and restore identity"
        );
        assert!(
            current["collaboration"] == self.configured["collaboration"],
            "actual restart must retain independent installed Skill state"
        );
        assert!(
            Snapshot::read(&self.file).same(self.retained_file.as_ref().unwrap()),
            "actual restart must retain exact model bytes, grant and native permissions"
        );
        assert!(
            Snapshot::read(&self.skill).same(self.retained_skill.as_ref().unwrap()),
            "actual restart must retain the installed Skill bytes and native permissions"
        );
        for route in &self.routes {
            assert!(
                route.receipts(product) == route.retained,
                "actual restart must retain same-request committed observations"
            );
            let source = cli(
                product,
                &["compute", "show", route.source.source_id()],
                None,
            );
            assert!(
                source["data"]["sources"]
                    .as_array()
                    .is_some_and(|sources| sources.len() == 1
                        && sources[0]["source_id"] == route.source.source_id()
                        && sources[0]["models"][0]["binding_id"] == route.source.binding_id()),
                "actual restart must retain the exact saved Native source and binding"
            );
            let plan = cli(product, &["routing", "show", &route.plan.plan_id], None);
            assert!(
                plan["data"]["head"]["model_alias"] == route.plan.alias
                    && plan["data"]["head"]["status"] == "enabled",
                "actual restart must retain the exact enabled public Plan alias"
            );
        }
        let current_token = grant(product, &self.connection);
        self.assert_provider(&current_token);
        self.invoke(contexts, "restarted");
        let retained: Vec<_> = self
            .routes
            .iter()
            .map(|route| route.receipts(product))
            .collect();
        for (route, actual) in self.routes.iter().zip(&retained) {
            assert!(route.retained.is_subset(actual));
        }
        let skill = Snapshot::read(&self.skill);
        let spec = json!({"schema_version":{"major":2,"minor":0},"context_id":self.context,
            "model":{"intent":"restore","restore_point_ref":current["restore_point_ref"]},"collaboration":{"intent":"keep"},"access_token":{"intent":"keep"}});
        apply(
            product,
            "restore",
            &spec,
            "windows-qoder-user-model-restore",
        );
        let restored = status(product, &self.context);
        assert!(
            restored["state"] == "not_configured"
                && restored["current_selection"].is_null()
                && restored["restore_point_ref"].is_null(),
            "public model Restore must release only its facet"
        );
        self.assert_original();
        assert!(
            Snapshot::read(&self.skill).same(&skill)
                && restored["collaboration"]["state"] == "configured",
            "model Restore must preserve the installed Skill and its independent native permissions"
        );
        self.assert_rejected(gateway, &current_token);
        self.restore_skill(product, "windows-qoder-user-final-skill-restore");
        assert!(
            !self.skill.exists(),
            "public independent Skill Restore must release its fresh owned artifact"
        );
        self.assert_original();
        self.assert_rejected(gateway, &current_token);
        for (route, retained) in self.routes.iter().zip(&retained) {
            assert!(
                route.receipts(product) == *retained,
                "Restore/revocation must retain exact request observations without new inference"
            );
            let ledger = route.oracle.ledger.lock().unwrap();
            let mut stages =
                BTreeSet::from(["configured".into(), "rotated".into(), "restarted".into()]);
            if route.protocol == UpstreamProtocol::Responses {
                stages.insert("collaboration".into());
            }
            assert!(
                ledger.probe == 1
                    && ledger.completed == stages
                    && ledger.requests.len()
                        == if route.protocol == UpstreamProtocol::Responses {
                            6
                        } else {
                            3
                        },
                "all saved ordinary native calls must complete exact independent protocol/usage witnesses"
            );
            assert!(
                route.protocol != UpstreamProtocol::Responses || ledger.cli_proven,
                "ordinary saved model must execute the real public CLI through its installed Skill"
            );
        }
        contexts.verify();
    }
    fn providers(&self) -> Vec<String> {
        self.routes
            .iter()
            .map(|route| {
                hiroute_domain::additional_model_plan_provider_id(
                    &self.namespace,
                    &route.plan.alias,
                )
            })
            .collect()
    }
    fn assert_provider(&self, token: &str) {
        let snapshot = Snapshot::read(&self.file);
        let value = snapshot.value();
        let original = self.original.as_ref().map(Snapshot::value);
        assert!(
            settings::without_owned(&value, &self.providers(), original.as_ref())
                == original.unwrap_or_else(|| json!({})),
            "model Configure/rotation/native calls must preserve all original user defaults and unrelated settings"
        );
        for route in &self.routes {
            let provider = &value["providers"][hiroute_domain::additional_model_plan_provider_id(
                &self.namespace,
                &route.plan.alias,
            )];
            assert!(
                provider["type"] == "openai-compatible"
                    && provider["authType"] == "bearer"
                    && provider["protocol"]
                        == if route.protocol == UpstreamProtocol::Responses {
                            "openai-responses"
                        } else {
                            "anthropic"
                        },
                "saved native provider must preserve the explicit Plan protocol"
            );
            assert!(
                provider["apiKey"].as_str() == Some(token),
                "private native provider must contain only the current grant"
            );
            let base = provider["baseUrl"].as_str().unwrap();
            assert!(
                base.starts_with("http://127.0.0.1:")
                    && base.ends_with(hiroute_domain::QODER_MODEL_BASE_PATH),
                "ordinary saved route must use the published Qoder Gateway base path"
            );
            assert!(
                provider["models"]
                    .as_array()
                    .is_some_and(|models| models.len() == 1
                        && models[0]["model"] == route.plan.alias
                        && models[0]["capabilities"]["tools"] == true
                        && models[0]["contextWindow"] == 131072
                        && models[0]["maxOutputTokens"]
                            .as_u64()
                            .is_some_and(|bound| bound > 0)),
                "saved native alias/tools/budgets must match the actual published Plan"
            );
        }
    }
    fn invoke(&self, contexts: &Contexts, stage: &str) {
        for route in &self.routes {
            contexts.invoke(
                &format!(
                    "{}/{}",
                    hiroute_domain::additional_model_plan_provider_id(
                        &self.namespace,
                        &route.plan.alias
                    ),
                    route.plan.alias
                ),
                &format!(
                    "WINDOWS-QODER-USER-MODEL-{}-{stage}: Reply with the independent receipt.",
                    route.nonce
                ),
                &format!("qoder-user-model-{stage}-{}", route.nonce),
            );
        }
    }
    fn assert_rejected(&self, gateway: SocketAddr, token: &str) {
        for route in &self.routes {
            let before = route.oracle.ledger.lock().unwrap().requests.len();
            let messages = route.protocol == UpstreamProtocol::Messages;
            let rejected = http::post(
                gateway,
                &format!(
                    "{}/{}",
                    hiroute_domain::QODER_MODEL_BASE_PATH,
                    if messages { "messages" } else { "responses" }
                ),
                Some(token),
                &if messages {
                    json!({"model":route.plan.alias,"messages":[{"role":"user","content":"removed authority must not reach source"}],"max_tokens":16,"stream":false})
                } else {
                    json!({"model":route.plan.alias,"input":"removed authority must not reach source","max_output_tokens":16,"stream":false})
                },
            );
            assert!(
                rejected.status == 401 && rejected.json()["code"] == "GATEWAY_GRANT_UNAUTHORIZED",
                "rotated/restored model authority must be rejected before inference"
            );
            assert!(
                route.oracle.ledger.lock().unwrap().requests.len() == before,
                "rejected model authority must not reach its upstream"
            );
        }
    }
    fn collaboration(&self, product: &InstalledProduct, contexts: &Contexts) {
        let before = Snapshot::read(&self.file);
        let spec = collaboration_spec(&self.context);
        // Model Configure/rotation changes the native settings dependency. The
        // previous collaboration proof must be renewed against those saved bytes.
        contexts.check_collaboration(product);
        apply(
            product,
            "connect",
            &spec,
            "windows-qoder-user-skill-configure",
        );
        contexts.check_collaboration(product);
        let route = self
            .routes
            .iter()
            .find(|route| route.protocol == UpstreamProtocol::Responses)
            .unwrap();
        contexts.invoke(&format!("{}/{}",hiroute_domain::additional_model_plan_provider_id(&self.namespace,&route.plan.alias),route.plan.alias),
            &format!("WINDOWS-QODER-USER-MODEL-{}-collaboration: Use hiroute-collaboration and its trusted public CLI to inspect the released schema.",route.nonce), &format!("qoder-user-model-collaboration-{}",route.nonce));
        self.restore_skill(product, "windows-qoder-user-skill-restore-model-kept");
        assert!(
            !self.skill.exists() && Snapshot::read(&self.file).same(&before),
            "independent Skill Configure/Restore must preserve exact model bytes/private grant and permissions"
        );
        assert_configured(&status(product, &self.context), &self.routes);
        // Reinstall the independently proven facet, then retain it over the real
        // restart and restore models first to exercise the opposite facet order.
        contexts.check_collaboration(product);
        apply(
            product,
            "connect",
            &spec,
            "windows-qoder-user-skill-reconfigure",
        );
        assert!(
            Snapshot::read(&self.file).same(&before),
            "Skill reconfiguration must not alter saved models"
        );
    }
    fn restore_skill(&self, product: &InstalledProduct, key: &str) {
        let current = status(product, &self.context);
        let spec = json!({"schema_version":{"major":2,"minor":0},"context_id":self.context,
            "model":{"intent":"keep"},"collaboration":{"intent":"restore","restore_point_ref":current["collaboration"]["restore_point_ref"]},"access_token":{"intent":"keep"}});
        apply(product, "restore", &spec, key);
    }
    fn assert_original(&self) {
        let current = Snapshot::external_optional(&self.file);
        assert!(
            match (&self.original, &current) {
                (Some(original), Some(current)) => original.same(current),
                (None, None) => true,
                _ => false,
            },
            "untouched native model Restore must return exact original bytes/permissions or original absence"
        );
    }
}
impl Route {
    fn receipts(&self, product: &InstalledProduct) -> BTreeSet<String> {
        let expected = self.oracle.ledger.lock().unwrap().requests.clone();
        receipts::committed(product, &self.plan.plan_id, &expected)
    }
}
fn label(protocol: UpstreamProtocol) -> &'static str {
    if protocol == UpstreamProtocol::Responses {
        "responses"
    } else {
        "messages"
    }
}
fn selection(routes: &[Route]) -> Value {
    let mut protocols = serde_json::Map::new();
    for route in routes {
        protocols.insert(route.plan.plan_id.clone(), json!(label(route.protocol)));
    }
    json!({"mode":"qoder_additional","allowed_plan_ids":routes.iter().map(|route|route.plan.plan_id.as_str()).collect::<BTreeSet<_>>(),"plan_protocols":protocols})
}
fn model_spec(context: &str, routes: &[Route], rotate: bool) -> Value {
    json!({"schema_version":{"major":2,"minor":0},"context_id":context,"model":{"intent":"configure","settings":selection(routes)},"collaboration":{"intent":"keep"},"access_token":{"intent":if rotate {"regenerate"} else {"keep"}}})
}
fn collaboration_spec(context: &str) -> Value {
    json!({"schema_version":{"major":2,"minor":0},"context_id":context,"model":{"intent":"keep"},"collaboration":{"intent":"configure","settings":{"trigger_mode":"explicit"}},"access_token":{"intent":"keep"}})
}
fn status(product: &InstalledProduct, context: &str) -> Value {
    cli(product, &["agents", "connect", "status", context], None)["data"].clone()
}
fn assert_configured(value: &Value, routes: &[Route]) {
    assert!(
        value["state"] == "configured"
            && value["restore_point_ref"].is_string()
            && value["current_selection"] == selection(routes),
        "public status must retain the exact additional model Plan/protocol choice"
    );
}
fn apply(product: &InstalledProduct, verb: &str, spec: &Value, key: &str) -> Value {
    let preview = cli(
        product,
        &["agents", verb, "preview"],
        Some(&json!({"spec":spec})),
    )["data"]
        .clone();
    assert!(
        preview["applicable"] == true && preview["resident_service"]["login_item_required"] != true,
        "public preview must admit this actual user context without inventing a manager"
    );
    let request = json!({"spec":preview["spec"],"accept_digest":preview["accept_digest"],"dependency_digest":preview["dependency_digest"],"expected_revisions":preview["expected_revisions"],"idempotency_key":key});
    let applied = cli(product, &["agents", verb, "apply"], Some(&request));
    let operation = applied["operation"]["operation_id"]
        .as_str()
        .or_else(|| applied["data"]["operation_id"].as_str())
        .unwrap();
    assert_operation(product, operation);
    let replay = cli(product, &["agents", verb, "apply"], Some(&request));
    assert!(
        replay["operation"]["operation_id"]
            .as_str()
            .or_else(|| replay["data"]["operation_id"].as_str())
            == Some(operation),
        "same public apply intent must retain exact operation identity"
    );
    preview
}
