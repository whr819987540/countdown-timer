export interface Size {
  width: number;
  height: number;
}

/** 缩放下限：再小文字就难以阅读了，此时由页面滚动兜底 */
export const MIN_SCALE = 0.8;

/**
 * 页面按固定的窗口尺寸设计。高缩放比例的屏幕上，后端会把窗口限制在屏幕工作区内，
 * 可用空间可能小于设计尺寸，此时整体等比缩小，保证按钮等内容完整可见。
 */
export function fitScale(viewport: Size, design: Size): number {
  if (viewport.width <= 0 || viewport.height <= 0) return 1;
  const scale = Math.min(1, viewport.width / design.width, viewport.height / design.height);
  // 向下取整到百分位，避免舍入误差让内容超出 1px 而出现滚动条
  return Math.max(MIN_SCALE, Math.floor(scale * 100) / 100);
}

/** 按当前窗口大小缩放页面，并随窗口尺寸变化更新 */
export function fitToWindow(design: Size): void {
  const apply = () => {
    const scale = fitScale({ width: window.innerWidth, height: window.innerHeight }, design);
    document.documentElement.style.zoom = scale === 1 ? "" : String(scale);
  };
  window.addEventListener("resize", apply);
  apply();
}
