import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Phase = "idle" | "running" | "paused" | "finished";

/** 对应后端 `timer::Snapshot` */
export interface Snapshot {
  phase: Phase;
  durationMs: number;
  remainingMs: number;
  rounds: number;
}

/** 对应后端 `commands::SettingsView` */
export interface Settings {
  durationMs: number;
  sound: boolean;
  closeToTray: boolean;
  autostart: boolean;
}

export type SettingsPatch = Partial<Pick<Settings, "sound" | "closeToTray" | "autostart">>;

/** 与后端 `service::STATE_EVENT` 保持一致 */
const STATE_EVENT = "timer-state";

export const api = {
  getState: () => invoke<Snapshot>("get_state"),
  start: (durationMs: number) => invoke<void>("start", { durationMs }),
  pause: () => invoke<void>("pause"),
  resume: () => invoke<void>("resume"),
  restart: () => invoke<void>("restart"),
  stop: () => invoke<void>("stop"),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: SettingsPatch) => invoke<Settings>("update_settings", { patch }),
  quit: () => invoke<void>("quit_app"),
  onState: (handler: (snapshot: Snapshot) => void): Promise<UnlistenFn> =>
    listen<Snapshot>(STATE_EVENT, (event) => handler(event.payload)),
};
