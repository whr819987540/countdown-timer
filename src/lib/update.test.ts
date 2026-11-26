import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { updateView } from "./update.ts";

describe("updateView", () => {
  it("没有新版本时不显示主窗口提示，设置中可手动检查", () => {
    for (const state of ["idle", "upToDate"] as const) {
      const view = updateView({ state }, true);
      assert.equal(view.banner, undefined);
      assert.deepEqual(view.button, { label: "检查更新", action: "check" });
    }
    assert.equal(updateView({ state: "upToDate" }, true).status, "已是最新版本");
    assert.equal(updateView({ state: "checking" }, true).button.action, "none");
    assert.equal(updateView({ state: "error", message: "网络错误" }, true).status, "网络错误");
  });

  it("发现新版本：空闲时可安装，计时中只提示", () => {
    const idle = updateView({ state: "available", version: "0.1.9" }, true);
    assert.deepEqual(idle.banner, { text: "发现新版本 0.1.9，点击更新", actionable: true });
    assert.equal(idle.button.action, "install");

    const busy = updateView({ state: "available", version: "0.1.9" }, false);
    assert.deepEqual(busy.banner, {
      text: "发现新版本 0.1.9，停止计时后可更新",
      actionable: false,
    });
    assert.equal(busy.button.action, "none");
  });

  it("下载进度只出现在提示和按钮上，不进入读屏播报的状态文字", () => {
    const view = updateView({ state: "downloading", version: "0.1.9", percent: 42 }, true);
    assert.equal(view.banner?.text, "正在下载更新 42%");
    assert.equal(view.button.label, "下载中 42%");
    assert.equal(view.status, "正在下载 0.1.9…");

    const unknown = updateView({ state: "downloading", version: "0.1.9", percent: null }, true);
    assert.equal(unknown.banner?.text, "正在下载更新");
    assert.equal(unknown.button.label, "下载中…");
  });

  it("安装中提示即将重启", () => {
    const view = updateView({ state: "installing", version: "0.1.9" }, true);
    assert.equal(view.banner?.text, "正在安装 0.1.9，程序将自动重启");
    assert.equal(view.button.action, "none");
  });
});
