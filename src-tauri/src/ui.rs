//! 窗口管理：主窗口与"时间到"提醒弹窗。

use tauri::window::{Color, UserAttentionType};
use tauri::{AppHandle, Manager, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const MAIN: &str = "main";
pub const ALERT: &str = "alert";

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 弹出置顶的提醒窗口；已存在时只把它重新带到前台。
pub fn show_alert(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(ALERT) {
        bring_to_front(&window);
        return;
    }
    let theme = app
        .get_webview_window(MAIN)
        .and_then(|window| window.theme().ok());
    let built = WebviewWindowBuilder::new(app, ALERT, WebviewUrl::App("alert.html".into()))
        .title("时间到")
        .inner_size(360.0, 300.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .center()
        .background_color(background_for(theme))
        .build();
    match built {
        Ok(window) => bring_to_front(&window),
        Err(err) => eprintln!("无法打开提醒窗口：{err}"),
    }
}

/// 关闭提醒窗口并释放其 WebView，保持后台驻留时的内存占用最小。
pub fn close_alert(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(ALERT) {
        let _ = window.destroy();
    }
}

fn bring_to_front(window: &WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
    // Windows 可能拒绝后台程序抢占焦点，此时任务栏图标会闪烁提示
    let _ = window.request_user_attention(Some(UserAttentionType::Critical));
}

/// 与前端 CSS 中的 `--color-bg` 一致，避免 WebView 加载完成前闪白。
pub fn background_for(theme: Option<Theme>) -> Color {
    match theme {
        Some(Theme::Dark) => Color(15, 23, 42, 255),
        _ => Color(248, 250, 252, 255),
    }
}
