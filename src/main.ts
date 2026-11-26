import "./styles/main.css";
import { getVersion } from "@tauri-apps/api/app";
import { api, type Mode, type Settings, type Snapshot, type UpdateStatus } from "./lib/api.ts";
import { fitToWindow } from "./lib/fit.ts";
import {
  FIELD_MAX,
  formatClock,
  fromHms,
  type Hms,
  pad2,
  parseField,
  remainingAfter,
  stepField,
  toHms,
} from "./lib/time.ts";
import { updateView } from "./lib/update.ts";
import { closeHint, metaText, primaryAction, statusText } from "./lib/view.ts";

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
  modeSwitch: byId("mode-switch"),
  presets: byId("presets"),
  error: byId("duration-error"),
  primary: byId<HTMLButtonElement>("primary"),
  primaryIcon: byId<SVGUseElement>("primary-icon"),
  primaryLabel: byId("primary-label"),
  secondary: byId<HTMLButtonElement>("secondary"),
  trayHint: byId("tray-hint"),
  updateBanner: byId<HTMLButtonElement>("update-banner"),
  updateBannerText: byId("update-banner-text"),
  settings: byId("settings"),
  openSettings: byId<HTMLButtonElement>("open-settings"),
  toMini: byId<HTMLButtonElement>("to-mini"),
  closeSettings: byId<HTMLButtonElement>("close-settings"),
  optSound: byId<HTMLInputElement>("opt-sound"),
  optTray: byId<HTMLInputElement>("opt-tray"),
  optAutostart: byId<HTMLInputElement>("opt-autostart"),
  optUpdate: byId<HTMLInputElement>("opt-update"),
  optMini: byId<HTMLInputElement>("opt-mini"),
  updateStatus: byId("update-status"),
  updateAction: byId<HTMLButtonElement>("update-action"),
  settingsError: byId("settings-error"),
  quit: byId<HTMLButtonElement>("quit"),
  version: byId("version"),
};

const fields: Record<keyof Hms, HTMLInputElement> = {
  h: byId("field-h"),
  m: byId("field-m"),
  s: byId("field-s"),
};

const modeRadios = [...ui.modeSwitch.querySelectorAll<HTMLInputElement>('input[name="mode"]')];

const HOUR_MS = 3_600_000;
/** 运行时界面刷新间隔，与 CSS 中 --duration-tick 一致，进度环过渡因此连贯 */
const RENDER_INTERVAL_MS = 250;

let snapshot: Snapshot = {
  mode: "normal",
  stage: "work",
  phase: "idle",
  durationMs: 0,
  remainingMs: 0,
  rounds: 0,
};
let receivedAt = performance.now();
let renderTimer: number | undefined;
let busy = false;
let updateStatus: UpdateStatus = { state: "idle" };
/** 决定底部提示内容的设置项（关闭 / 最小化窗口时的行为） */
let closeBehavior: Pick<Settings, "closeToTray" | "miniOnStart"> = {
  closeToTray: true,
  miniOnStart: true,
};

// ---------- 状态渲染 ----------

function applySnapshot(next: Snapshot): void {
  const previous = snapshot.phase;
  snapshot = next;
  receivedAt = performance.now();
  if (next.phase === "idle" && previous !== "idle" && next.mode === "normal") {
    // 停止后把编辑器恢复为刚才使用的时长，方便直接再次开始
    writeEditor(next.durationMs);
  }
  renderPhase();
  renderClock();
  renderUpdate();

  window.clearInterval(renderTimer);
  renderTimer =
    next.phase === "running" ? window.setInterval(renderClock, RENDER_INTERVAL_MS) : undefined;
}

function currentRemaining(): number {
  return snapshot.phase === "running"
    ? remainingAfter(snapshot.remainingMs, performance.now() - receivedAt)
    : snapshot.remainingMs;
}

/** 只在内容变化时写入，避免 aria-live 区域每秒重复播报 */
function setText(element: HTMLElement, text: string): void {
  if (element.textContent !== text) element.textContent = text;
}

/** 空闲的普通模式下显示时长编辑器，其余情况显示剩余时间 */
function isEditing(): boolean {
  return snapshot.phase === "idle" && snapshot.mode === "normal";
}

function renderPhase(): void {
  const { mode, stage, phase } = snapshot;
  const idle = phase === "idle";
  const editing = isEditing();
  ui.app.dataset.mode = mode;
  ui.app.dataset.stage = stage;
  ui.app.dataset.phase = phase;
  setText(ui.status, statusText(snapshot));
  ui.editor.hidden = !editing;
  ui.display.hidden = editing;
  ui.presets.inert = !editing;
  ui.secondary.hidden = idle;
  ui.toMini.hidden = idle;
  renderHint();
  for (const radio of modeRadios) {
    radio.checked = radio.value === mode;
    radio.disabled = !idle;
  }
  const action = primaryAction(snapshot);
  setText(ui.primaryLabel, action.label);
  ui.primaryIcon.setAttribute("href", action.icon);
}

