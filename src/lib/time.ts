/** 与后端 `timer::MAX_DURATION` 保持一致：99:59:59 */
export const MAX_DURATION_MS = (99 * 3600 + 59 * 60 + 59) * 1000;

export interface Hms {
  h: number;
  m: number;
  s: number;
}

/** 每个输入框允许的最大值 */
export const FIELD_MAX: Record<keyof Hms, number> = { h: 99, m: 59, s: 59 };

/** 剩余毫秒向上取整到秒：只有真正归零时才显示 00:00 */
export function toHms(ms: number): Hms {
  const total = Math.ceil(Math.max(0, ms) / 1000);
  return { h: Math.floor(total / 3600), m: Math.floor(total / 60) % 60, s: total % 60 };
}

export function fromHms({ h, m, s }: Hms): number {
  return ((h * 60 + m) * 60 + s) * 1000;
}

export function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

/** 时长达到一小时显示 `HH:MM:SS`，否则显示 `MM:SS` */
export function formatClock(ms: number, withHours = false): string {
  const { h, m, s } = toHms(ms);
  return withHours || h > 0 ? `${pad2(h)}:${pad2(m)}:${pad2(s)}` : `${pad2(m)}:${pad2(s)}`;
}

/** 给屏幕阅读器的时长描述，例如 "1 小时 5 分钟" */
export function describeDuration(ms: number): string {
  const { h, m, s } = toHms(ms);
  const parts = [h && `${h} 小时`, m && `${m} 分钟`, s && `${s} 秒`].filter(Boolean);
  return parts.length > 0 ? parts.join(" ") : "0 秒";
}

/** 只保留数字并限制在 0..max 之间；空输入视为 0 */
export function parseField(raw: string, max: number): number {
  const digits = raw.replace(/\D/g, "").slice(-2);
  const value = digits === "" ? 0 : Number.parseInt(digits, 10);
  return Math.min(Math.max(value, 0), max);
}

/** 上下键 / 滚轮调整：超出范围时循环 */
export function stepField(value: number, delta: number, max: number): number {
  const span = max + 1;
  return (((value + delta) % span) + span) % span;
}

/** 根据收到快照后经过的时间推算当前剩余毫秒 */
export function remainingAfter(remainingMs: number, elapsedMs: number): number {
  return Math.max(0, remainingMs - Math.max(0, elapsedMs));
}

/** 预计结束时刻，格式 `HH:MM` */
export function formatEndTime(from: Date, remainingMs: number): string {
  const end = new Date(from.getTime() + Math.ceil(remainingMs / 1000) * 1000);
  return `${pad2(end.getHours())}:${pad2(end.getMinutes())}`;
}
