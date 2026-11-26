//! 计时服务：持有状态机和设置，运行后台计时线程，并把状态同步到前端、托盘、悬浮窗和提醒弹窗。
//!
//! 线程模型：
//! - 命令 / 托盘菜单在主线程修改状态机后立即返回；
//! - 计时线程在每个整秒边界被唤醒，检测本轮是否结束；
//! - 所有界面副作用（推送事件、更新托盘、开关窗口、提示音）统一排队到主线程的 [`publish`]
//!   执行，并在执行时读取最新状态，因此不会出现旧状态覆盖新状态的问题；
//! - [`publish`] 绝不能在命令或事件回调中同步执行：它可能创建窗口，见 [`refresh`]。

use std::path::PathBuf;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::settings::Settings;
use crate::timer::{Mode, Phase, Snapshot, Stage, Timer, TimerError};
use crate::{sound, tray, ui};

/// 前端监听的状态事件名
pub const STATE_EVENT: &str = "timer-state";

pub struct AppState {
    timer: Mutex<Timer>,
    /// 状态被修改时唤醒计时线程
    wake: Condvar,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    /// 最近一次同步到界面的快照，用来识别阶段切换
    published: Mutex<Option<Snapshot>>,
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        let settings = Settings::load(&settings_path);
        Self {
            timer: Mutex::new(Timer::new(settings.duration(), settings.mode)),
            wake: Condvar::new(),
            settings: Mutex::new(settings),
            settings_path,
            published: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.timer).snapshot(Instant::now())
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    /// 修改设置并立即写盘；写盘失败只记录日志，不影响本次运行。
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> Settings {
        let mut settings = lock(&self.settings);
        change(&mut settings);
        if let Err(err) = settings.save(&self.settings_path) {
            eprintln!("保存设置失败：{err}");
        }
        settings.clone()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn apply(
    app: &AppHandle,
    action: impl FnOnce(&mut Timer, Instant) -> Result<(), TimerError>,
) -> Result<(), TimerError> {
    let state = app.state::<AppState>();
    action(&mut lock(&state.timer), Instant::now())?;
    state.wake.notify_all();
    refresh(app);
    Ok(())
}

/// 开始新的倒计时。给出时长时记住它供下次使用；省略时沿用上次的时长（护眼模式时长固定）。
pub fn start(app: &AppHandle, duration: Option<Duration>) -> Result<(), TimerError> {
    apply(app, |timer, now| timer.start(duration, now))?;
    if let Some(duration) = duration {
        let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        app.state::<AppState>()
            .update_settings(|settings| settings.duration_ms = duration_ms);
    }
    Ok(())
}

/// 切换计时模式（仅空闲时可用），并记住该模式供下次启动使用。
pub fn set_mode(app: &AppHandle, mode: Mode) -> Result<(), TimerError> {
    apply(app, |timer, _| timer.set_mode(mode))?;
    app.state::<AppState>()
        .update_settings(|settings| settings.mode = mode);
    Ok(())
}

pub fn pause(app: &AppHandle) -> Result<(), TimerError> {
    apply(app, Timer::pause)
}

pub fn resume(app: &AppHandle) -> Result<(), TimerError> {
    apply(app, Timer::resume)
}

/// 本段结束后进入下一段："重置"再来一轮，或护眼模式的开始休息 / 开始下一轮。
pub fn advance(app: &AppHandle) -> Result<(), TimerError> {
    apply(app, Timer::advance)
}

pub fn stop(app: &AppHandle) -> Result<(), TimerError> {
    apply(app, |timer, _| {
        timer.stop();
        Ok(())
    })
}

/// 托盘菜单的主操作：按当前阶段执行 开始 / 暂停 / 继续 / 进入下一段。
pub fn toggle(app: &AppHandle) -> Result<(), TimerError> {
    let phase = lock(&app.state::<AppState>().timer).phase();
    match phase {
        Phase::Idle => start(app, None),
        Phase::Running => pause(app),
        Phase::Paused => resume(app),
        Phase::Finished => advance(app),
    }
}

/// 安排一次界面同步（在主线程执行）。
///
/// 总是先转到后台线程再派发到主线程：同步命令和托盘菜单的回调本身就运行在主线程上，
/// 此时 `run_on_main_thread` 会**立即同步执行**，而 [`publish`] 可能创建窗口（悬浮窗、提醒弹窗）——
/// 在同步命令或事件回调中创建窗口会在 Windows 上死锁（WebView2 的限制），整个程序失去响应。
/// 从后台线程派发则排进事件循环，在当前回调返回后执行。
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let target = handle.clone();
        if let Err(err) = handle.run_on_main_thread(move || publish(&target)) {
            eprintln!("同步界面失败：{err}");
        }
    });
}

fn publish(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot();
    let previous = lock(&state.published).replace(snapshot);
    let view = |s: &Snapshot| (s.mode, s.stage, s.phase);
    let changed = previous.as_ref().map(view) != Some(view(&snapshot));

    if let Err(err) = app.emit(STATE_EVENT, snapshot) {
        eprintln!("推送状态失败：{err}");
    }
    tray::update(app, &snapshot, changed);

    if !changed {
        return;
    }
    // 一次计时开始时缩为悬浮窗，结束（回到空闲）时关闭悬浮窗
    let was_idle = previous.map_or(true, |previous| previous.phase == Phase::Idle);
    if was_idle && snapshot.phase != Phase::Idle {
        if state.settings().mini_on_start {
            ui::show_mini(app);
        }
    } else if !was_idle && snapshot.phase == Phase::Idle {
        ui::exit_mini(app);
    }
    if previous.is_some_and(|previous| previous.phase == Phase::Finished) {
        sound::stop_alarm();
    }
    let title = match snapshot.mode {
        Mode::Normal => "时间到",
        Mode::EyeCare => "20-20-20 护眼",
    };
    if snapshot.phase == Phase::Finished {
        if state.settings().sound {
            sound::start_alarm();
        }
        ui::show_alert(app, title, true);
    } else if snapshot.stage == Stage::Rest {
        // 远眺休息期间弹窗保持打开并显示倒计时；从"该休息了"切换过来时不再抢焦点
        ui::show_alert(app, title, false);
    } else {
        ui::close_alert(app);
    }
}

/// 启动后台计时线程。线程空闲时阻塞等待，运行时只在整秒边界醒来，几乎不占 CPU。
pub fn spawn_ticker(app: AppHandle) -> std::io::Result<()> {
    thread::Builder::new()
        .name("countdown-ticker".into())
        .spawn(move || run_ticker(&app))
        .map(drop)
}

fn run_ticker(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut timer = lock(&state.timer);
    loop {
        timer = match timer.next_wakeup(Instant::now()) {
            Some(wait) => {
                state
                    .wake
                    .wait_timeout(timer, wait)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0
            }
            None => state
                .wake
                .wait(timer)
                .unwrap_or_else(PoisonError::into_inner),
        };
        if timer.phase() == Phase::Running {
            timer.tick(Instant::now());
            refresh(app);
        }
    }
}
