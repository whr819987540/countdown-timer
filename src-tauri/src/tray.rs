//! 系统托盘：显示剩余时间，并提供不打开主窗口即可控制计时的菜单。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::timer::{format_clock, Mode, Phase, Snapshot, Stage, TimerError};
use crate::{service, ui};

struct TrayHandles {
    icon: TrayIcon,
    toggle: MenuItem<Wry>,
    stop: MenuItem<Wry>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "开始", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "停止", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &stop,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("倒计时")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => ui::show_main(app),
            "toggle" => report(service::toggle(app)),
            "stop" => report(service::stop(app)),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                ui::show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder.build(app)?;

    app.manage(TrayHandles { icon, toggle, stop });
    Ok(())
}

/// 同步托盘提示文字；阶段变化时同时更新菜单项。
pub fn update(app: &AppHandle, snapshot: &Snapshot, phase_changed: bool) {
    let Some(handles) = app.try_state::<TrayHandles>() else {
        return;
    };
    let _ = handles.icon.set_tooltip(Some(tooltip(snapshot)));
    if phase_changed {
        let _ = handles.toggle.set_text(toggle_label(snapshot));
        let _ = handles.stop.set_enabled(snapshot.phase != Phase::Idle);
    }
}

fn toggle_label(snapshot: &Snapshot) -> &'static str {
    match (snapshot.mode, snapshot.stage, snapshot.phase) {
        (_, _, Phase::Idle) => "开始",
        (_, _, Phase::Running) => "暂停",
        (_, _, Phase::Paused) => "继续",
        (Mode::Normal, _, Phase::Finished) => "重置",
        (Mode::EyeCare, Stage::Work, Phase::Finished) => "开始休息",
        (Mode::EyeCare, Stage::Rest, Phase::Finished) => "开始下一轮",
    }
}

fn tooltip(snapshot: &Snapshot) -> String {
    let remaining = format_clock(snapshot.remaining_ms);
    match (snapshot.mode, snapshot.stage, snapshot.phase) {
        (Mode::Normal, _, Phase::Idle) => {
            format!("倒计时 · 就绪 {}", format_clock(snapshot.duration_ms))
        }
        (Mode::Normal, _, Phase::Running) => format!("倒计时 · 剩余 {remaining}"),
        (Mode::Normal, _, Phase::Paused) => format!("倒计时 · 已暂停 {remaining}"),
        (Mode::Normal, _, Phase::Finished) => "倒计时 · 时间到！".to_owned(),
        (Mode::EyeCare, _, Phase::Idle) => "20-20-20 护眼 · 就绪".to_owned(),
        (Mode::EyeCare, Stage::Work, Phase::Running) => format!("20-20-20 · 专注剩余 {remaining}"),
        (Mode::EyeCare, Stage::Rest, Phase::Running) => format!("20-20-20 · 远眺休息 {remaining}"),
        (Mode::EyeCare, _, Phase::Paused) => format!("20-20-20 · 已暂停 {remaining}"),
        (Mode::EyeCare, Stage::Work, Phase::Finished) => "20-20-20 · 该休息眼睛了！".to_owned(),
        (Mode::EyeCare, Stage::Rest, Phase::Finished) => "20-20-20 · 休息结束".to_owned(),
    }
}

fn report(result: Result<(), TimerError>) {
    if let Err(err) = result {
        eprintln!("托盘操作失败：{err}");
    }
}
