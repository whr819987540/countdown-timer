import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  describeDuration,
  formatClock,
  formatEndTime,
  fromHms,
  MAX_DURATION_MS,
  parseField,
  remainingAfter,
  stepField,
  toHms,
} from "./time.ts";

describe("toHms / fromHms", () => {
  it("向上取整到秒", () => {
    assert.deepEqual(toHms(0), { h: 0, m: 0, s: 0 });
    assert.deepEqual(toHms(1), { h: 0, m: 0, s: 1 });
    assert.deepEqual(toHms(59_001), { h: 0, m: 1, s: 0 });
  });

  it("可以互相转换", () => {
    assert.equal(fromHms({ h: 1, m: 2, s: 3 }), 3_723_000);
    assert.deepEqual(toHms(3_723_000), { h: 1, m: 2, s: 3 });
    assert.deepEqual(toHms(MAX_DURATION_MS), { h: 99, m: 59, s: 59 });
  });
});

describe("formatClock", () => {
  it("不足一小时显示 MM:SS", () => {
    assert.equal(formatClock(25 * 60_000), "25:00");
    assert.equal(formatClock(500), "00:01");
  });

  it("超过一小时或强制时显示 HH:MM:SS", () => {
    assert.equal(formatClock(3_600_000), "01:00:00");
    assert.equal(formatClock(90_000, true), "00:01:30");
  });
});

describe("describeDuration", () => {
  it("省略为 0 的单位", () => {
    assert.equal(describeDuration(3_900_000), "1 小时 5 分钟");
    assert.equal(describeDuration(30_000), "30 秒");
    assert.equal(describeDuration(0), "0 秒");
  });
});

describe("parseField", () => {
  it("过滤非数字并限制范围", () => {
    assert.equal(parseField("", 59), 0);
    assert.equal(parseField("7a", 59), 7);
    assert.equal(parseField("75", 59), 59);
    assert.equal(parseField("123", 99), 23);
  });
});

describe("stepField", () => {
  it("超出范围时循环", () => {
    assert.equal(stepField(59, 1, 59), 0);
    assert.equal(stepField(0, -1, 59), 59);
    assert.equal(stepField(10, 5, 99), 15);
  });
});

describe("remainingAfter", () => {
  it("不会小于 0", () => {
    assert.equal(remainingAfter(1000, 400), 600);
    assert.equal(remainingAfter(1000, 5000), 0);
    assert.equal(remainingAfter(1000, -5), 1000);
  });
});

describe("formatEndTime", () => {
  it("计算结束时刻", () => {
    const from = new Date(2026, 0, 1, 9, 58, 30);
    assert.equal(formatEndTime(from, 90_000), "10:00");
  });
});
