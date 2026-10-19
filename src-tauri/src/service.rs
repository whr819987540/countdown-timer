//! 计时服务：持有状态机和设置，运行后台计时线程，并把状态同步到前端、托盘和提醒弹窗。
//!
//! 线程模型：
//! - 命令 / 托盘菜单在主线程修改状态机后立即返回；
//! - 计时线程在每个整秒边界被唤醒，检测本轮是否结束；
//! - 所有界面副作用（推送事件、更新托盘、开关弹窗、提示音）统一排队到主线程的 [`publish`]
//!   执行，并在执行时读取最新状态，因此不会出现旧状态覆盖新状态的问题。

use std::path::PathBuf;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::settings::Settings;
use crate::timer::{Phase, Snapshot, Timer, TimerError};
use crate::{sound, tray, ui};

/// 前端监听的状态事件名
pub const STATE_EVENT: &str = "timer-state";

pub struct AppState {
    timer: Mutex<Timer>,
    /// 状态被修改时唤醒计时线程
    wake: Condvar,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    /// 最近一次同步到界面的阶段，用来识别阶段切换
    published_phase: Mutex<Phase>,
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        let settings = Settings::load(&settings_path);
        Self {
            timer: Mutex::new(Timer::new(settings.duration())),
            wake: Condvar::new(),
            settings: Mutex::new(settings),
            settings_path,
            published_phase: Mutex::new(Phase::Idle),
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

/// 以给定时长开始新的倒计时，并记住该时长供下次使用。
pub fn start(app: &AppHandle, duration: Duration) -> Result<(), TimerError> {
    apply(app, |timer, now| timer.start(duration, now))?;
    let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    app.state::<AppState>()
        .update_settings(|settings| settings.duration_ms = duration_ms);
    Ok(())
}

pub fn pause(app: &AppHandle) -> Result<(), TimerError> {
    apply(app, Timer::pause)
}

pub fn resume(app: &AppHandle) -> Result<(), TimerError> {
    apply(app, Timer::resume)
}

/// "重置"：以相同时长再来一轮。
pub fn restart(app: &AppHandle) -> Result<(), TimerError> {
    apply(app, |timer, now| {
        timer.restart(now);
        Ok(())
    })
}

pub fn stop(app: &AppHandle) -> Result<(), TimerError> {
    apply(app, |timer, _| {
        timer.stop();
        Ok(())
    })
}

/// 托盘菜单的主操作：按当前阶段执行 开始 / 暂停 / 继续 / 重置。
pub fn toggle(app: &AppHandle) -> Result<(), TimerError> {
    let state = app.state::<AppState>();
    let phase = lock(&state.timer).phase();
    match phase {
        Phase::Idle => start(app, state.settings().duration()),
        Phase::Running => pause(app),
        Phase::Paused => resume(app),
        Phase::Finished => restart(app),
    }
}

/// 安排一次界面同步（在主线程执行）。
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || publish(&handle)) {
        eprintln!("同步界面失败：{err}");
    }
}

fn publish(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot();
    let previous = std::mem::replace(&mut *lock(&state.published_phase), snapshot.phase);
    let phase_changed = previous != snapshot.phase;

    if let Err(err) = app.emit(STATE_EVENT, snapshot) {
        eprintln!("推送状态失败：{err}");
    }
    tray::update(app, &snapshot, phase_changed);

    if !phase_changed {
        return;
    }
    if snapshot.phase == Phase::Finished {
        if state.settings().sound {
            sound::start_alarm();
        }
        ui::show_alert(app);
    } else if previous == Phase::Finished {
        sound::stop_alarm();
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
