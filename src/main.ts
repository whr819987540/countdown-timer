import "./styles/main.css";
import { getVersion } from "@tauri-apps/api/app";
import { api, type Phase, type Settings, type Snapshot } from "./lib/api.ts";
import {
  FIELD_MAX,
  formatClock,
  formatEndTime,
  fromHms,
  type Hms,
  pad2,
  parseField,
  remainingAfter,
  stepField,
  toHms,
} from "./lib/time.ts";

function byId<T extends Element = HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`缺少元素 #${id}`);
  return element as unknown as T;
}

const ui = {
  app: byId("app"),
  status: byId("status"),
  editor: byId("time-editor"),
  display: byId("time-display"),
  meta: byId("meta"),
  ring: byId<SVGCircleElement>("ring-progress"),
  presets: byId("presets"),
  error: byId("duration-error"),
  primary: byId<HTMLButtonElement>("primary"),
  primaryIcon: byId<SVGUseElement>("primary-icon"),
  primaryLabel: byId("primary-label"),
  secondary: byId<HTMLButtonElement>("secondary"),
  trayHint: byId("tray-hint"),
  settings: byId("settings"),
  openSettings: byId<HTMLButtonElement>("open-settings"),
  closeSettings: byId<HTMLButtonElement>("close-settings"),
  optSound: byId<HTMLInputElement>("opt-sound"),
  optTray: byId<HTMLInputElement>("opt-tray"),
  optAutostart: byId<HTMLInputElement>("opt-autostart"),
  settingsError: byId("settings-error"),
  quit: byId<HTMLButtonElement>("quit"),
  version: byId("version"),
};

const fields: Record<keyof Hms, HTMLInputElement> = {
  h: byId("field-h"),
  m: byId("field-m"),
  s: byId("field-s"),
};

const STATUS_TEXT: Record<Phase, string> = {
  idle: "设置时长",
  running: "倒计时中",
  paused: "已暂停",
  finished: "时间到",
};

const PRIMARY_ACTION: Record<Phase, { label: string; icon: string }> = {
  idle: { label: "开始", icon: "#i-play" },
  running: { label: "暂停", icon: "#i-pause" },
  paused: { label: "继续", icon: "#i-play" },
  finished: { label: "重置", icon: "#i-restart" },
};

const HOUR_MS = 3_600_000;
/** 运行时界面刷新间隔，与 CSS 中 --duration-tick 一致，进度环过渡因此连贯 */
const RENDER_INTERVAL_MS = 250;

let snapshot: Snapshot = { phase: "idle", durationMs: 0, remainingMs: 0, rounds: 0 };
let receivedAt = performance.now();
let renderTimer: number | undefined;
let busy = false;

// ---------- 状态渲染 ----------

function applySnapshot(next: Snapshot): void {
  const previous = snapshot.phase;
  snapshot = next;
  receivedAt = performance.now();
  if (next.phase === "idle" && previous !== "idle") {
    // 停止后把编辑器恢复为刚才使用的时长，方便直接再次开始
    writeEditor(next.durationMs);
  }
  renderPhase();
  renderClock();

  window.clearInterval(renderTimer);
  renderTimer =
    next.phase === "running" ? window.setInterval(renderClock, RENDER_INTERVAL_MS) : undefined;
}

function currentRemaining(): number {
  return snapshot.phase === "running"
    ? remainingAfter(snapshot.remainingMs, performance.now() - receivedAt)
    : snapshot.remainingMs;
}

function renderPhase(): void {
  const { phase } = snapshot;
  const idle = phase === "idle";
  ui.app.dataset.phase = phase;
  ui.status.textContent = STATUS_TEXT[phase];
  ui.editor.hidden = !idle;
  ui.display.hidden = idle;
  ui.presets.inert = !idle;
  ui.secondary.hidden = idle;
  ui.primaryLabel.textContent = PRIMARY_ACTION[phase].label;
  ui.primaryIcon.setAttribute("href", PRIMARY_ACTION[phase].icon);
}

function renderClock(): void {
  const { phase, durationMs, rounds } = snapshot;
  if (phase === "idle") {
    ui.ring.style.strokeDashoffset = "0";
    ui.meta.textContent = "";
    return;
  }
  const remaining = currentRemaining();
  const withHours = durationMs >= HOUR_MS;
  ui.display.textContent = formatClock(remaining, withHours);
  ui.display.classList.toggle("is-long", withHours);
  const fraction = durationMs > 0 ? remaining / durationMs : 0;
  ui.ring.style.strokeDashoffset = String(100 * (1 - fraction));
  ui.meta.textContent = metaText(phase, remaining, rounds);
}

function metaText(phase: Phase, remaining: number, rounds: number): string {
  switch (phase) {
    case "running":
      return `第 ${rounds + 1} 轮 · ${formatEndTime(new Date(), remaining)} 结束`;
    case "paused":
      return `第 ${rounds + 1} 轮 · 共 ${formatClock(snapshot.durationMs)}`;
    case "finished":
      return `已完成 ${rounds} 轮`;
    default:
      return "";
  }
}

// ---------- 时长编辑 ----------

function readEditor(): number {
  return fromHms({
    h: parseField(fields.h.value, FIELD_MAX.h),
    m: parseField(fields.m.value, FIELD_MAX.m),
    s: parseField(fields.s.value, FIELD_MAX.s),
  });
}

