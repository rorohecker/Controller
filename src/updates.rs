use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct PendingUpdate(pub Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    current_version: String,
}

#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateInfo>, String> {
    let before_exit = app.clone();
    let update = app
        .updater_builder()
        .on_before_exit(move || {
            if let Some(bridge) = before_exit.try_state::<Arc<crate::Bridge>>() {
                bridge.shutdown();
            }
        })
        .build()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?;
    let info = update.as_ref().map(|update| UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
    });
    *pending
        .0
        .lock()
        .map_err(|_| "Update state is unavailable")? = update;
    Ok(info)
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let update = pending
        .0
        .lock()
        .map_err(|_| "Update state is unavailable")?
        .take()
        .ok_or("Check for an update first")?;
    let mut downloaded = 0_u64;
    let progress_app = app.clone();
    let finish_app = app.clone();
    update
        .download_and_install(
            move |chunk_length, total| {
                downloaded = downloaded.saturating_add(chunk_length as u64);
                let _ = progress_app.emit(
                    "update-progress",
                    json!({"downloaded": downloaded, "total": total}),
                );
            },
            move || {
                let _ = finish_app.emit("update-progress", json!({"finished": true}));
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}
