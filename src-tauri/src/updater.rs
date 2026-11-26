//! 自动更新：定期检查 rolling release 上的新版本，由用户确认后下载、校验签名并安装。
//!
//! - 启动后稍等片刻检查一次，之后每 6 小时检查一次（可在设置中关闭；debug 构建不自动检查）；
//! - 发现新版本只提示，不自动安装：安装会退出并重启程序，所以计时进行中不允许安装；
//! - 安装包分段并行下载（见 [`crate::download`]），由 CI 用私钥签名，插件校验签名后才运行安装程序。
//!   Windows 上随后本进程退出，NSIS 安装程序以被动模式安装完成后自动重新启动程序。

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use reqwest::header::{HeaderValue, ACCEPT};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

use crate::download;
use crate::service::AppState;
use crate::timer::Phase;
use crate::tray;

/// 前端监听的更新状态事件名
pub const UPDATE_EVENT: &str = "update-state";

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
/// 安装更新前写入的标记文件：更新后首次启动时即使带 `--hidden` 也显示主窗口
const SHOW_AFTER_UPDATE: &str = "show-after-update";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateStatus {
    /// 尚未检查
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
    },
    Downloading {
        version: String,
        /// 服务器未返回文件大小时为空
        percent: Option<u8>,
    },
    /// 下载并校验完成，正在启动安装程序
    Installing {
        version: String,
    },
    Error {
        message: String,
    },
}

#[derive(Default)]
pub struct UpdateState {
    status: Mutex<UpdateStatus>,
    /// 最近一次检查到的新版本
    pending: Mutex<Option<Update>>,
    /// 检查与下载互斥，避免重复请求
    busy: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 离开作用域时释放 `busy` 标志
struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn try_acquire(flag: &AtomicBool) -> Option<BusyGuard<'_>> {
    (!flag.swap(true, Ordering::SeqCst)).then_some(BusyGuard(flag))
}

pub fn status(app: &AppHandle) -> UpdateStatus {
    lock(&app.state::<UpdateState>().status).clone()
}

fn set_status(app: &AppHandle, status: UpdateStatus) {
    *lock(&app.state::<UpdateState>().status) = status.clone();
    if let Err(err) = app.emit(UPDATE_EVENT, &status) {
        eprintln!("推送更新状态失败：{err}");
    }
    tray::update_updater(app, &status);
}

/// 安装程序启动后插件会直接退出进程，先移除托盘图标等资源，避免通知区域残留图标。
/// 该回调保存在检查得到的 [`Update`] 中，所以检查时就要设置。
fn build_updater(app: &AppHandle) -> tauri_plugin_updater::Result<Updater> {
    let cleanup = app.clone();
    app.updater_builder()
        .on_before_exit(move || cleanup.cleanup_before_exit())
        .build()
}

/// 检查更新。定时检查（`manual` 为假）静默进行，失败只记日志。
pub async fn check(app: &AppHandle, manual: bool) {
    let state = app.state::<UpdateState>();
    let Some(_busy) = try_acquire(&state.busy) else {
        return;
    };
    if manual {
        set_status(app, UpdateStatus::Checking);
    }
    let result = match build_updater(app) {
        Ok(updater) => updater.check().await,
        Err(err) => Err(err),
    };
    match result {
        Ok(Some(update)) => {
            let version = update.version.clone();
            *lock(&state.pending) = Some(update);
            set_status(app, UpdateStatus::Available { version });
        }
        Ok(None) => {
            *lock(&state.pending) = None;
            set_status(app, UpdateStatus::UpToDate);
        }
        Err(err) => {
            eprintln!("检查更新失败：{err}");
            // 之前已发现的新版本仍然有效，继续提示
            let pending = lock(&state.pending).as_ref().map(|u| u.version.clone());
            if let Some(version) = pending {
                set_status(app, UpdateStatus::Available { version });
            } else if manual {
                set_status(
                    app,
                    UpdateStatus::Error {
                        message: "检查更新失败，请检查网络后重试".into(),
                    },
                );
            }
        }
    }
}

fn timer_idle(app: &AppHandle) -> bool {
    app.state::<AppState>().snapshot().phase == Phase::Idle
}

