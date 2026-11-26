import "./styles/alert.css";
import { api, type Snapshot } from "./lib/api.ts";
import { fitToWindow } from "./lib/fit.ts";
import { formatClock, remainingAfter } from "./lib/time.ts";
import { alertView } from "./lib/view.ts";

function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`缺少元素 #${id}`);
  return element as T;
}

const root = byId("alert");
const icon = document.getElementById("alert-icon") as unknown as SVGUseElement;
const title = byId("alert-title");
const timer = byId("alert-timer");
const desc = byId("alert-desc");
const hint = byId("alert-hint");
const primary = byId<HTMLButtonElement>("primary");
const primaryIcon = document.getElementById("primary-icon") as unknown as SVGUseElement;
const primaryLabel = byId("primary-label");
const stop = byId<HTMLButtonElement>("stop");

// 与后端 ui::show_alert 中的窗口尺寸一致
fitToWindow({ width: 360, height: 320 });

/** 与主窗口一致的刷新间隔 */
const RENDER_INTERVAL_MS = 250;

let snapshot: Snapshot | undefined;
let receivedAt = performance.now();
let countdownTimer: number | undefined;
/** 当前显示的提醒标题，用来识别提醒内容的切换 */
let shownTitle: string | undefined;

/** 只在内容变化时写入，避免 aria-live 区域重复播报 */
function setText(element: HTMLElement, text: string): void {
  if (element.textContent !== text) element.textContent = text;
}

function renderCountdown(): void {
  if (!snapshot) return;
  const remaining =
    snapshot.phase === "running"
      ? remainingAfter(snapshot.remainingMs, performance.now() - receivedAt)
      : snapshot.remainingMs;
  setText(timer, formatClock(remaining));
}

/**
 * 普通模式下弹窗只显示"时间到"，用户操作后由后端销毁；
 * 护眼模式下弹窗会依次显示"该休息了 → 远眺休息倒计时 → 休息结束"，直到进入下一轮专注或停止。
 */
function render(next: Snapshot): void {
  snapshot = next;
  receivedAt = performance.now();
  const view = alertView(next);

  root.dataset.kind = view.kind;
  root.dataset.phase = next.phase;
  icon.setAttribute("href", view.kind === "rest" ? "#i-eye" : "#i-bell");
  setText(title, view.title);
  setText(desc, view.desc);
  desc.setAttribute("aria-label", view.descLabel);
  setText(hint, view.hint);

  timer.hidden = !view.countdown;
  renderCountdown();
  window.clearInterval(countdownTimer);
  countdownTimer =
    view.countdown && next.phase === "running"
      ? window.setInterval(renderCountdown, RENDER_INTERVAL_MS)
      : undefined;

  primary.hidden = view.primary === undefined;
  if (view.primary) {
    setText(primaryLabel, view.primary.label);
    primaryIcon.setAttribute("href", view.primary.icon);
  }
  // 上一次操作已生效（或状态已被其他窗口改变），重新允许操作
  primary.disabled = false;
  stop.disabled = false;
  // 切换到新的提醒内容时，把焦点放到主按钮上，直接按 Enter 即可继续
  if (view.primary && view.title !== shownTitle) primary.focus();
  shownTitle = view.title;
}

/** 防止重复点击；成功后等待新的状态推送（或窗口被后端销毁） */
async function act(action: () => Promise<void>): Promise<void> {
  if (stop.disabled) return;
  primary.disabled = true;
  stop.disabled = true;
  try {
    await action();
  } catch {
    primary.disabled = false;
    stop.disabled = false;
  }
}

primary.addEventListener("click", () => void act(api.advance));
stop.addEventListener("click", () => void act(api.stop));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") void act(api.stop);
});

await api.onState((next) => {
  // 进入空闲或下一轮专注时后端会销毁本窗口，不必再渲染
  if (next.phase === "finished" || next.stage === "rest") render(next);
});
render(await api.getState());
