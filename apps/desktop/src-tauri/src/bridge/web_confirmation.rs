use super::{DesktopFailure, main_window};
use crate::confirmation::CONFIRMATION_TTL;
use std::sync::atomic::Ordering;
use tauri::{Emitter, EventTarget, Manager, State, WebviewWindow};

pub(super) const WEB_CONFIRMATION_EVENT: &str = "hiroute-web-confirmation";

pub(crate) fn expire_window_confirmations(app: &tauri::AppHandle, window_label: &str) {
    app.state::<super::DesktopState>()
        .2
        .fetch_add(1, Ordering::SeqCst);
    app.state::<WebConfirmationState>()
        .cancel_context(window_label);
    app.state::<super::WorkerDependencyConfirmationState>()
        .cancel_window(window_label);
}

pub use hiroute_product_session::web_confirmation::{
    ConfirmationBroker as WebConfirmationState, WebConfirmationDecision, WebConfirmationPrompt,
    WebConfirmationRequest,
};

pub(super) async fn request_web_confirmation(
    window: &WebviewWindow,
    prompt: WebConfirmationPrompt,
) -> bool {
    let state = window.state::<WebConfirmationState>();
    let Ok((request, receive)) = state.begin(window.label(), prompt) else {
        return false;
    };
    let confirmation_id = request.confirmation_id.clone();
    if window
        .emit_to(
            EventTarget::webview(window.label()),
            WEB_CONFIRMATION_EVENT,
            request,
        )
        .is_err()
    {
        state.cancel(window.label(), &confirmation_id);
    }
    let decision = tokio::time::timeout(CONFIRMATION_TTL, receive).await;
    if decision.is_err() {
        state.cancel(window.label(), &confirmation_id);
    }
    decision.ok().and_then(Result::ok).unwrap_or(false)
}

#[tauri::command]
pub(super) async fn web_confirmation_snapshot(
    window: WebviewWindow,
    state: State<'_, WebConfirmationState>,
) -> Result<Option<WebConfirmationRequest>, DesktopFailure> {
    main_window(&window)?;
    Ok(state.snapshot(window.label()))
}

#[tauri::command]
pub(super) async fn resolve_web_confirmation(
    window: WebviewWindow,
    state: State<'_, WebConfirmationState>,
    input: WebConfirmationDecision,
) -> Result<(), DesktopFailure> {
    main_window(&window)?;
    state.resolve(window.label(), &input.confirmation_id, input.accepted)
}
