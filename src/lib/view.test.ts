import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { Snapshot } from "./api.ts";
import { alertView, metaText, primaryAction, statusText } from "./view.ts";

const MIN = 60_000;

function snap(partial: Partial<Snapshot>): Snapshot {
  return {
    mode: "normal",
    stage: "work",
    phase: "idle",
    durationMs: 25 * MIN,
    remainingMs: 25 * MIN,
    rounds: 0,
    ...partial,
  };
}

describe("statusText", () => {
  it("普通模式按阶段显示", () => {
    assert.equal(statusText(snap({ phase: "idle" })), "设置时长");
    assert.equal(statusText(snap({ phase: "finished" })), "时间到");
  });

  it("护眼模式区分专注与休息", () => {
    const eye = { mode: "eyeCare" } as const;
    assert.equal(statusText(snap({ ...eye, phase: "idle" })), "准备专注");
    assert.equal(statusText(snap({ ...eye, phase: "running" })), "专注中");
    assert.equal(statusText(snap({ ...eye, phase: "finished" })), "该休息眼睛了");
    assert.equal(statusText(snap({ ...eye, stage: "rest", phase: "running" })), "远眺休息中");
    assert.equal(statusText(snap({ ...eye, stage: "rest", phase: "finished" })), "休息结束");
  });
});

describe("primaryAction", () => {
  it("结束后的操作随模式和阶段变化", () => {
    assert.equal(primaryAction(snap({ phase: "finished" })).label, "重置");
    assert.deepEqual(primaryAction(snap({ mode: "eyeCare", phase: "finished" })), {
      label: "开始休息",
      icon: "#i-eye",
    });
    assert.equal(
      primaryAction(snap({ mode: "eyeCare", stage: "rest", phase: "finished" })).label,
      "下一轮",
    );
  });

  it("计时中的操作与模式无关", () => {
    assert.equal(primaryAction(snap({ mode: "eyeCare", phase: "running" })).label, "暂停");
    assert.equal(primaryAction(snap({ mode: "eyeCare", phase: "paused" })).label, "继续");
  });
});

describe("metaText", () => {
  const now = new Date(2026, 0, 1, 14, 0, 0);

  it("普通模式显示轮次与结束时刻", () => {
    assert.equal(metaText(snap({ phase: "running" }), 5 * MIN, now), "第 1 轮 · 14:05 结束");
    assert.equal(
      metaText(snap({ phase: "paused", rounds: 1 }), 5 * MIN, now),
      "第 2 轮 · 共 25:00",
    );
    assert.equal(metaText(snap({ phase: "idle" }), 25 * MIN, now), "");
  });

  it("护眼模式专注时显示休息时刻，休息时显示提示", () => {
    const eye = { mode: "eyeCare", durationMs: 20 * MIN } as const;
    assert.equal(metaText(snap({ ...eye, phase: "idle" }), 20 * MIN, now), "每 20 分钟远眺 20 秒");
    assert.equal(
      metaText(snap({ ...eye, phase: "running" }), 20 * MIN, now),
      "第 1 轮 · 14:20 休息",
    );
    assert.equal(
      metaText(snap({ ...eye, stage: "rest", phase: "running", rounds: 1 }), 15_000, now),
      "看向 6 米（20 英尺）外的物体",
    );
    assert.equal(
      metaText(snap({ ...eye, stage: "rest", phase: "finished", rounds: 1 }), 0, now),
      "已完成 1 轮",
    );
  });
});

describe("alertView", () => {
  it("普通模式：时间到，可重置", () => {
    const view = alertView(snap({ phase: "finished", rounds: 2 }));
    assert.equal(view.kind, "alarm");
    assert.equal(view.desc, "25:00 的倒计时已结束，已完成 2 轮");
    assert.equal(view.descLabel, "25 分钟的倒计时已结束，已完成 2 轮");
    assert.equal(view.primary?.label, "重置");
    assert.equal(view.countdown, false);
  });

  it("护眼模式：专注结束 → 休息中 → 休息结束", () => {
    const eye = { mode: "eyeCare", rounds: 1 } as const;
    const work = alertView(snap({ ...eye, phase: "finished" }));
    assert.equal(work.title, "该休息眼睛了");
    assert.equal(work.primary?.label, "开始休息");

    const resting = alertView(snap({ ...eye, stage: "rest", phase: "running" }));
    assert.equal(resting.kind, "rest");
    assert.equal(resting.primary, undefined);
    assert.equal(resting.countdown, true);
    assert.equal(alertView(snap({ ...eye, stage: "rest", phase: "paused" })).title, "休息已暂停");

    const done = alertView(snap({ ...eye, stage: "rest", phase: "finished" }));
    assert.equal(done.kind, "alarm");
    assert.equal(done.title, "休息结束");
    assert.equal(done.primary?.label, "下一轮");
  });
});
