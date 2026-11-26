//! 让窗口完整落在显示器工作区内的纯计算（不依赖 Tauri，便于单元测试）。

/// 物理像素矩形
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 需要调整到的内容区尺寸与外框位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub inner_width: u32,
    pub inner_height: u32,
    pub x: i32,
    pub y: i32,
}

/// `outer` 是窗口外框（含标题栏与边框）的位置和尺寸，`inner` 是内容区尺寸。
///
/// 窗口已完整位于工作区内时返回 `None`；否则把内容区缩小到放得下，并在工作区内居中。
pub fn fit_window(area: Rect, outer: Rect, inner: (u32, u32)) -> Option<Fit> {
    let (area_x, area_y) = (i64::from(area.x), i64::from(area.y));
    let (area_w, area_h) = (i64::from(area.width), i64::from(area.height));
    let chrome_w = i64::from(outer.width.saturating_sub(inner.0));
    let chrome_h = i64::from(outer.height.saturating_sub(inner.1));

    let inner_w = i64::from(inner.0).min(area_w - chrome_w).max(1);
    let inner_h = i64::from(inner.1).min(area_h - chrome_h).max(1);
    let (outer_w, outer_h) = (inner_w + chrome_w, inner_h + chrome_h);
    let (x, y) = (i64::from(outer.x), i64::from(outer.y));

    let resized = inner_w != i64::from(inner.0) || inner_h != i64::from(inner.1);
    let inside = x >= area_x
        && y >= area_y
        && x + outer_w <= area_x + area_w
        && y + outer_h <= area_y + area_h;
    if !resized && inside {
        return None;
    }
    let center = |start: i64, span: i64, len: i64| {
        i32::try_from(start + (span - len).max(0) / 2).unwrap_or(i32::MAX)
    };
    Some(Fit {
        inner_width: u32::try_from(inner_w).unwrap_or(inner.0),
        inner_height: u32::try_from(inner_h).unwrap_or(inner.1),
        x: center(area_x, area_w, outer_w),
        y: center(area_y, area_h, outer_h),
    })
}

/// `rect` 是否完整位于 `area` 内
pub fn contains(area: Rect, rect: Rect) -> bool {
    let right = |r: Rect| i64::from(r.x) + i64::from(r.width);
    let bottom = |r: Rect| i64::from(r.y) + i64::from(r.height);
    rect.x >= area.x
        && rect.y >= area.y
        && right(rect) <= right(area)
        && bottom(rect) <= bottom(area)
}

/// 把 `size` 大小的窗口放在 `area` 右上角、留出 `margin` 边距时的左上角坐标
pub fn top_right(area: Rect, size: (u32, u32), margin: u32) -> (i32, i32) {
    let x = i64::from(area.x) + i64::from(area.width) - i64::from(size.0) - i64::from(margin);
    let y = i64::from(area.y) + i64::from(margin);
    (
        i32::try_from(x.max(i64::from(area.x))).unwrap_or(area.x),
        i32::try_from(y).unwrap_or(area.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1920×1080 屏幕、175% 缩放、底部任务栏 84 物理像素（48 逻辑像素）
    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 996,
    };
    /// 380×600 逻辑像素内容区 + 标题栏 / 边框，按 175% 换算
    const INNER: (u32, u32) = (665, 1050);
    const CHROME: (u32, u32) = (16, 64);

    fn outer_at(x: i32, y: i32, inner: (u32, u32)) -> Rect {
        Rect {
            x,
            y,
            width: inner.0 + CHROME.0,
            height: inner.1 + CHROME.1,
        }
    }

    #[test]
    fn leaves_fitting_window_alone() {
        let inner = (380, 600);
        assert_eq!(fit_window(AREA, outer_at(700, 200, inner), inner), None);
    }

    #[test]
    fn shrinks_and_centers_window_taller_than_work_area() {
        // 系统按整块屏幕居中：y 为负，底部落到任务栏后面
        let fit = fit_window(AREA, outer_at(627, -17, INNER), INNER).unwrap();
        assert_eq!((fit.inner_width, fit.inner_height), (665, 996 - 64));
        assert_eq!(fit.y, 0);
        assert_eq!(fit.x, (1920 - 681) / 2);
    }

    #[test]
    fn moves_window_covered_by_taskbar_without_resizing() {
        // 尺寸放得下，但按整块屏幕居中后底部被任务栏遮住
        let inner = (665, 900);
        let fit = fit_window(AREA, outer_at(627, 58, inner), inner).unwrap();
        assert_eq!((fit.inner_width, fit.inner_height), inner);
        assert_eq!(fit.y, (996 - 964) / 2);
    }

    #[test]
    fn respects_work_area_offset() {
        // 任务栏在顶部，或副屏位于主屏左侧（负坐标）
        let area = Rect {
            x: -1920,
            y: 84,
            width: 1920,
            height: 996,
        };
        let fit = fit_window(area, outer_at(-1293, 0, INNER), INNER).unwrap();
        assert_eq!(fit.y, 84);
        assert!(fit.x >= -1920 && fit.x + 681 <= 0);
    }

    #[test]
    fn checks_containment() {
        let rect = |x, y| Rect {
            x,
            y,
            width: 420,
            height: 112,
        };
        assert!(contains(AREA, rect(0, 0)));
        assert!(contains(AREA, rect(1500, 884)));
        assert!(!contains(AREA, rect(1501, 0)));
        assert!(!contains(AREA, rect(0, 885)));
        assert!(!contains(AREA, rect(-1, 10)));
    }

    #[test]
    fn places_window_at_top_right() {
        assert_eq!(top_right(AREA, (420, 112), 28), (1920 - 420 - 28, 28));
        let offset = Rect {
            x: -1920,
            y: 84,
            width: 1920,
            height: 996,
        };
        assert_eq!(top_right(offset, (420, 112), 28), (-420 - 28, 84 + 28));
    }
}
