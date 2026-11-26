import "./styles/mini.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Snapshot } from "./lib/api.ts";
import { remainingAfter } from "./lib/time.ts";
import { miniLabel } from "./lib/view.ts";

function byId<T extends Element = HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`缺少元素 #${id}`);
  return element as unknown as T;
}

const button = byId<HTMLButtonElement>("mini");
const ring = byId<SVGCircleElement>("ring");
const iconSvg = byId<SVGSVGElement>("mini-icon-svg");
const icon = byId<SVGUseElement>("mini-icon");

/** 圆环变化很慢，每秒更新一次即可（CSS 中配合 1 秒线性过渡，看起来是连续的） */
const RENDER_INTERVAL_MS = 1000;
/** 按下后移动超过这个距离（像素）才算拖动，否则视为单击 */
const DRAG_THRESHOLD = 3;

let snapshot: Snapshot | undefined;
let receivedAt = performance.now();
let renderTimer: number | undefined;

function currentRemaining(): number {
  if (!snapshot) return 0;
  return snapshot.phase === "running"
    ? remainingAfter(snapshot.remainingMs, performance.now() - receivedAt)
    : snapshot.remainingMs;
}

function renderRing(): void {
  if (!snapshot) return;
  const fraction = snapshot.durationMs > 0 ? currentRemaining() / snapshot.durationMs : 0;
  ring.style.strokeDashoffset = String(100 * (1 - fraction));
}

/** 提示文字只在状态变化和鼠标移入时更新，不随秒数跳动 */
function renderLabel(): void {
  if (!snapshot) return;
  const label = miniLabel(snapshot, currentRemaining());
  button.title = label;
  button.setAttribute("aria-label", label);
}

/** 圆环中央的静态图标：状态不只靠颜色区分 */
function stateIcon({ phase, stage }: Snapshot): string | undefined {
  if (phase === "paused") return "#i-pause";
  if (phase === "finished") return "#i-bell";
  if (stage === "rest") return "#i-eye";
  return undefined;
}

function render(next: Snapshot): void {
  snapshot = next;
  receivedAt = performance.now();
  button.dataset.phase = next.phase;
  button.dataset.stage = next.stage;
  const href = stateIcon(next);
  iconSvg.toggleAttribute("hidden", href === undefined);
  if (href) icon.setAttribute("href", href);
  renderRing();
  renderLabel();
  window.clearInterval(renderTimer);
  renderTimer =
    next.phase === "running" ? window.setInterval(renderRing, RENDER_INTERVAL_MS) : undefined;
}

// 自己区分单击与拖动：拖动交给系统移动窗口，单击才展开主窗口
let pressedAt: { x: number; y: number } | undefined;
let dragged = false;

button.addEventListener("pointerdown", (event) => {
  if (event.button !== 0) return;
  pressedAt = { x: event.screenX, y: event.screenY };
  dragged = false;
});
button.addEventListener("pointermove", (event) => {
  if (!pressedAt) return;
  const distance = Math.hypot(event.screenX - pressedAt.x, event.screenY - pressedAt.y);
  if (distance > DRAG_THRESHOLD) {
    pressedAt = undefined;
    dragged = true;
    void getCurrentWindow().startDragging();
  }
});
button.addEventListener("pointerup", () => {
  pressedAt = undefined;
});
button.addEventListener("click", () => {
  if (dragged) {
    dragged = false;
    return;
  }
  void api.showMain();
});
button.addEventListener("pointerenter", renderLabel);
button.addEventListener("focus", renderLabel);
document.addEventListener("contextmenu", (event) => event.preventDefault());

await api.onState(render);
render(await api.getState());