function writeEditor(ms: number): void {
  const hms = toHms(ms);
  for (const unit of Object.keys(fields) as (keyof Hms)[]) {
    fields[unit].value = pad2(hms[unit]);
  }
  syncPresets();
}

function syncPresets(): void {
  const duration = readEditor();
  for (const chip of ui.presets.querySelectorAll<HTMLButtonElement>(".chip")) {
    const pressed = Number(chip.dataset.minutes) * 60_000 === duration;
    chip.setAttribute("aria-pressed", String(pressed));
  }
}

function setField(unit: keyof Hms, value: number): void {
  fields[unit].value = pad2(value);
  showError(ui.error, "");
  syncPresets();
}

for (const unit of Object.keys(fields) as (keyof Hms)[]) {
  const input = fields[unit];
  const max = FIELD_MAX[unit];
  input.addEventListener("focus", () => input.select());
  // 连续输入数字时保留最后两位，像微波炉面板一样"滚动"录入
  input.addEventListener("input", () => setField(unit, parseField(input.value, max)));
  input.addEventListener("keydown", (event) => {
    if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      event.preventDefault();
      const delta = (event.key === "ArrowUp" ? 1 : -1) * (event.shiftKey ? 10 : 1);
      setField(unit, stepField(parseField(input.value, max), delta, max));
      input.select();
    } else if (event.key === "Enter") {
      event.preventDefault();
      void onPrimary();
    }
  });
  input.addEventListener(
    "wheel",
    (event) => {
      if (snapshot.phase !== "idle") return;
      event.preventDefault();
      setField(unit, stepField(parseField(input.value, max), event.deltaY < 0 ? 1 : -1, max));
    },
    { passive: false },
  );
}

ui.presets.addEventListener("click", (event) => {
  const chip = (event.target as Element).closest<HTMLButtonElement>(".chip");
  if (!chip) return;
  writeEditor(Number(chip.dataset.minutes) * 60_000);
  showError(ui.error, "");
});

// ---------- 计时操作 ----------

function showError(target: HTMLElement, message: string): void {
  target.textContent = message;
}

async function run(action: () => Promise<void>): Promise<void> {
  if (busy) return;
  busy = true;
  ui.app.setAttribute("aria-busy", "true");
  try {
    await action();
    showError(ui.error, "");
  } catch (error) {
    showError(ui.error, String(error));
  } finally {
    busy = false;
    ui.app.removeAttribute("aria-busy");
  }
}

async function onPrimary(): Promise<void> {
  switch (snapshot.phase) {
    case "idle": {
      const duration = readEditor();
      if (duration < 1000) {
        showError(ui.error, "请先设置大于 0 的时长");
        fields.m.focus();
        return;
      }
      await run(() => api.start(duration));
      break;
    }
    case "running":
      await run(api.pause);
      break;
    case "paused":
      await run(api.resume);
      break;
    case "finished":
      await run(api.restart);
      break;
  }
}

ui.primary.addEventListener("click", () => void onPrimary());
ui.secondary.addEventListener("click", () => void run(api.stop));

// ---------- 设置 ----------

function applySettings(settings: Settings): void {
  ui.optSound.checked = settings.sound;
  ui.optTray.checked = settings.closeToTray;
  ui.optAutostart.checked = settings.autostart;
  ui.trayHint.textContent = settings.closeToTray
    ? "关闭窗口后仍会在托盘中继续计时"
    : "关闭窗口将退出程序";
}

function setSettingsOpen(open: boolean): void {
  ui.settings.dataset.open = String(open);
  ui.settings.inert = !open;
  ui.app.inert = open;
  ui.openSettings.setAttribute("aria-expanded", String(open));
  if (open) {
    showError(ui.settingsError, "");
    // 开机自启动可能在系统设置里被改过，每次打开都重新读取
    api.getSettings().then(applySettings, () => undefined);
    ui.closeSettings.focus();
  } else {
    ui.openSettings.focus();
  }
}

function bindSwitch(input: HTMLInputElement, key: "sound" | "closeToTray" | "autostart"): void {
  input.addEventListener("change", async () => {
    showError(ui.settingsError, "");
    try {
      applySettings(await api.updateSettings({ [key]: input.checked }));
    } catch (error) {
      input.checked = !input.checked;
      showError(ui.settingsError, String(error));
    }
  });
}

bindSwitch(ui.optSound, "sound");
bindSwitch(ui.optTray, "closeToTray");
bindSwitch(ui.optAutostart, "autostart");

ui.openSettings.addEventListener("click", () => setSettingsOpen(true));
ui.closeSettings.addEventListener("click", () => setSettingsOpen(false));
ui.quit.addEventListener("click", () => void api.quit());

// ---------- 键盘 ----------

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && ui.settings.dataset.open === "true") {
    setSettingsOpen(false);
  } else if (event.key === " " && event.target === document.body) {
    // 焦点不在任何控件上时，空格键执行主操作（开始 / 暂停 / 继续 / 重置）
    event.preventDefault();
    void onPrimary();
  }
});

// ---------- 启动 ----------

async function init(): Promise<void> {
  await api.onState(applySnapshot);
  const [state, settings] = await Promise.all([api.getState(), api.getSettings()]);
  applySettings(settings);
  writeEditor(state.phase === "idle" ? settings.durationMs : state.durationMs);
  applySnapshot(state);
  getVersion().then(
    (version) => {
      ui.version.textContent = `版本 ${version}`;
    },
    () => undefined,
  );
}

init().catch((error) => showError(ui.error, `初始化失败：${String(error)}`));
