// release 构建不弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod fit;
mod service;
mod settings;
mod sound;
mod timer;
mod tray;
mod ui;
mod updater;

use tauri::{Manager, Window, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::service::AppState;
use crate::updater::UpdateState;

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            app.manage(AppState::new(settings_path));
            app.manage(UpdateState::default());
            tray::create(app.handle())?;
            service::spawn_ticker(app.handle().clone())?;
            updater::spawn_checker(app.handle().clone())?;
            service::refresh(app.handle());

            // 用户手动更新后由安装程序重启：即使是开机自启动带来的 --hidden 参数也显示主窗口
            let show = !std::env::args().any(|arg| arg == HIDDEN_ARG)
                || updater::take_show_after_update(app.handle());
            if let Some(window) = app.get_webview_window(ui::MAIN) {
                let _ = window.set_background_color(Some(ui::background_for(window.theme().ok())));
                ui::fit_to_work_area(&window);
                if show {
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
            commands::set_mode,
            commands::pause,
            commands::resume,
            commands::advance,
            commands::stop,
            commands::get_settings,
            commands::update_settings,
            commands::get_update_status,
            commands::check_update,
            commands::install_update,
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
