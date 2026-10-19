// release 构建不弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod service;
mod settings;
mod sound;
mod timer;
mod tray;
mod ui;

use tauri::{Manager, Window, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::service::AppState;

/// 开机自启动时附带的参数：只驻留托盘，不弹出主窗口
const HIDDEN_ARG: &str = "--hidden";

fn main() {
    tauri::Builder::default()
        // 必须最先注册：重复启动时只唤起已运行的实例
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            ui::show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            app.manage(AppState::new(settings_path));
            tray::create(app.handle())?;
            service::spawn_ticker(app.handle().clone())?;
            service::refresh(app.handle());

            if let Some(window) = app.get_webview_window(ui::MAIN) {
                let _ = window.set_background_color(Some(ui::background_for(window.theme().ok())));
                if !std::env::args().any(|arg| arg == HIDDEN_ARG) {
                    window.show()?;
                    window.set_focus()?;
                }
            }
            Ok(())
        })
        .on_window_event(handle_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::start,
            commands::pause,
            commands::resume,
            commands::restart,
            commands::stop,
            commands::get_settings,
            commands::update_settings,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("启动倒计时程序失败");
}

fn handle_window_event(window: &Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let app = window.app_handle();
    match window.label() {
        ui::MAIN => {
            if app.state::<AppState>().settings().close_to_tray {
                api.prevent_close();
                let _ = window.hide();
            } else {
                app.exit(0);
            }
        }
        ui::ALERT => {
            // 用 Alt+F4 等方式关掉提醒弹窗，等同于点击"停止"；弹窗随后由 publish 销毁
            api.prevent_close();
            if let Err(err) = service::stop(app) {
                eprintln!("停止倒计时失败：{err}");
            }
        }
        _ => {}
    }
}
