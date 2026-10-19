//! 暴露给前端的 Tauri 命令。状态变化通过 `timer-state` 事件推送，命令本身不返回快照。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::service::{self, AppState};
use crate::sound;
use crate::timer::{Snapshot, TimerError};

type CommandResult<T = ()> = Result<T, String>;

fn message(err: TimerError) -> String {
    err.to_string()
}

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> Snapshot {
    state.snapshot()
}

#[tauri::command]
pub fn start(app: AppHandle, duration_ms: u64) -> CommandResult {
    service::start(&app, Duration::from_millis(duration_ms)).map_err(message)
}

#[tauri::command]
pub fn pause(app: AppHandle) -> CommandResult {
    service::pause(&app).map_err(message)
}

#[tauri::command]
pub fn resume(app: AppHandle) -> CommandResult {
    service::resume(&app).map_err(message)
}

#[tauri::command]
pub fn restart(app: AppHandle) -> CommandResult {
    service::restart(&app).map_err(message)
}

#[tauri::command]
pub fn stop(app: AppHandle) -> CommandResult {
    service::stop(&app).map_err(message)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    duration_ms: u64,
    sound: bool,
    close_to_tray: bool,
    /// 开机自启动的真实状态以注册表为准，不存进 settings.json
    autostart: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    sound: Option<bool>,
    close_to_tray: Option<bool>,
    autostart: Option<bool>,
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsView {
    settings_view(&app)
}

#[tauri::command]
pub fn update_settings(app: AppHandle, patch: SettingsPatch) -> CommandResult<SettingsView> {
    if let Some(enabled) = patch.autostart {
        let autolaunch = app.autolaunch();
        if autolaunch.is_enabled().unwrap_or(false) != enabled {
            let result = if enabled {
                autolaunch.enable()
            } else {
                autolaunch.disable()
            };
            result.map_err(|err| format!("修改开机自启动失败：{err}"))?;
        }
    }
    if patch.sound == Some(false) {
        sound::stop_alarm();
    }
    app.state::<AppState>().update_settings(|settings| {
        if let Some(sound) = patch.sound {
            settings.sound = sound;
        }
        if let Some(close_to_tray) = patch.close_to_tray {
            settings.close_to_tray = close_to_tray;
        }
    });
    Ok(settings_view(&app))
}

fn settings_view(app: &AppHandle) -> SettingsView {
    let settings = app.state::<AppState>().settings();
    SettingsView {
        duration_ms: settings.duration_ms,
        sound: settings.sound,
        close_to_tray: settings.close_to_tray,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
