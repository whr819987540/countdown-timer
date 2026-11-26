import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { fitScale, MIN_SCALE } from "./fit.ts";

const DESIGN = { width: 380, height: 600 };

describe("fitScale", () => {
  it("空间足够时不缩放", () => {
    assert.equal(fitScale({ width: 380, height: 600 }, DESIGN), 1);
    assert.equal(fitScale({ width: 500, height: 900 }, DESIGN), 1);
  });

  it("空间不足时按较紧的一边等比缩小，并向下取整", () => {
    assert.equal(fitScale({ width: 380, height: 540 }, DESIGN), 0.9);
    // 537 / 600 = 0.895 → 0.89，确保缩放后不会超出
    assert.equal(fitScale({ width: 380, height: 537 }, DESIGN), 0.89);
    assert.equal(fitScale({ width: 342, height: 600 }, DESIGN), 0.9);
  });

  it("有下限，尺寸异常时不缩放", () => {
    assert.equal(fitScale({ width: 380, height: 300 }, DESIGN), MIN_SCALE);
    assert.equal(fitScale({ width: 0, height: 0 }, DESIGN), 1);
  });
});
