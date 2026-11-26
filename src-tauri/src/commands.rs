//! 暴露给前端的 Tauri 命令。状态变化通过 `timer-state` 事件推送，命令本身不返回快照。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::service::{self, AppState};
use crate::timer::{Mode, Snapshot, TimerError};
use crate::updater::{self, UpdateStatus};
use crate::{sound, ui};

type CommandResult<T = ()> = Result<T, String>;

fn message(err: TimerError) -> String {
    err.to_string()
}

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> Snapshot {
    state.snapshot()
}

/// 省略 `duration_ms` 时沿用上次的时长（护眼模式时长固定，前端不传）。
#[tauri::command]
pub fn start(app: AppHandle, duration_ms: Option<u64>) -> CommandResult {
    service::start(&app, duration_ms.map(Duration::from_millis)).map_err(message)
}

#[tauri::command]
pub fn set_mode(app: AppHandle, mode: Mode) -> CommandResult {
    service::set_mode(&app, mode).map_err(message)
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
pub fn advance(app: AppHandle) -> CommandResult {
    service::advance(&app).map_err(message)
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
    auto_update: bool,
    mini_on_start: bool,
    /// 开机自启动的真实状态以注册表为准，不存进 settings.json
    autostart: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    sound: Option<bool>,
    close_to_tray: Option<bool>,
    auto_update: Option<bool>,
    mini_on_start: Option<bool>,
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
        if let Some(auto_update) = patch.auto_update {
            settings.auto_update = auto_update;
        }
        if let Some(mini_on_start) = patch.mini_on_start {
            settings.mini_on_start = mini_on_start;
        }
    });
    if patch.auto_update == Some(true) {
        // 刚打开自动检查时立即检查一次，不必等到下一个周期
        let app = app.clone();
        tauri::async_runtime::spawn(async move { updater::check(&app, false).await });
    }
    Ok(settings_view(&app))
}

fn settings_view(app: &AppHandle) -> SettingsView {
    let settings = app.state::<AppState>().settings();
    SettingsView {
        duration_ms: settings.duration_ms,
        sound: settings.sound,
        close_to_tray: settings.close_to_tray,
        auto_update: settings.auto_update,
        mini_on_start: settings.mini_on_start,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

// 创建 / 销毁窗口的命令必须是 async：同步命令在主线程上执行，Windows 上创建窗口会死锁

/// 悬浮窗的"展开"：关闭悬浮窗并显示主窗口。
#[tauri::command]
pub async fn show_main(app: AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || ui::show_main(&handle));
}

/// 主窗口的"缩为悬浮窗"。
#[tauri::command]
pub async fn show_mini(app: AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || ui::show_mini(&handle));
}

#[tauri::command]
pub fn get_update_status(app: AppHandle) -> UpdateStatus {
    updater::status(&app)
}

/// 手动检查更新；结果通过 `update-state` 事件推送。
#[tauri::command]
pub async fn check_update(app: AppHandle) {
    updater::check(&app, true).await;
}

/// 下载并安装新版本；成功时程序会退出并由安装程序重新启动，不会返回。
#[tauri::command]
pub async fn install_update(app: AppHandle) -> CommandResult {
    updater::install(&app).await
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