/// 下载并安装已发现的新版本。成功时（Windows）本进程会退出，不会返回。
pub async fn install(app: &AppHandle) -> Result<(), String> {
    const BUSY_TIMER: &str = "计时进行中，停止计时后再更新";
    if !timer_idle(app) {
        return Err(BUSY_TIMER.into());
    }
    let state = app.state::<UpdateState>();
    let Some(_busy) = try_acquire(&state.busy) else {
        return Err("正在检查或下载更新，请稍候".into());
    };
    let Some(update) = lock(&state.pending).clone() else {
        return Err("没有可安装的新版本".into());
    };
    let version = update.version.clone();
    let restore = |app: &AppHandle| {
        set_status(
            app,
            UpdateStatus::Available {
                version: version.clone(),
            },
        );
    };

    set_status(
        app,
        UpdateStatus::Downloading {
            version: version.clone(),
            percent: Some(0),
        },
    );
    let shown = Mutex::new(Some(0u8));
    let on_progress = |done: u64, total: Option<u64>| {
        let percent = total
            .filter(|&total| total > 0)
            .map(|total| u8::try_from(done * 100 / total).unwrap_or(100).min(100));
        // 只在百分比变化时推送，避免每个数据块都刷新界面
        let mut shown = lock(&shown);
        if percent != *shown {
            *shown = percent;
            set_status(
                app,
                UpdateStatus::Downloading {
                    version: version.clone(),
                    percent,
                },
            );
        }
    };
    let bytes = download_package(&update, on_progress)
        .await
        .map_err(|err| {
            eprintln!("下载更新失败：{err}");
            restore(app);
            "下载更新失败，请稍后重试".to_owned()
        })?;
    if let Err(err) = update.verify(&bytes) {
        eprintln!("更新包签名校验失败：{err}");
        restore(app);
        return Err("更新包校验失败，请稍后重试".into());
    }

    // 下载期间用户可能开始了计时，此时放弃安装，避免打断计时
    if !timer_idle(app) {
        restore(app);
        return Err(BUSY_TIMER.into());
    }
    set_status(
        app,
        UpdateStatus::Installing {
            version: version.clone(),
        },
    );
    let marker = marker_path(app);
    if let Some(path) = &marker {
        let _ = fs::write(path, &version);
    }
    match update.install(&bytes) {
        // Windows 上走不到这里；其他平台需要手动重启
        Ok(()) => app.restart(),
        Err(err) => {
            eprintln!("安装更新失败：{err}");
            if let Some(path) = &marker {
                let _ = fs::remove_file(path);
            }
            restore(app);
            Err("安装更新失败，请稍后重试".into())
        }
    }
}

/// 分段并行下载安装包（插件自带的下载只用单个连接），代理与请求头沿用插件的设置。
/// 只下载不校验，调用方随后用 [`Update::verify`] 校验签名。
async fn download_package(
    update: &Update,
    on_progress: impl Fn(u64, Option<u64>) + Sync,
) -> Result<Vec<u8>, String> {
    let mut builder = download::client_builder().user_agent(USER_AGENT);
    if update.no_proxy {
        builder = builder.no_proxy();
    } else if let Some(proxy) = &update.proxy {
        let proxy = reqwest::Proxy::all(proxy.as_str()).map_err(|err| err.to_string())?;
        builder = builder.proxy(proxy);
    }
    let client = builder.build().map_err(|err| err.to_string())?;
    let mut headers = update.headers.clone();
    headers
        .entry(ACCEPT)
        .or_insert(HeaderValue::from_static("application/octet-stream"));
    download::fetch(&client, &update.download_url, &headers, on_progress).await
}

fn marker_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(SHOW_AFTER_UPDATE))
}

/// 是否刚由用户触发的更新重启而来（读取后删除标记）
pub fn take_show_after_update(app: &AppHandle) -> bool {
    marker_path(app).is_some_and(|path| fs::remove_file(path).is_ok())
}

/// 启动定时检查线程：空闲时只是睡眠，几乎不占资源。
pub fn spawn_checker(app: AppHandle) -> std::io::Result<()> {
    if cfg!(debug_assertions) {
        // 开发构建版本号固定为 0.1.0，自动检查只会制造噪音；仍可在设置中手动检查
        return Ok(());
    }
    thread::Builder::new()
        .name("update-checker".into())
        .spawn(move || {
            thread::sleep(FIRST_CHECK_DELAY);
            loop {
                if app.state::<AppState>().settings().auto_update {
                    tauri::async_runtime::block_on(check(&app, false));
                }
                thread::sleep(CHECK_INTERVAL);
            }
        })
        .map(drop)
}
