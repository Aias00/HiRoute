//! Actual native filesystem and durable admission regressions, without launching an Agent.
//! Pinned-client public New/Continue acceptance remains a separate required business gate.
use super::*;
use crate::delegation::authorization_tests::fixture::sample;
use crate::delegation::profile::{native_context_path, native_history, windows_workspace};
use hiroute_application::delegation::{
    admission::DelegationAdmission, safety::RunSafetyProjection,
};
use hiroute_application::publication::admission::SharedAdmissionGate;
use hiroute_domain::delegation::*;
use hiroute_domain::{CompiledAgentPlanV1, OperationId};
use hiroute_local_storage::LocalStorageSet;
use hiroute_platform::windows::security;
use hiroute_platform_test_support::junction;
use std::sync::Mutex;

/// Forward every operation to the real coordinated store. No in-memory run/native state and
/// no process-spawn checkpoints are fabricated to make a continuation admissible.
struct StoredRuntime(Mutex<LocalStorageSet>);

impl StoredRuntime {
    fn with<T>(&self, action: impl FnOnce(&dyn DelegationRuntimePort) -> T) -> T {
        action(self.0.lock().unwrap().runtime())
    }
}

impl DelegationRuntimePort for StoredRuntime {
    fn task(
        &self,
        workspace: &WorkspaceId,
        task: &str,
    ) -> Result<Option<DelegationTaskV1>, DelegationErrorV1> {
        self.with(|runtime| runtime.task(workspace, task))
    }

    fn run(
        &self,
        workspace: &WorkspaceId,
        run: &str,
    ) -> Result<Option<DelegationRunV1>, DelegationErrorV1> {
        self.with(|runtime| runtime.run(workspace, run))
    }

    fn find_submission(
        &self,
        workspace: &WorkspaceId,
        continuation: bool,
        key: &str,
    ) -> Result<Option<DelegationRunV1>, DelegationErrorV1> {
        self.with(|runtime| runtime.find_submission(workspace, continuation, key))
    }

    fn accept(
        &self,
        acceptance: &DelegationAcceptanceV1,
    ) -> Result<DelegationRunV1, DelegationErrorV1> {
        self.with(|runtime| runtime.accept(acceptance))
    }

    fn checkpoint(
        &self,
        workspace: &WorkspaceId,
        run: &str,
        revision: u64,
        event_id: &str,
        event: &DelegationCheckpointV1,
    ) -> Result<DelegationRunV1, DelegationErrorV1> {
        self.with(|runtime| runtime.checkpoint(workspace, run, revision, event_id, event))
    }

    fn request_cancel(
        &self,
        workspace: &WorkspaceId,
        run: &str,
        operation: &OperationId,
        reason: &str,
    ) -> Result<DelegationCancelReceiptV1, DelegationErrorV1> {
        self.with(|runtime| runtime.request_cancel(workspace, run, operation, reason))
    }

    fn unreconciled(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<Vec<DelegationRunV1>, DelegationErrorV1> {
        self.with(|runtime| runtime.unreconciled(workspace))
    }

    fn set_resume_materials(
        &self,
        workspace: &WorkspaceId,
        task: &str,
        run: &str,
        until_ms: u64,
        bodies: &[DelegationBodyRefV1],
        history: &[String],
    ) -> Result<(), DelegationErrorV1> {
        self.with(|runtime| {
            runtime.set_resume_materials(workspace, task, run, until_ms, bodies, history)
        })
    }

    fn native_root(
        &self,
        workspace: &WorkspaceId,
        task: &str,
    ) -> Result<Option<DelegationNativeRootSnapshotV1>, DelegationErrorV1> {
        self.with(|runtime| runtime.native_root(workspace, task))
    }

    fn commit_native_root_ready(
        &self,
        ready: &DelegationNativeRootReadyV1,
    ) -> Result<DelegationNativeRootSnapshotV1, DelegationErrorV1> {
        self.with(|runtime| runtime.commit_native_root_ready(ready))
    }

    fn mark_native_root_unknown(
        &self,
        workspace: &WorkspaceId,
        task: &str,
        generation: u64,
        nonce: &str,
    ) -> Result<(), DelegationErrorV1> {
        self.with(|runtime| runtime.mark_native_root_unknown(workspace, task, generation, nonce))
    }
}

struct SelectedArtifact(WorkerInstallationConfig);

impl WorkerInstallationSelectionSource for SelectedArtifact {
    fn selection(
        &self,
        harness: WorkerHarnessV1,
    ) -> Result<Option<WorkerInstallationSelection>, DelegationErrorV1> {
        Ok(
            (harness == self.0.harness).then(|| WorkerInstallationSelection {
                config: self.0.clone(),
                revision: 1,
            }),
        )
    }
}

struct Fixture {
    runtime: Arc<StoredRuntime>,
    storage: PathBuf,
    workspace: PathBuf,
    directory: crate::test_support::PrivateTempDir,
}

impl Fixture {
    fn new() -> Self {
        let directory = crate::test_support::private_tempdir();
        let storage = directory.path().join("store");
        let runtime = Arc::new(StoredRuntime(Mutex::new(
            LocalStorageSet::open_for_daemon_startup(&storage).unwrap(),
        )));
        let workspace = directory.path().join("workspace % ! 空 格");
        drop(security::create_owner_only_directory(&workspace).unwrap());
        Self {
            runtime,
            storage,
            workspace,
            directory,
        }
    }

