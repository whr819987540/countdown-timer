import "./styles/alert.css";
import { api, type Snapshot } from "./lib/api.ts";
import { describeDuration, formatClock } from "./lib/time.ts";

const desc = document.getElementById("alert-desc") as HTMLParagraphElement;
const restart = document.getElementById("restart") as HTMLButtonElement;
const stop = document.getElementById("stop") as HTMLButtonElement;

function render({ durationMs, rounds }: Snapshot): void {
  desc.textContent = `${formatClock(durationMs)} 的倒计时已结束，已完成 ${rounds} 轮`;
  desc.setAttribute(
    "aria-label",
    `${describeDuration(durationMs)}的倒计时已结束，已完成 ${rounds} 轮`,
  );
}

/** 操作完成后后端会销毁本窗口；这里只负责防止重复点击 */
async function act(action: () => Promise<void>): Promise<void> {
  if (restart.disabled) return;
  restart.disabled = true;
  stop.disabled = true;
  try {
    await action();
  } catch {
    restart.disabled = false;
    stop.disabled = false;
  }
}

restart.addEventListener("click", () => void act(api.restart));
stop.addEventListener("click", () => void act(api.stop));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") void act(api.stop);
});

await api.onState((snapshot) => {
  if (snapshot.phase === "finished") render(snapshot);
});
render(await api.getState());
restart.focus();
