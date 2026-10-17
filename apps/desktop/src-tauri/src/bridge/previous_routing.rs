use super::*;
use hiroute_application_api::{AgentPreviousRoutingPreviewV1, AgentPreviousRoutingQueryV1};

#[tauri::command]
pub async fn preview_agent_previous_routing(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    input: AgentPreviousRoutingQueryV1,
) -> Result<AgentPreviousRoutingPreviewV1, DesktopFailure> {
    main_window(&window)?;
    let guard = state.0.lock().await;
    let session = guard.as_ref().ok_or("RESIDENT_UNAVAILABLE")?;
    crate::session::query(&session.client, "PreviewAgentPreviousRouting", &input).await
}