    fn admitted_profile_input(&self) -> (ManagedWorkerProfileSource, WorkerProfileInput) {
        let value: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../../../e2e/product/fixtures/routing/current-publication.v3.json"
        ))
        .unwrap();
        let compiled: CompiledAgentPlanV1 =
            serde_json::from_value::<CompiledAgentPlanV1>(value["plans"][0].clone())
                .unwrap()
                .into_current()
                .unwrap();
        let (workspace, canonical) = windows_workspace(self.workspace.to_str().unwrap()).unwrap();
        let mut input = sample("profile-task", "profile-new", &workspace.root_identity);
        input.task.workspace = workspace;
        input.task.plan.harness = WorkerHarnessV1::QoderCli;
        input.task.plan.model_alias = compiled.model_alias().as_str().to_owned();
        input.task.plan.plan_id = compiled.body.identity.agent_plan_id.clone();
        input.task.plan.plan_digest = compiled.digest.clone();
        input.run.configuration.canonical_workspace_path = canonical;
        let gate = Arc::new(SharedAdmissionGate::new());
        let safety = RunSafetyProjection::new(gate.clone(), "epoch".into()).unwrap();
        safety.finish_startup_recovery();
        let run = DelegationAdmission {
            gate,
            safety: &safety,
            runtime: self.runtime.as_ref(),
        }
        .accept(&input, |_, _| Ok(()))
        .unwrap();
        let task = self
            .runtime
            .task(&run.workspace_id, &run.task_id)
            .unwrap()
            .unwrap();
        // A real PE is used only as an installation metadata artifact; it is never executed
        // or presented as Qoder acceptance. Qoder's renderer needs no external account/probe.
        let executable = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let selections = Arc::new(SelectedArtifact(WorkerInstallationConfig::qoder_native(
            executable,
        )));
        let source = ManagedWorkerProfileSource {
            storage_root: self.storage.clone(),
            selections,
            runtime: self.runtime.clone(),
        };
        (
            source,
            WorkerProfileInput {
                protocol: hiroute_domain::AgentIngressProtocolV1::Responses,
                task: task.clone(),
                permit: run.profile_permit().unwrap(),
                run,
                compiled_plan: compiled,
                workspace_path: self.workspace.clone(),
                gateway: "127.0.0.1:12345".parse().unwrap(),
                model_token: hiroute_domain::ProtectedSecret::new(
                    b"profile-materialization-only-token".to_vec(),
                )
                .unwrap(),
            },
        )
    }

    fn build(&self) -> (CandidateWorkerProfile, DelegationTaskV1) {
        let (source, input) = self.admitted_profile_input();
        let task = input.task.clone();
        let profile = source.build(input).unwrap();
        (profile, task)
    }

    fn retained(
        &self,
        profile: &CandidateWorkerProfile,
        task: &DelegationTaskV1,
    ) -> (TaskSessionRoot, DelegationNativeRootV1, Vec<PathBuf>) {
        let root = self
            .runtime
            .native_root(&task.workspace_id, &task.task_id)
            .unwrap()
            .unwrap()
            .root;
        assert_eq!(root.state, DelegationNativeRootStateV1::Ready);
        let history = native_history(
            &profile.session_root,
            WorkerHarnessV1::QoderCli,
            "71a2b020-312e-49cd-b084-924cbb3642ad",
        )
        .unwrap()
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
        let base = Path::new(root.managed_base_path.as_deref().unwrap());
        let session = TaskSessionRoot::prepare(
            base,
            &task.workspace_id,
            &task.workspace.root_identity,
            &task.task_id,
            WorkerHarnessV1::QoderCli,
            SessionRootUse::Continue {
                required_history: &history,
            },
        )
        .unwrap();
        (session, root, history)
    }
}

