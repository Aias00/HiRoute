use super::web_confirmation::{WebConfirmationPrompt, request_web_confirmation};
use super::{ClientContext, CommandState, DesktopFailure, main_window};
use std::sync::atomic::Ordering;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationDeleteInput {
    session_id: String,
    data_class: hiroute_application_api::DeletionDataClass,
    #[serde(default)]
    language: Option<String>,
}

pub async fn observation_delete(
    window: &ClientContext<'_>,
    state: &CommandState<'_>,
    input: ObservationDeleteInput,
) -> Result<serde_json::Value, DesktopFailure> {
    main_window(window)?;
    {
        let epoch = state.2.load(Ordering::SeqCst);
        let spec = hiroute_application_api::SessionDeletionSpecV1 {
            workspace_id: hiroute_domain::WorkspaceId::default(),
            session_id: hiroute_domain::SessionId::parse(input.session_id)
                .map_err(|_| "SESSION_INVALID")?,
            data_class: input.data_class,
            delete_rollups: false,
        };
        let preview = super::confirmation_session(state, epoch)
            .await?
            .as_mut()
            .ok_or("RESIDENT_UNAVAILABLE")?
            .deletion_preview(spec)
            .await?;
        let task_refs: u64 = preview
            .managed_scopes
            .iter()
            .map(|scope| scope.reference_count)
            .sum();
        let english = input.language.as_deref() == Some("en");
        let (title, message, confirm, cancel) = localized_confirmation(
            english,
            &preview.session.spec.data_class,
            preview.session.requests,
            preview.session.content_instances,
            task_refs,
        );
        if !request_web_confirmation(
            window,
            WebConfirmationPrompt {
                title: title.into(),
                message,
                confirm_label: confirm.into(),
                cancel_label: cancel.into(),
            },
        )
        .await
        {
            return Ok(serde_json::json!({"cancelled":true}));
        }
        if state.2.load(Ordering::SeqCst) != epoch || !window.is_visible().unwrap_or(false) {
            return Err("CONFIRMATION_STALE".into());
        }
        let outcome = super::confirmation_session(state, epoch)
            .await?
            .as_mut()
            .ok_or("RESIDENT_UNAVAILABLE")?
            .deletion_apply(preview)
            .await?;
        serde_json::to_value(outcome).map_err(|_| "RESPONSE_DATA_INVALID".into())
    }
}

fn localized_confirmation(
    english: bool,
    data_class: &hiroute_application_api::DeletionDataClass,
    requests: u64,
    content_instances: u64,
    task_refs: u64,
) -> (&'static str, String, &'static str, &'static str) {
    if english {
        let scope = if *data_class == hiroute_application_api::DeletionDataClass::ContentOnly {
            "This removes prompts, answers and tool content while keeping the run record."
        } else {
            "This removes the session and its per-request records."
        };
        (
            "HiRoute · Clear this session",
            format!(
                "{scope}\n\nAffected: {requests} requests, {content_instances} content items and {task_refs} linked task content items.\n\nThis affects only this session and cannot be undone. Your working folders will not be deleted."
            ),
            "Clear session",
            "Cancel",
        )
    } else {
        let scope = if *data_class == hiroute_application_api::DeletionDataClass::ContentOnly {
            "将清理问题、回答和工具内容，保留运行记录。"
        } else {
            "将删除这条会话及其逐请求记录。"
        };
        (
            "HiRoute · 清理此会话",
            format!(
                "{scope}\n\n影响范围：{requests} 个请求、{content_instances} 段会话正文、{task_refs} 条关联任务正文。\n\n只影响这条会话且不可撤销；不会删除工作目录。"
            ),
            "确认清理",
            "取消",
        )
    }
}

pub async fn observation_capacity_get(
    window: &ClientContext<'_>,
    state: &CommandState<'_>,
) -> Result<hiroute_domain::ObservationCapacityStatusV1, DesktopFailure> {
    observation_capacity_call(window, state, None).await
}
pub async fn observation_capacity_set(
    window: &ClientContext<'_>,
    state: &CommandState<'_>,
    input: hiroute_application_api::ObservationCapacityUpdateV1,
) -> Result<hiroute_domain::ObservationCapacityStatusV1, DesktopFailure> {
    if input.max_active_bytes == Some(0) {
        return Err("REQUEST_INVALID".into());
    }
    observation_capacity_call(window, state, Some(input)).await
}
async fn observation_capacity_call(
    window: &ClientContext<'_>,
    state: &CommandState<'_>,
    update: Option<hiroute_application_api::ObservationCapacityUpdateV1>,
) -> Result<hiroute_domain::ObservationCapacityStatusV1, DesktopFailure> {
    main_window(window)?;
    let client = state
        .0
        .lock()
        .await
        .as_ref()
        .ok_or("RESIDENT_UNAVAILABLE")?
        .client
        .clone();
    let envelope = client
        .observation_capacity(&crate::random_id()?, update)
        .await?;
    if envelope.error.is_some() {
        let value = serde_json::to_value(envelope).map_err(|_| "RESPONSE_DATA_INVALID")?;
        return Err(DesktopFailure::backend(
            serde_json::from_value(value).map_err(|_| "RESPONSE_DATA_INVALID")?,
        ));
    }
    envelope.data.ok_or_else(|| "RESPONSE_DATA_MISSING".into())
}
