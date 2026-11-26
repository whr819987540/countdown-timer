import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Phase = "idle" | "running" | "paused" | "finished";

/** 对应后端 `timer::Mode`：普通倒计时 / 20-20-20 护眼 */
export type Mode = "normal" | "eyeCare";

/** 对应后端 `timer::Stage`：普通倒计时与护眼专注都是 work，远眺休息是 rest */
export type Stage = "work" | "rest";

/** 对应后端 `timer::Snapshot` */
export interface Snapshot {
  mode: Mode;
  stage: Stage;
  phase: Phase;
  /** 当前段的总时长 */
  durationMs: number;
  remainingMs: number;
  rounds: number;
}

/** 对应后端 `commands::SettingsView` */
export interface Settings {
  durationMs: number;
  sound: boolean;
  closeToTray: boolean;
  autoUpdate: boolean;
  autostart: boolean;
}

export type SettingsPatch = Partial<
  Pick<Settings, "sound" | "closeToTray" | "autoUpdate" | "autostart">
>;

/** 对应后端 `updater::UpdateStatus` */
export type UpdateStatus =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "upToDate" }
  | { state: "available"; version: string }
  /** 服务器未返回文件大小时 percent 为 null */
  | { state: "downloading"; version: string; percent: number | null }
  | { state: "installing"; version: string }
  | { state: "error"; message: string };

/** 与后端 `service::STATE_EVENT` 保持一致 */
const STATE_EVENT = "timer-state";
/** 与后端 `updater::UPDATE_EVENT` 保持一致 */
const UPDATE_EVENT = "update-state";

export const api = {
  getState: () => invoke<Snapshot>("get_state"),
  /** 省略时长时沿用上次的时长；护眼模式时长固定，不传 */
  start: (durationMs?: number) => invoke<void>("start", { durationMs }),
  /** 仅空闲时可切换 */
  setMode: (mode: Mode) => invoke<void>("set_mode", { mode }),
  pause: () => invoke<void>("pause"),
  resume: () => invoke<void>("resume"),
  /** 本段结束后进入下一段：重置 / 开始休息 / 开始下一轮 */
  advance: () => invoke<void>("advance"),
  stop: () => invoke<void>("stop"),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: SettingsPatch) => invoke<Settings>("update_settings", { patch }),
  quit: () => invoke<void>("quit_app"),
  onState: (handler: (snapshot: Snapshot) => void): Promise<UnlistenFn> =>
    listen<Snapshot>(STATE_EVENT, (event) => handler(event.payload)),
  getUpdateStatus: () => invoke<UpdateStatus>("get_update_status"),
  /** 结果通过 update-state 事件下发 */
  checkUpdate: () => invoke<void>("check_update"),
  /** 成功时程序退出并由安装程序重启，Promise 不会完成 */
  installUpdate: () => invoke<void>("install_update"),
  onUpdate: (handler: (status: UpdateStatus) => void): Promise<UnlistenFn> =>
    listen<UpdateStatus>(UPDATE_EVENT, (event) => handler(event.payload)),
};
