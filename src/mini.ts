import "./styles/mini.css";
import { api, type Snapshot } from "./lib/api.ts";
import { formatClock, remainingAfter } from "./lib/time.ts";
import { primaryAction, statusText } from "./lib/view.ts";

function byId<T extends Element = HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`缺少元素 #${id}`);
  return element as unknown as T;
}

const root = byId("mini");
const ring = byId<SVGCircleElement>("ring");
const time = byId("time");
const status = byId("status");
const primary = byId<HTMLButtonElement>("primary");
const primaryIcon = byId<SVGUseElement>("primary-icon");
const expand = byId<HTMLButtonElement>("expand");

const HOUR_MS = 3_600_000;
/** 与主窗口一致的刷新间隔 */
const RENDER_INTERVAL_MS = 250;

let snapshot: Snapshot | undefined;
let receivedAt = performance.now();
let renderTimer: number | undefined;
let busy = false;

/** 只在内容变化时写入，避免 aria-live 区域重复播报 */
function setText(element: Element, text: string): void {
  if (element.textContent !== text) element.textContent = text;
}

function renderClock(): void {
  if (!snapshot) return;
  const { phase, durationMs, remainingMs } = snapshot;
  const remaining =
    phase === "running" ? remainingAfter(remainingMs, performance.now() - receivedAt) : remainingMs;
  const withHours = durationMs >= HOUR_MS;
  setText(time, formatClock(remaining, withHours));
  time.classList.toggle("is-long", withHours);
  const fraction = durationMs > 0 ? remaining / durationMs : 0;
  ring.style.strokeDashoffset = String(100 * (1 - fraction));
}

function render(next: Snapshot): void {
  snapshot = next;
  receivedAt = performance.now();
  root.dataset.phase = next.phase;
  root.dataset.stage = next.stage;
  setText(status, statusText(next));
  const action = primaryAction(next);
  primary.setAttribute("aria-label", action.label);
  primary.title = action.label;
  primaryIcon.setAttribute("href", action.icon);
  renderClock();
  window.clearInterval(renderTimer);
  renderTimer =
    next.phase === "running" ? window.setInterval(renderClock, RENDER_INTERVAL_MS) : undefined;
}

/** 与主窗口的主按钮一致：暂停 / 继续 / 重置 / 开始休息 / 下一轮 */
async function onPrimary(): Promise<void> {
  if (busy || !snapshot) return;
  const action = { running: api.pause, paused: api.resume, finished: api.advance, idle: undefined }[
    snapshot.phase
  ];
  if (!action) return;
  busy = true;
  try {
    await action();
  } catch {
    // 状态已被其他窗口改变时忽略，界面以随后推送的状态为准
  } finally {
    busy = false;
  }
}

primary.addEventListener("click", () => void onPrimary());
expand.addEventListener("click", () => void api.showMain());

await api.onState(render);
render(await api.getState());
