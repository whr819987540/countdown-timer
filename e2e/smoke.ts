/**
 * Windows 端到端冒烟测试（在 CI 的 e2e 任务中运行，见 .github/workflows/ci.yml）。
 *
 * 通过 msedgedriver（WebView2 的 WebDriver）驱动安装好的程序，像用户一样点击界面走一遍主流程：
 * 开始计时 → 悬浮窗 → 时间到弹窗 → 停止 → 悬浮窗展开 / 缩回（按钮、最小化、关闭）→ 20-20-20 模式 → 退出程序。
 * 点击经由页面脚本触发，与真实点击走同一条 IPC 路径（同步命令在主线程执行），
 * 因此能发现 0.1.6 那样"开始计时后主线程死锁"的问题：每一步都有超时，卡死时测试失败而不是一直挂起。
 * 关键状态的截图保存在 e2e/screenshots/，CI 中作为构建产物上传，可直接查看真实的 Windows 渲染。
 *
 * 用法（附加模式，CI 中使用）：
 *   1. 以环境变量 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 启动程序；
 *   2. 启动 msedgedriver --port=4444（版本须与 WebView2 运行时一致）；
 *   3. 以环境变量 APP_PID=<程序进程号> 运行 node e2e/smoke.ts 127.0.0.1:9222
 *      （最小化、关闭主窗口是原生窗口操作，WebDriver 做不到，借进程号找到主窗口发送窗口消息）
 * 不用 msedgedriver 自己启动程序：Tauri 给 WebView2 指定了自己的数据目录，驱动找不到 DevToolsActivePort 文件。
 */
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { setTimeout as sleep } from "node:timers/promises";
import { Session } from "./webdriver.ts";

type WindowName = "main" | "mini" | "alert";

const debuggerAddress = process.argv[2];
if (!debuggerAddress) {
  console.error("用法：node e2e/smoke.ts <WebView2 调试地址，如 127.0.0.1:9222>");
  process.exit(2);
}
const server = process.env.WEBDRIVER_URL ?? "http://127.0.0.1:4444";
const shotsDir = new URL("./screenshots/", import.meta.url);

/** 反复探测直到返回 true，超时则报错并带上最后一次的异常 */
async function waitFor(what: string, probe: () => Promise<boolean>, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError: unknown;
  while (Date.now() < deadline) {
    try {
      if (await probe()) return;
    } catch (error) {
      lastError = error;
    }
    await sleep(250);
  }
  const detail = lastError === undefined ? "" : `；最后一次错误：${String(lastError)}`;
  throw new Error(`等待超时（${timeoutMs / 1000} 秒）：${what}${detail}`);
}

const session = await Session.create(server, {
  browserName: "webview2",
  "ms:edgeChromium": true,
  "ms:edgeOptions": { debuggerAddress },
});

/** 按页面识别各窗口：主窗口 index.html、悬浮窗 mini.html、提醒弹窗 alert.html */
async function windows(): Promise<Map<WindowName, string>> {
  const found = new Map<WindowName, string>();
  for (const handle of await session.handles()) {
    await session.switchTo(handle);
    const path = await session.execute<string>("return location.pathname");
    found.set(path.includes("mini") ? "mini" : path.includes("alert") ? "alert" : "main", handle);
  }
  return found;
}

async function hasWindow(name: WindowName): Promise<boolean> {
  return (await windows()).has(name);
}

async function inWindow<T>(name: WindowName, action: () => Promise<T>): Promise<T> {
  const handle = (await windows()).get(name);
  if (!handle) throw new Error(`窗口不存在：${name}`);
  await session.switchTo(handle);
  return action();
}

/** 用页面脚本点击：窗口隐藏时也能操作，且与真实点击触发同样的命令调用 */
async function press(selector: string): Promise<void> {
  await session.execute(
    `const el = document.querySelector(arguments[0]);
     if (!el) throw new Error("找不到元素 " + arguments[0]);
     el.click();`,
    selector,
  );
}

function text(selector: string): Promise<string | null> {
  return session.execute(
    "return document.querySelector(arguments[0])?.textContent ?? null",
    selector,
  );
}

async function shot(name: string): Promise<void> {
  await mkdir(shotsDir, { recursive: true });
  await writeFile(new URL(`${name}.png`, shotsDir), await session.screenshot());
}

/**
 * 像用户点标题栏按钮一样最小化 / 关闭主窗口。
 * 找的是进程中唯一可见的顶层窗口，所以调用时悬浮窗与提醒弹窗必须都已关闭。
 */
function nativeMain(action: "minimize" | "close"): void {
  const pid = process.env.APP_PID ?? "";
  if (!/^\d+$/.test(pid)) throw new Error("缺少环境变量 APP_PID（程序进程号）");
  const call =
    action === "minimize"
      ? "[void][E2E.Native]::ShowWindow($h, 6)" // SW_MINIMIZE
      : "[void][E2E.Native]::PostMessageW($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)"; // WM_CLOSE
  const script = `
    Add-Type -Namespace E2E -Name Native -MemberDefinition '
      [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
      [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);'
    $h = (Get-Process -Id ${pid}).MainWindowHandle
    if ($h -eq [IntPtr]::Zero) { throw '找不到主窗口' }
    ${call}`;
  execFileSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", script], {
    stdio: "inherit",
    timeout: 30_000,
  });
}

async function step(name: string, action: () => Promise<void>): Promise<void> {
  const started = Date.now();
  try {
    await action();
  } catch (error) {
    console.error(`✗ ${name}`);
    throw error;
  }
  console.log(`✓ ${name}（${((Date.now() - started) / 1000).toFixed(1)} 秒）`);
}

