//! 窗口管理：主窗口、计时中的悬浮窗与提醒弹窗（"时间到"，以及护眼模式的休息提醒）。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::window::Color;
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Theme, UserAttentionType,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::fit::{bottom_right, contains, fit_window, keep_badge_center, Rect};
use crate::service::AppState;
use crate::timer::Phase;

pub const MAIN: &str = "main";
pub const ALERT: &str = "alert";
pub const MINI: &str = "mini";

/// 悬浮窗中圆形徽章的直径（逻辑像素）。窗口按这个尺寸创建，页面里徽章的直径等于窗口高度（见 mini.css）；
/// Windows 不允许窗口窄于约 136 像素，宽度实际会被撑大，见 [`shape_mini`]
const MINI_DISC: f64 = 48.0;
/// 悬浮窗默认放在工作区右下角（任务栏上方），距边缘的距离（逻辑像素）。
/// 不放右上角：那里是最大化窗口的最小化 / 关闭按钮，置顶的悬浮窗会挡住它们
const MINI_MARGIN: f64 = 24.0;
/// 主窗口是否是被悬浮窗替换掉的：一次计时结束、关闭悬浮窗时据此恢复主窗口
static RESTORE_MAIN: AtomicBool = AtomicBool::new(false);

/// 所有窗口共用的 WebView2 启动参数。
///
/// - 同一数据目录下的各窗口参数必须完全一致，否则后创建的窗口会失败，所以主窗口、悬浮窗、提醒弹窗都用这里的值；
/// - 前半部分与 wry 的默认值相同（去掉"迷你菜单"与 SmartScreen）；
/// - wry 总会显式设置启动参数，这会让 WebView2 标准的 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` 环境变量失效，
///   这里把它追加回来——端到端测试借此打开调试端口（见 `e2e/smoke.ts`），平时该变量为空，不影响程序。
pub fn browser_args() -> String {
    let mut args = String::from("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    if let Ok(extra) = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
        let extra = extra.trim();
        if !extra.is_empty() {
            args.push(' ');
            args.push_str(extra);
        }
    }
    args
}

/// 按 tauri.conf.json 中的配置创建主窗口（配置里 `create: false`，以便统一设置 WebView2 启动参数）。
pub fn create_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    WebviewWindowBuilder::from_config(app, &config)?
        .additional_browser_args(&browser_args())
        .build()
}

/// 显示主窗口；悬浮窗打开时先关闭它（"展开"）。
pub fn show_main(app: &AppHandle) {
    close_mini(app);
    RESTORE_MAIN.store(false, Ordering::SeqCst);
    reveal_main(app);
}

fn reveal_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 计时进行中且开启了悬浮窗时，把主窗口缩为悬浮窗（用户关闭 / 最小化主窗口时调用）；返回是否会这样做。
///
/// 窗口事件回调运行在主线程上，在其中直接创建窗口会在 Windows 上死锁（见 AGENTS.md），
/// 所以先切到后台线程，再派发回主线程执行。
pub fn shrink_main_later(app: &AppHandle) -> bool {
    if !wants_mini(app) {
        return false;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let target = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            // 派发期间计时可能已经结束
            if wants_mini(&target) {
                show_mini(&target);
            }
            // 悬浮窗没能打开时：由关闭触发的（主窗口未最小化）照旧隐藏到托盘，不能点了关闭却没反应
            if target.get_webview_window(MINI).is_none() {
                if let Some(main) = target.get_webview_window(MAIN) {
                    if !main.is_minimized().unwrap_or(false) {
                        let _ = main.hide();
                    }
                }
            }
        });
    });
    true
}

fn wants_mini(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state.settings().mini_on_start && state.snapshot().phase != Phase::Idle
}

/// 缩为悬浮窗：在屏幕角落打开置顶的小圆环，并隐藏主窗口。
pub fn show_mini(app: &AppHandle) {
    if app.get_webview_window(MINI).is_some() {
        return;
    }
    let main = app.get_webview_window(MAIN);
    // 透明、无阴影：屏幕上只露出页面画的圆形徽章。先隐藏着创建，摆好位置再显示，避免窗口跳动
    let built = WebviewWindowBuilder::new(app, MINI, WebviewUrl::App("mini.html".into()))
        .title("倒计时")
        .inner_size(MINI_DISC, MINI_DISC)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .additional_browser_args(&browser_args())
        .build();
    let window = match built {
        Ok(window) => window,
        Err(err) => {
            eprintln!("无法打开悬浮窗：{err}");
            return;
        }
    };
    shape_mini(&window.as_ref().window());
    place_mini(app, &window, main.as_ref());
    let _ = window.show();
    if let Some(main) = main {
        RESTORE_MAIN.store(main.is_visible().unwrap_or(false), Ordering::SeqCst);
        let _ = main.hide();
    }
}

