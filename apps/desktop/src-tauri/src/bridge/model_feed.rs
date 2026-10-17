//! Shared daemon-owned feed commands; WebView cannot supply a URL or verification key.
use super::*;
use hiroute_application_api::*;

#[tauri::command]
pub async fn model_feed_status(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<ModelFeedStatusV1, DesktopFailure> {
    main_window(&window)?;
    let guard = state.0.lock().await;
    let session = guard.as_ref().ok_or("RESIDENT_UNAVAILABLE")?;
    crate::session::query(
        &session.client,
        "GetModelFeedStatus",
        &ClientEmptyRequestV1::default(),
    )
    .await
}

#[tauri::command]
pub async fn model_feed_refresh(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<ModelFeedStatusV1, DesktopFailure> {
    main_window(&window)?;
    let guard = state.0.lock().await;
    let session = guard.as_ref().ok_or("RESIDENT_UNAVAILABLE")?;
    crate::session::query(
        &session.client,
        "RefreshModelFeed",
        &ClientEmptyRequestV1::default(),
    )
    .await
}

#[tauri::command]
pub async fn model_feed_settings_apply(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    input: ModelFeedSettingsRequestV1,
) -> Result<ModelFeedStatusV1, DesktopFailure> {
    main_window(&window)?;
    let guard = state.0.lock().await;
    let session = guard.as_ref().ok_or("RESIDENT_UNAVAILABLE")?;
    crate::session::query(&session.client, "ApplyModelFeedSettings", &input).await
}

#[tauri::command]
pub async fn model_feed_remove_preview(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<ModelFeedRemovalPreviewV1, DesktopFailure> {
    main_window(&window)?;
    let guard = state.0.lock().await;
    let session = guard.as_ref().ok_or("RESIDENT_UNAVAILABLE")?;
    crate::session::query(
        &session.client,
        "PreviewModelFeedRemoval",
        &ClientEmptyRequestV1::default(),
    )
    .await
}

#[tauri::command]
pub async fn model_feed_remove_apply(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    input: ModelFeedRemoveRequestV1,
) -> Result<ModelFeedStatusV1, DesktopFailure> {
    main_window(&window)?;
    let guard = state.0.lock().await;
    let session = guard.as_ref().ok_or("RESIDENT_UNAVAILABLE")?;
    crate::session::query(&session.client, "ApplyModelFeedRemoval", &input).await
}

#[tauri::command]
pub async fn model_availability(
    window: WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<ModelAvailabilityResultV1, DesktopFailure> {
    main_window(&window)?;
    let guard = state.0.lock().await;
    let session = guard.as_ref().ok_or("RESIDENT_UNAVAILABLE")?;
    crate::session::query(
        &session.client,
        "GetModelAvailability",
        &ClientEmptyRequestV1::default(),
    )
    .await
}