let exited = false;
try {
  await step("主窗口加载完成", async () => {
    await waitFor(
      "主按钮显示「开始」",
      async () => (await inWindow("main", () => text("#primary-label"))) === "开始",
      30_000,
    );
    await shot("01-main-idle");
  });

  await step("开始 3 秒倒计时后缩为悬浮窗", async () => {
    await inWindow("main", async () => {
      await session.execute(
        `for (const [id, value] of [["field-h", "00"], ["field-m", "00"], ["field-s", "03"]]) {
           document.getElementById(id).value = value;
         }`,
      );
      await press("#primary");
    });
    await waitFor("悬浮窗出现", () => hasWindow("mini"));
    await inWindow("mini", async () => {
      // Windows 会把窗口撑宽到约 136 像素：徽章必须是贴在右侧、直径等于窗口高度（48px）的完整圆形，
      // 后端按窗口高度把多出的部分裁掉；两者不一致时圆环会缺一块（高缩放比例屏幕上出现过）
      const layout = await session.execute<Record<string, number>>(
        `const rect = document.getElementById("mini").getBoundingClientRect();
         return { top: rect.top, right: rect.right, width: rect.width, height: rect.height,
                  viewportWidth: innerWidth, viewportHeight: innerHeight, dpr: devicePixelRatio };`,
      );
      console.log(`  悬浮窗：${JSON.stringify(layout)}`);
      const near = (a: number | undefined, b: number | undefined) =>
        a !== undefined && b !== undefined && Math.abs(a - b) <= 1;
      const ok =
        near(layout.viewportHeight, 48) &&
        near(layout.width, layout.viewportHeight) &&
        near(layout.height, layout.viewportHeight) &&
        near(layout.top, 0) &&
        near(layout.right, layout.viewportWidth);
      if (!ok) throw new Error(`悬浮窗徽章位置或大小不对：${JSON.stringify(layout)}`);
      await shot("02-mini-running");
    });
  });

  await step("时间到弹出提醒", async () => {
    await waitFor("提醒弹窗出现", () => hasWindow("alert"), 15_000);
    await inWindow("alert", async () => {
      await waitFor(
        "弹窗显示「时间到！」",
        async () => (await text("#alert-title")) === "时间到！",
      );
      await shot("03-alert");
    });
    await inWindow("mini", () => shot("04-mini-finished"));
  });

  await step("在弹窗中停止：弹窗与悬浮窗关闭，主窗口回到空闲", async () => {
    await inWindow("alert", () => press("#stop"));
    await waitFor("弹窗与悬浮窗关闭", async () => {
      const open = await windows();
      return !open.has("alert") && !open.has("mini");
    });
    await inWindow("main", async () => {
      await waitFor("主按钮恢复为「开始」", async () => (await text("#primary-label")) === "开始");
      await shot("05-main-after-stop");
    });
  });

  /** 单击悬浮窗展开主窗口 */
  async function expandMini(): Promise<void> {
    await inWindow("mini", () => press("#mini"));
    await waitFor("单击后悬浮窗关闭", async () => !(await hasWindow("mini")));
  }

  await step("展开主窗口后，用按钮、最小化、关闭窗口都能缩回悬浮窗，然后停止", async () => {
    await inWindow("main", async () => {
      await press('.chip[data-minutes="1"]');
      await press("#primary");
    });
    await waitFor("悬浮窗出现", () => hasWindow("mini"));
    await expandMini();
    await inWindow("main", async () => {
      await waitFor("「缩为悬浮窗」按钮可见", () =>
        session.execute<boolean>(`return !document.getElementById("to-mini").hidden`),
      );
      const hint = await text("#tray-hint");
      if (hint !== "关闭或最小化窗口会缩为悬浮窗，计时继续") {
        throw new Error(`计时中的底部提示不对：${hint}`);
      }
      await shot("06-main-running");
      await press("#to-mini");
    });
    await waitFor("按钮缩回悬浮窗", () => hasWindow("mini"));

    await expandMini();
    nativeMain("minimize");
    await waitFor("最小化后缩回悬浮窗", () => hasWindow("mini"));

    await expandMini();
    nativeMain("close");
    await waitFor("关闭后缩回悬浮窗", () => hasWindow("mini"));

    // 关闭只是缩回悬浮窗，程序和主窗口都还在
    await inWindow("main", () => press("#secondary"));
    await waitFor("停止后悬浮窗关闭", async () => !(await hasWindow("mini")));
  });

  await step("20-20-20 模式开始专注并停止", async () => {
    await inWindow("main", async () => {
      await press('input[name="mode"][value="eyeCare"]');
      await waitFor("状态为「准备专注」", async () => (await text("#status")) === "准备专注");
      await shot("07-eye-idle");
      await press("#primary");
    });
    await waitFor("悬浮窗出现", () => hasWindow("mini"));
    await inWindow("main", async () => {
      await waitFor("状态为「专注中」", async () => (await text("#status")) === "专注中");
      await press("#secondary");
    });
    await waitFor("停止后悬浮窗关闭", async () => !(await hasWindow("mini")));
    await inWindow("main", () => press('input[name="mode"][value="normal"]'));
  });

  await step("设置页「退出程序」能退出", async () => {
    await inWindow("main", async () => {
      await press("#open-settings");
      await sleep(400);
      await shot("08-settings");
      await press("#quit");
    });
    await waitFor(
      "程序退出",
      async () => {
        try {
          return (await session.handles()).length === 0;
        } catch {
          return true;
        }
      },
      15_000,
    );
    exited = true;
  });
} catch (error) {
  try {
    await shot("failure");
  } catch {
    // 程序已卡死或退出时截不到图
  }
  console.error(error);
  process.exitCode = 1;
} finally {
  if (!exited) await session.delete().catch(() => undefined);
}

if (process.exitCode !== 1) console.log("冒烟测试全部通过");