/// 把悬浮窗裁剪成贴右侧、直径等于窗口高度的圆形。
///
/// Windows 不允许窗口窄于约 136 像素，48×48 的悬浮窗会被撑宽，两侧多出的透明区域虽然看不见，
/// 却会挡住下面窗口的点击。裁剪后圆形以外既不显示也不接收点击，看起来和用起来都只是一个小圆环。
/// 页面把徽章画在右侧、直径同样取窗口高度（见 mini.css），所以无论缩放比例如何，两者都严丝合缝；
/// 不按"48 × 缩放比例"计算：窗口尺寸没跟上缩放比例时（见 [`refit_mini_later`]）会和页面对不上，圆环被裁掉一块。
/// 窗口尺寸变化时需要重新裁剪。
#[cfg(windows)]
pub fn shape_mini(window: &tauri::Window) {
    use windows_sys::Win32::Graphics::Gdi::{CreateEllipticRgn, DeleteObject, SetWindowRgn};

    // 悬浮窗没有边框和标题栏，窗口区域与页面区域重合
    let (Ok(hwnd), Ok(size)) = (window.hwnd(), window.outer_size()) else {
        return;
    };
    let diameter = i32::try_from(size.height).unwrap_or(i32::MAX);
    let width = i32::try_from(size.width).unwrap_or(i32::MAX);
    if diameter == 0 {
        return;
    }
    let left = (width - diameter).max(0);
    // SAFETY: 只传入整数坐标（相对窗口左上角）；右、下边界不包含在区域内，所以各加 1
    let region = unsafe { CreateEllipticRgn(left, 0, left + diameter + 1, diameter + 1) };
    if region.is_null() {
        return;
    }
    // SAFETY: hwnd 是 Tauri 返回的有效窗口句柄；SetWindowRgn 成功后由系统接管 region
    let applied = unsafe { SetWindowRgn(hwnd.0, region, 1) } != 0;
    if !applied {
        // SAFETY: 设置失败时 region 仍归我们所有，释放它避免泄漏
        unsafe { DeleteObject(region) };
    }
}

#[cfg(not(windows))]
pub fn shape_mini(_window: &tauri::Window) {}

/// 悬浮窗换到缩放比例不同的屏幕后，按新的缩放比例恢复 48 逻辑像素的大小。
///
/// 一般情况下 tao 处理 `WM_DPICHANGED` 时已经按比例调整了窗口尺寸；但系统关闭了"拖动时显示窗口内容"时
/// 它会保留原来的物理尺寸，悬浮窗在高缩放比例的屏幕上就会变小。这里在缩放比例变化处理完之后再设一次尺寸
/// （在事件回调里直接设置会被 tao 随后的调整覆盖，所以延后到主线程的下一轮执行）。
pub fn refit_mini_later(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let target = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if let Some(window) = target.get_webview_window(MINI) {
                resize_mini(&window);
            }
        });
    });
}

