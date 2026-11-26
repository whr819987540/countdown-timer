import type { Phase, Settings, Snapshot } from "./api.ts";
import { describeDuration, formatClock, formatEndTime } from "./time.ts";

/** 快照中决定界面形态的部分 */
export type View = Pick<Snapshot, "mode" | "stage" | "phase">;

export interface Action {
  label: string;
  /** sprite 中的图标 id，例如 `#i-play` */
  icon: string;
}

/** 20-20-20 法则的说明，主窗口与提醒弹窗共用 */
export const EYE_CARE_TIP = "看向 6 米（20 英尺）外的物体";

/** 表盘顶部的状态文字 */
export function statusText({ mode, stage, phase }: View): string {
  const table: Record<Phase, string> =
    mode === "normal"
      ? { idle: "设置时长", running: "倒计时中", paused: "已暂停", finished: "时间到" }
      : stage === "work"
        ? { idle: "准备专注", running: "专注中", paused: "已暂停", finished: "该休息眼睛了" }
        : { idle: "准备专注", running: "远眺休息中", paused: "休息已暂停", finished: "休息结束" };
  return table[phase];
}

/** 主按钮（与托盘菜单的主操作一致） */
export function primaryAction({ mode, stage, phase }: View): Action {
  switch (phase) {
    case "idle":
      return { label: "开始", icon: "#i-play" };
    case "running":
      return { label: "暂停", icon: "#i-pause" };
    case "paused":
      return { label: "继续", icon: "#i-play" };
    case "finished":
      if (mode === "normal") return { label: "重置", icon: "#i-restart" };
      return stage === "work"
        ? { label: "开始休息", icon: "#i-eye" }
        : { label: "下一轮", icon: "#i-play" };
  }
}

/** 表盘中剩余时间下方的说明：轮次、预计结束时刻等 */
export function metaText(snapshot: Snapshot, remainingMs: number, now: Date): string {
  const { mode, stage, phase, durationMs, rounds } = snapshot;
  if (phase === "finished") return `已完成 ${rounds} 轮`;
  if (mode === "normal") {
    if (phase === "running") return `第 ${rounds + 1} 轮 · ${formatEndTime(now, remainingMs)} 结束`;
    if (phase === "paused") return `第 ${rounds + 1} 轮 · 共 ${formatClock(durationMs)}`;
    return "";
  }
  if (phase === "idle") return "每 20 分钟远眺 20 秒";
  if (stage === "rest") return EYE_CARE_TIP;
  if (phase === "running") return `第 ${rounds + 1} 轮 · ${formatEndTime(now, remainingMs)} 休息`;
  return `第 ${rounds + 1} 轮 · 共 ${formatClock(durationMs)}`;
}

/** 提醒弹窗的内容 */
export interface AlertView {
  /** alarm：时间到（铃铛、警示色）；rest：远眺休息中（眼睛、休息色） */
  kind: "alarm" | "rest";
  title: string;
  desc: string;
  /** 给屏幕阅读器的描述（时长读作"25 分钟"而不是"25:00"） */
  descLabel: string;
  /** 省略时隐藏主按钮，只保留"停止" */
  primary?: Action;
  /** 是否显示剩余时间 */
  countdown: boolean;
  hint: string;
}

export function alertView(snapshot: Snapshot): AlertView {
  const { mode, stage, phase, durationMs, rounds } = snapshot;
  if (mode === "normal") {
    return {
      kind: "alarm",
      title: "时间到！",
      desc: `${formatClock(durationMs)} 的倒计时已结束，已完成 ${rounds} 轮`,
      descLabel: `${describeDuration(durationMs)}的倒计时已结束，已完成 ${rounds} 轮`,
      primary: primaryAction({ mode, stage, phase: "finished" }),
      countdown: false,
      hint: "重置后将以相同时长重新开始倒计时 · Esc 停止",
    };
  }
  if (stage === "rest" && phase === "finished") {
    const desc = `已完成 ${rounds} 轮，准备好后开始下一个 20 分钟`;
    return {
      kind: "alarm",
      title: "休息结束",
      desc,
      descLabel: desc,
      primary: primaryAction({ mode, stage, phase }),
      countdown: false,
      hint: "Enter 开始下一轮 · Esc 停止",
    };
  }
  if (stage === "rest") {
    const desc = `${EYE_CARE_TIP}，放松眼睛`;
    return {
      kind: "rest",
      title: phase === "paused" ? "休息已暂停" : "远眺休息中",
      desc,
      descLabel: desc,
      countdown: true,
      hint:
        phase === "paused"
          ? "可在主窗口或托盘中继续 · Esc 停止"
          : "休息结束时会再次提醒 · Esc 停止",
    };
  }
  const desc = "已专注 20 分钟，请看向 6 米外的物体 20 秒";
  return {
    kind: "alarm",
    title: "该休息眼睛了",
    desc,
    descLabel: desc,
    primary: primaryAction({ mode, stage, phase: "finished" }),
    countdown: false,
    hint: "Enter 开始 20 秒休息 · Esc 停止",
  };
}

/** 粗略的剩余时间：悬浮窗只在鼠标悬停时显示，按分钟描述，避免盯着跳动的秒数 */
export function roughRemaining(ms: number): string {
  if (ms < 60_000) return `剩余 ${Math.max(0, Math.ceil(ms / 1000))} 秒`;
  const minutes = Math.ceil(ms / 60_000);
  if (minutes < 60) return `剩余约 ${minutes} 分钟`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `剩余约 ${hours} 小时` : `剩余约 ${hours} 小时 ${rest} 分钟`;
}

/** 悬浮窗的悬停提示与读屏名称 */
export function miniLabel(snapshot: Snapshot, remainingMs: number): string {
  const status = statusText(snapshot);
  return snapshot.phase === "finished"
    ? `${status} · 点击展开`
    : `${status} · ${roughRemaining(remainingMs)} · 点击展开`;
}

/**
 * 主窗口底部的提示：关闭 / 最小化窗口会怎样。
 * 计时中且开启了悬浮窗时，最小化（以及关闭到托盘）改为缩回悬浮窗，与后端 `ui::shrink_main_later` 一致。
 */
export function closeHint(
  { closeToTray, miniOnStart }: Pick<Settings, "closeToTray" | "miniOnStart">,
  phase: Phase,
): string {
  const toMini = miniOnStart && phase !== "idle";
  if (!closeToTray) {
    return toMini ? "最小化窗口会缩为悬浮窗，关闭窗口将退出程序" : "关闭窗口将退出程序";
  }
  return toMini ? "关闭或最小化窗口会缩为悬浮窗，计时继续" : "关闭窗口后仍会在托盘中继续计时";
}