function renderClock(): void {
  const { durationMs } = snapshot;
  if (isEditing()) {
    ui.ring.style.strokeDashoffset = "0";
    setText(ui.meta, "");
    return;
  }
  const remaining = currentRemaining();
  const withHours = durationMs >= HOUR_MS;
  setText(ui.display, formatClock(remaining, withHours));
  ui.display.classList.toggle("is-long", withHours);
  const fraction = durationMs > 0 ? remaining / durationMs : 0;
  ui.ring.style.strokeDashoffset = String(100 * (1 - fraction));
  setText(ui.meta, metaText(snapshot, remaining, new Date()));
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

/** 执行一个操作并显示错误；返回是否成功 */
async function run(action: () => Promise<void>): Promise<boolean> {
  if (busy) return false;
  busy = true;
  ui.app.setAttribute("aria-busy", "true");
  try {
    await action();
    showError(ui.error, "");
    return true;
  } catch (error) {
    showError(ui.error, String(error));
    return false;
  } finally {
    busy = false;
    ui.app.removeAttribute("aria-busy");
  }
}

async function onPrimary(): Promise<void> {
  switch (snapshot.phase) {
    case "idle": {
      if (snapshot.mode === "eyeCare") {
        await run(() => api.start());
        break;
      }
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
      await run(api.advance);
      break;
  }
}

ui.primary.addEventListener("click", () => void onPrimary());
ui.toMini.addEventListener("click", () => void api.showMini());
ui.secondary.addEventListener("click", () => void run(api.stop));

for (const radio of modeRadios) {
  radio.addEventListener("change", async () => {
    if (!radio.checked) return;
    // 失败时把选中项恢复为实际模式；成功时由随后的 timer-state 事件更新界面
    if (!(await run(() => api.setMode(radio.value as Mode)))) renderPhase();
  });
}

// ---------- 更新 ----------

function renderUpdate(): void {
  const view = updateView(updateStatus, snapshot.phase === "idle");
  ui.updateBanner.hidden = view.banner === undefined;
  ui.trayHint.hidden = view.banner !== undefined;
  if (view.banner) {
    setText(ui.updateBannerText, view.banner.text);
    ui.updateBanner.disabled = !view.banner.actionable;
  }
  setText(ui.updateStatus, view.status);
  setText(ui.updateAction, view.button.label);
  ui.updateAction.disabled = view.button.action === "none";
  ui.updateAction.dataset.action = view.button.action;
  // 有可安装的新版本时突出为主按钮
  const install = view.button.action === "install";
  ui.updateAction.classList.toggle("btn-primary", install);
  ui.updateAction.classList.toggle("btn-secondary", !install);
}

function applyUpdateStatus(status: UpdateStatus): void {
  updateStatus = status;
  renderUpdate();
}

/** 下载进度和结果通过 update-state 事件推送；成功时程序会退出并自动重启 */
async function installUpdate(errorTarget: HTMLElement): Promise<void> {
  showError(errorTarget, "");
  try {
    await api.installUpdate();
  } catch (error) {
    showError(errorTarget, String(error));
  }
}

ui.updateBanner.addEventListener("click", () => void installUpdate(ui.error));
ui.updateAction.addEventListener("click", () => {
  if (ui.updateAction.dataset.action === "install") {
    void installUpdate(ui.settingsError);
  } else if (ui.updateAction.dataset.action === "check") {
    showError(ui.settingsError, "");
    void api.checkUpdate();
  }
});

// ---------- 设置 ----------

function applySettings(settings: Settings): void {
  ui.optSound.checked = settings.sound;
  ui.optTray.checked = settings.closeToTray;
  ui.optAutostart.checked = settings.autostart;
  ui.optUpdate.checked = settings.autoUpdate;
  ui.optMini.checked = settings.miniOnStart;
  closeBehavior = settings;
  renderHint();
}

function renderHint(): void {
  setText(ui.trayHint, closeHint(closeBehavior, snapshot.phase));
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

function bindSwitch(
  input: HTMLInputElement,
  key: "sound" | "closeToTray" | "autostart" | "autoUpdate" | "miniOnStart",
): void {
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
bindSwitch(ui.optUpdate, "autoUpdate");
bindSwitch(ui.optMini, "miniOnStart");

ui.openSettings.addEventListener("click", () => setSettingsOpen(true));
ui.closeSettings.addEventListener("click", () => setSettingsOpen(false));
ui.quit.addEventListener("click", () => void api.quit());

// ---------- 键盘 ----------

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && ui.settings.dataset.open === "true") {
    setSettingsOpen(false);
  } else if (event.key === " " && event.target === document.body) {
    // 焦点不在任何控件上时，空格键执行主操作（开始 / 暂停 / 继续 / 重置 / 开始休息 / 下一轮）
    event.preventDefault();
    void onPrimary();
  }
});

// ---------- 启动 ----------

// 与 tauri.conf.json 中主窗口的尺寸一致
fitToWindow({ width: 380, height: 600 });

async function init(): Promise<void> {
  await Promise.all([api.onState(applySnapshot), api.onUpdate(applyUpdateStatus)]);
  const [state, settings, update] = await Promise.all([
    api.getState(),
    api.getSettings(),
    api.getUpdateStatus(),
  ]);
  updateStatus = update;
  applySettings(settings);
  // 普通模式开始计时时会记住时长，所以设置里的时长总是编辑器该显示的值
  writeEditor(settings.durationMs);
  applySnapshot(state);
  getVersion().then(
    (version) => {
      ui.version.textContent = `版本 ${version}`;
    },
    () => undefined,
  );
}

init().catch((error) => showError(ui.error, `初始化失败：${String(error)}`));