#[test]
fn managed_new_profile_accepts_the_same_workspace_without_verbatim_spelling() {
    if crate::test_support::isolated_agent_home(concat!(
        module_path!(),
        "::managed_new_profile_accepts_the_same_workspace_without_verbatim_spelling"
    )) {
        return;
    }
    let fixture = Fixture::new();
    let (profile, task) = fixture.build();
    let canonical = fs::canonicalize(&fixture.workspace).unwrap();
    assert_ne!(canonical, profile.cwd);
    assert_eq!(native_context_path(&canonical), profile.cwd);
    assert_eq!(
        windows_workspace(profile.cwd.to_str().unwrap()).unwrap().0,
        task.workspace
    );
    assert!(!profile.private_root.exists());
    let root = fixture
        .runtime
        .native_root(&task.workspace_id, &task.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(root.root.state, DelegationNativeRootStateV1::Ready);
    assert_eq!(root.uses[0].state, DelegationNativeUseStateV1::Accepted);
}

#[test]
fn native_root_new_to_continue_reopens_verbatim_base_with_exact_retained_identity() {
    if crate::test_support::isolated_agent_home(concat!(
        module_path!(),
        "::native_root_new_to_continue_reopens_verbatim_base_with_exact_retained_identity"
    )) {
        return;
    }
    let fixture = Fixture::new();
    let (profile, task) = fixture.build();
    let (continued, root, history) = fixture.retained(&profile, &task);
    let base = Path::new(root.managed_base_path.as_deref().unwrap());
    let ordinary = native_context_path(base);
    assert_ne!(base, ordinary);
    assert_eq!(
        continued.verify_ownership(&ordinary, &root).unwrap(),
        root.filesystem_identity.clone().unwrap()
    );
    assert_eq!(
        continued.verify_ownership(base, &root).unwrap(),
        root.filesystem_identity.clone().unwrap()
    );
    continued
        .verify_native_session("71a2b020-312e-49cd-b084-924cbb3642ad")
        .unwrap();
    assert_eq!(
        continued.verify_native_session("another-native-session"),
        Err(DelegationErrorV1::ResumeUnavailable)
    );

    let mut different = root.clone();
    different.managed_base_path = Some(fixture.workspace.to_string_lossy().into_owned());
    assert_eq!(
        continued.verify_ownership(base, &different),
        Err(DelegationErrorV1::ResumeUnavailable)
    );
    let marker = continued.path().join(DELEGATION_NATIVE_ROOT_MARKER_FILE_V1);
    let bytes = fs::read(&marker).unwrap();
    fs::rename(&marker, continued.path().join("previous-marker")).unwrap();
    hiroute_platform::durable::write_private_new(&marker, &bytes).unwrap();
    assert_ne!(
        continued.verify_ownership(base, &root).unwrap(),
        root.filesystem_identity.unwrap()
    );
    fs::remove_file(continued.path().join(&history[0])).unwrap();
    assert!(
        TaskSessionRoot::prepare(
            base,
            &task.workspace_id,
            &task.workspace.root_identity,
            &task.task_id,
            WorkerHarnessV1::QoderCli,
            SessionRootUse::Continue {
                required_history: &history
            }
        )
        .is_err()
    );
}

#[test]
fn retained_native_root_rejects_a_junction_spelling_of_its_managed_base() {
    if crate::test_support::isolated_agent_home(concat!(
        module_path!(),
        "::retained_native_root_rejects_a_junction_spelling_of_its_managed_base"
    )) {
        return;
    }
    let fixture = Fixture::new();
    let (profile, task) = fixture.build();
    let (continued, mut root, _) = fixture.retained(&profile, &task);
    let base = PathBuf::from(root.managed_base_path.as_ref().unwrap());
    let alias = fixture.directory.path().join("base-alias");
    junction::create(&alias, &base).unwrap();
    assert_eq!(
        fs::canonicalize(&alias).unwrap(),
        fs::canonicalize(&base).unwrap()
    );
    root.managed_base_path = Some(alias.to_string_lossy().into_owned());
    assert_eq!(
        continued.verify_ownership(&base, &root),
        Err(DelegationErrorV1::ResumeUnavailable)
    );
    assert!(crate::delegation::profile::same_windows_directory(&alias, &base).is_err());
    fs::remove_dir(alias).unwrap();

    let replacement = Fixture::new();
    let (source, input) = replacement.admitted_profile_input();
    fs::rename(
        &replacement.workspace,
        replacement.directory.path().join("previous-workspace"),
    )
    .unwrap();
    drop(security::create_owner_only_directory(&replacement.workspace).unwrap());
    assert_ne!(
        windows_workspace(replacement.workspace.to_str().unwrap())
            .unwrap()
            .0,
        input.task.workspace
    );
    assert_eq!(
        source.build(input).err(),
        Some(DelegationErrorV1::ResumeUnavailable)
    );
}