/// 把悬浮窗设回 48 逻辑像素，徽章中心保持不动（宽度受系统最小宽度限制，也会随缩放比例变化）。
fn resize_mini(window: &WebviewWindow) {
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let _ = window.set_size(LogicalSize::new(MINI_DISC, MINI_DISC));
    let Ok(resized) = window.outer_size() else {
        return;
    };
    if resized != size {
        let old = Rect {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        };
        let (x, y) = keep_badge_center(old, (resized.width, resized.height));
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

/// 一次计时结束（回到空闲）：关闭悬浮窗；主窗口是被悬浮窗替换掉的就恢复显示。
pub fn exit_mini(app: &AppHandle) {
    if close_mini(app) && RESTORE_MAIN.swap(false, Ordering::SeqCst) {
        reveal_main(app);
    }
}

/// 关闭悬浮窗并记住它的位置；返回悬浮窗之前是否打开着。
fn close_mini(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(MINI) else {
        return false;
    };
    if let Ok(position) = window.outer_position() {
        remember_mini_position(app, position);
    }
    let _ = window.destroy();
    true
}

/// 记住悬浮窗位置（位置没变时不写盘）。
pub fn remember_mini_position(app: &AppHandle, position: PhysicalPosition<i32>) {
    let state = app.state::<AppState>();
    let position = Some((position.x, position.y));
    if state.settings().mini_position != position {
        state.update_settings(|settings| settings.mini_position = position);
    }
}

fn work_rect(monitor: &tauri::Monitor) -> Rect {
    let area = monitor.work_area();
    Rect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    }
}

/// 沿用上次拖到的位置（仍完整位于某个显示器的工作区内时）；否则放在主窗口所在显示器的右下角。
fn place_mini(app: &AppHandle, window: &WebviewWindow, main: Option<&WebviewWindow>) {
    let Ok(size) = window.outer_size() else {
        return;
    };
    if let Some((x, y)) = app.state::<AppState>().settings().mini_position {
        let rect = Rect {
            x,
            y,
            width: size.width,
            height: size.height,
        };
        let monitors = window.available_monitors().unwrap_or_default();
        if monitors
            .iter()
            .any(|monitor| contains(work_rect(monitor), rect))
        {
            let _ = window.set_position(PhysicalPosition::new(x, y));
            return;
        }
    }
    let monitor = main
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let margin = (MINI_MARGIN * monitor.scale_factor()).round() as u32;
    // 徽章贴在窗口右侧、高度与窗口相同，按窗口尺寸摆放即可让徽章距工作区右下角 margin
    let (x, y) = bottom_right(work_rect(&monitor), (size.width, size.height), margin);
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// 弹出置顶的提醒窗口。窗口已存在时，`attention` 为真才把它重新带到前台并闪烁任务栏。
pub fn show_alert(app: &AppHandle, title: &str, attention: bool) {
    if let Some(window) = app.get_webview_window(ALERT) {
        if attention {
            bring_to_front(&window, true);
        }
        return;
    }
    let theme = app
        .get_webview_window(MAIN)
        .and_then(|window| window.theme().ok());
    let built = WebviewWindowBuilder::new(app, ALERT, WebviewUrl::App("alert.html".into()))
        .title(title)
        .inner_size(360.0, 320.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .center()
        .background_color(background_for(theme))
        .additional_browser_args(&browser_args())
        .build();
    match built {
        Ok(window) => {
            fit_to_work_area(&window);
            bring_to_front(&window, attention);
        }
        Err(err) => eprintln!("无法打开提醒窗口：{err}"),
    }
}

/// 关闭提醒窗口并释放其 WebView，保持后台驻留时的内存占用最小。
pub fn close_alert(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(ALERT) {
        let _ = window.destroy();
    }
}

fn bring_to_front(window: &WebviewWindow, attention: bool) {
    let _ = window.show();
    let _ = window.set_focus();
    if attention {
        // Windows 可能拒绝后台程序抢占焦点，此时任务栏图标会闪烁提示
        let _ = window.request_user_attention(Some(UserAttentionType::Critical));
    }
}

/// 让窗口完整落在所在显示器的工作区（去掉任务栏后的区域）内。
///
/// 窗口尺寸以逻辑像素固定，在高缩放比例的屏幕上（例如 1080p 屏幕设为 175%）可能比工作区还高；
/// 而系统按整块屏幕居中，底部会被任务栏遮住或落到屏幕外。此时缩小窗口并在工作区内重新居中，
/// 页面会按可用高度整体缩小（见 `src/lib/fit.ts`），内容仍然完整。
pub fn fit_to_work_area(window: &WebviewWindow) {
    let monitor = match window.current_monitor() {
        Ok(Some(monitor)) => monitor,
        _ => match window.primary_monitor() {
            Ok(Some(monitor)) => monitor,
            _ => return,
        },
    };
    let (Ok(outer), Ok(inner), Ok(position)) = (
        window.outer_size(),
        window.inner_size(),
        window.outer_position(),
    ) else {
        return;
    };
    let area = monitor.work_area();
    let area = Rect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    };
    let outer = Rect {
        x: position.x,
        y: position.y,
        width: outer.width,
        height: outer.height,
    };
    let Some(fit) = fit_window(area, outer, (inner.width, inner.height)) else {
        return;
    };
    if (fit.inner_width, fit.inner_height) != (inner.width, inner.height) {
        let _ = window.set_size(PhysicalSize::new(fit.inner_width, fit.inner_height));
    }
    let _ = window.set_position(PhysicalPosition::new(fit.x, fit.y));
}

/// 与前端 CSS 中的 `--color-bg` 一致，避免 WebView 加载完成前闪白。
pub fn background_for(theme: Option<Theme>) -> Color {
    match theme {
        Some(Theme::Dark) => Color(15, 23, 42, 255),
        _ => Color(248, 250, 252, 255),
    }
}
