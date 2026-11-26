# AGENTS.md

给 AI 编程助手（以及人类贡献者）的项目指南。动手前先读完本文件。

## 项目概览

轻量的 Windows 倒计时桌面应用：设置时长 → 开始 → 时间到弹出置顶提醒 → 用户点击「重置」以相同时长再来一轮。
另有 20-20-20 护眼模式（专注 20 分钟 → 确认后远眺休息 20 秒 → 手动开始下一轮）。
支持系统托盘后台驻留、开机自启动。功能细节以 [docs/FEATURES.md](docs/FEATURES.md) 为准。

- 桌面框架：Tauri v2（Rust，系统 WebView2）
- 前端：原生 TypeScript + CSS，Vite 构建，**不引入 UI 框架**
- 分发：NSIS 安装包，GitHub Actions 在 `main` 上做 rolling release

「轻量」是硬性约束：新增依赖（npm 或 crate）前先确认无法用现有能力或几十行代码实现，并在提交说明里写明理由。

## 目录结构

```text
index.html                主窗口（含内联 SVG 图标 sprite）
alert.html                提醒弹窗：时间到 / 护眼模式的休息提醒（无边框、置顶，由后端按需创建 / 销毁）
mini.html                 计时悬浮窗：48×48 透明窗口里的圆形进度环（置顶、不占任务栏，由后端按需创建 / 销毁）
src/
  main.ts                 主窗口逻辑：渲染状态、时长编辑、设置面板
  alert.ts                提醒弹窗逻辑
  mini.ts                 悬浮窗逻辑
  lib/api.ts              所有 Tauri 命令与事件的类型化封装（前端只通过它和后端通信）
  lib/time.ts             纯函数工具（格式化、解析、步进），配套 time.test.ts
  lib/view.ts             由快照推导界面文案（状态、按钮、弹窗内容），配套 view.test.ts
  lib/update.ts           由更新状态推导界面内容（底部提示、设置面板），配套 update.test.ts
  lib/fit.ts              窗口可用空间不足时整体等比缩放页面，配套 fit.test.ts
  styles/base.css         设计令牌（颜色 / 间距 / 字体 / 动效）与通用组件
  styles/main.css         主窗口样式
  styles/alert.css        提醒弹窗样式
  styles/mini.css         悬浮窗样式
src-tauri/
  tauri.conf.json         窗口、CSP、打包（NSIS）配置
  capabilities/           前端权限（最小化：core:default + 拖动窗口；新窗口要加进 windows 列表）
  icons/                  由 assets/app-icon.svg 生成，不要手改
  src/main.rs             组装插件、托盘、窗口事件、命令
  src/timer.rs            倒计时状态机（纯逻辑 + 单元测试）
  src/service.rs          AppState、后台计时线程、状态同步（publish）
  src/settings.rs         settings.json 读写（+ 单元测试）
  src/commands.rs         暴露给前端的命令
  src/tray.rs             托盘图标与菜单
  src/ui.rs               主窗口 / 提醒弹窗管理
  src/fit.rs              窗口适配屏幕工作区的纯计算（+ 单元测试）
  src/sound.rs            Windows 闹钟提示音（PlaySoundW）
  src/updater.rs          自动更新：定时检查、下载、校验签名、安装（tauri-plugin-updater）
  src/download.rs         分段并行下载（Range 请求、断点续传，+ 单元测试），供更新下载安装包
assets/app-icon.svg       应用图标源文件
design-system/            ui-ux-pro-max 生成的设计系统（MASTER.md 为视觉规范）
e2e/                      Windows 端到端冒烟测试（smoke.ts + 最小 WebDriver 客户端 webdriver.ts，CI 中运行）
docs/FEATURES.md          功能说明
.github/workflows/ci.yml  检查、打包、rolling release
```

## 常用命令

```bash
npm ci                    # 安装依赖（Node ≥ 24）
npm run tauri dev         # 开发模式（需要 Windows，或装有 webkit2gtk 的 Linux）
npm run check             # 前端：biome ci + tsc --noEmit + node --test
npm run format            # 自动修复格式 / import 排序
npm run build             # 构建前端到 dist/（Rust 编译期需要它存在）

cd src-tauri
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test

npx tauri icon assets/app-icon.svg -o src-tauri/icons   # 修改图标后重新生成，并删除 android/ios/Square* 等无用文件
```

在 Linux 上无法编译 Linux 版 Tauri（缺 webkit2gtk）时，可以对 Windows 目标做交叉检查（只检查、不链接）：

```bash
rustup target add x86_64-pc-windows-msvc
cd src-tauri
CC_x86_64_pc_windows_msvc=clang cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

- 资源文件需要 `clang`（预处理）和较新的 `llvm-rc`；`llvm-rc` 10 解析不了 Tauri 生成的 manifest，
  只做 clippy 时可以在 `PATH` 前面放一个只创建空输出文件（`/fo` 参数）的 `llvm-rc` 替身。
- `cargo test` 需要链接，只能在 Windows（CI）上运行。

## 架构要点

- **计时只在 Rust 里做**。`timer::Timer` 是纯状态机（Idle / Running / Paused / Finished），所有方法显式接收
  `now: Instant`，便于测试。前端只负责展示：收到快照后用 `performance.now()` 在两次推送之间插值。
- **模式与段**：`Mode`（Normal / EyeCare）只能在空闲时切换并记入设置；一次计时由若干段组成，
  `Stage`（Work / Rest）标识当前段。每段结束都停在 Finished，由 `advance` 进入下一段：普通模式以相同时长再来一轮，
  护眼模式在 20 分钟专注与 20 秒休息之间交替。快照的 `durationMs` 是**当前段**的时长。
- **线程模型**（见 `service.rs` 顶部注释）：
  - 命令和托盘菜单修改状态机后调用 `service::refresh`；
  - 后台线程 `countdown-ticker` 用 `Condvar` 等待到下一个整秒边界，检测本轮是否结束；
  - 所有界面副作用（推送 `timer-state` 事件、更新托盘、开关悬浮窗 / 提醒弹窗、提示音）都排队到主线程的
    `publish` 执行，执行时读取**最新**状态，因此不会乱序。不要在持有 `timer` 锁时调用任何会派发到主线程并等待的
    Tauri API。
  - `refresh` 必须经后台线程再 `run_on_main_thread`：在主线程上调用时 Tauri 会**同步**执行闭包，
    而同步命令 / 托盘回调都在主线程，此时 `publish` 创建窗口会在 Windows 上死锁（0.1.6 的事故：
    开始计时后程序卡死、托盘无法退出）。**任何会创建窗口的代码都不能在同步命令或事件回调里直接执行。**
- **前后端契约**：命令名、参数和 `Snapshot` / `Settings` 结构在 `src/lib/api.ts` 与 Rust 两侧必须同步修改。
  Rust 结构体统一 `#[serde(rename_all = "camelCase")]`。命令不返回快照，状态一律通过 `timer-state` 事件下发。
- **提醒弹窗按需创建**，用户操作后 `destroy()`，不常驻内存；用 `Alt+F4` 关闭等同于「停止」。
  显示条件由 `publish` 根据快照判断：Finished 时打开并置前，护眼模式的休息段（Running / Paused）期间保持打开
  显示倒计时，其余状态销毁。
- **悬浮窗**（`ui::show_mini` / `exit_mini`）：`publish` 检测到一次计时开始（空闲 → 非空闲）且开启了
  `mini_on_start` 时打开悬浮窗并隐藏主窗口，记录主窗口原本是否可见；回到空闲时关闭悬浮窗，按记录恢复主窗口。
  `ui::show_main`（展开、托盘、单实例唤起）会先关闭悬浮窗。悬浮窗关闭时把位置写入设置，下次打开时若仍在某个
  显示器工作区内就沿用，否则放到右下角（计算在 `fit.rs`；不放右上角，以免挡住最大化窗口的关闭按钮）。
  计时中最小化主窗口、或关闭主窗口（`close_to_tray` 时）会经 `ui::shrink_main_later` 缩回悬浮窗
  （窗口事件回调在主线程，所以先切到后台线程再派发回主线程创建窗口）；关闭 `mini_on_start` 后保持原行为。
  Windows 不允许窗口窄于约 136 像素，悬浮窗会被撑宽：页面把徽章贴在右侧，`ui::shape_mini` 用
  `SetWindowRgn` 把窗口裁成贴右侧的圆形，圆形外的点击穿透到下面的窗口；尺寸变化时重新裁剪。
  **徽章直径与裁剪直径都取窗口高度**（CSS 用 `100vh`，不写死 48px，也不按"48 × 缩放比例"算），
  这样窗口尺寸没跟上缩放比例（系统关闭"拖动时显示窗口内容"时 tao 不在 `WM_DPICHANGED` 中调整尺寸）
  或页面被缩放时，圆形仍然完整；`ui::refit_mini_later` 在缩放比例变化后再把窗口设回 48 逻辑像素（徽章中心不动）。
  底盘外缘离裁剪边缘留 1 个 SVG 单位，避免锯齿状的区域边缘切断描边。悬浮窗刻意不显示数字（用户反馈跳动的秒数分散注意力），
  剩余时间只在悬停提示中按分钟显示；单击 / 拖动由 `mini.ts` 自行区分（移动超过 3px 才调用 `startDragging`）。
  窗口透明且无阴影，页面的 `color-scheme` 设为 `normal`，否则深色模式下画布会被涂成不透明的深色。
- **创建 / 销毁窗口的命令必须是 `async`**：同步命令在主线程执行，Windows 上在其中创建窗口会死锁；
  命令内再用 `run_on_main_thread` 派发窗口操作。
- **主窗口启动时隐藏**（`visible: false`），在 `setup` 中设置与主题一致的背景色后再显示，避免闪白；
  带 `--hidden` 参数（开机自启动）时不显示。
- **高缩放比例适配**：窗口尺寸以逻辑像素固定且不可调整，高缩放比例下可能比屏幕工作区还高，而系统按整块屏幕居中，
  底部会被任务栏遮住。`ui::fit_to_work_area`（计算在 `fit.rs`）在显示主窗口 / 创建提醒弹窗前把窗口缩进工作区并居中；
  前端 `fitToWindow` 再按可用尺寸设置根元素 `zoom`（下限 0.8，`.app` / `.settings` / `.alert` 可滚动兜底）。
  页面布局仍按 380×600（弹窗 360×320）设计，改窗口尺寸时同步修改 `fitToWindow` 的参数。
- 设置保存在 `app_config_dir()/settings.json`；开机自启动状态以 `tauri-plugin-autostart`（注册表）为准，不存文件。
- **自动更新**（`updater.rs`）：线程 `update-checker` 启动 10 秒后、之后每 6 小时检查一次（debug 构建不自动检查）。
  状态通过 `update-state` 事件下发，发现新版本只提示；`install` 在计时未空闲时拒绝，下载完成后再确认一次。
  Windows 上插件运行安装程序后直接 `exit(0)`，所以退出前的清理（移除托盘图标）放在检查时设置的
  `on_before_exit` 中；安装前写入标记文件，重启后即使带 `--hidden` 也显示主窗口。
  前端不直接调用插件 API（capabilities 保持最小），只调用 `check_update` / `install_update` 等自有命令。
- **更新包下载**不用插件自带的单连接 `Update::download`，而是 `download::fetch`：先用 `Range: bytes=0-0`
  探测大小，再切成最多 4 段并行下载（强制 HTTP/1.1，否则各段会复用同一连接）；GitHub 跳转得到的签名地址有有效期，
  重试时从原始地址重新跳转。下载后先 `Update::verify` 校验签名，`install` 内部还会再校验一次。
  `reqwest` / `futures-util` 是插件已有的依赖，直接依赖时版本要与插件一致，不要额外开启特性（会增大体积）。
- **更新签名密钥**：公钥在 `tauri.conf.json` 的 `plugins.updater.pubkey`，并开启 `requireSignedVersion`
  （拒绝签名中未记录版本号、或版本号与 `latest.json` 不一致的安装包，防止降级攻击）；私钥只存在维护者本机
  （`~/.tauri/countdown-timer.key`）和仓库 secret `TAURI_SIGNING_PRIVATE_KEY` 中，**绝不提交**。
  私钥丢失后已安装的程序将无法再自动更新（只能手动安装一次带新公钥的版本）。

## 编码约定

- 注释、界面文案、错误信息、文档使用**简体中文**；代码标识符使用英文。
- Rust：`cargo fmt` + `clippy -D warnings` 必须通过；不使用 `unwrap()` 处理运行时可能失败的情况
  （锁中毒用 `PoisonError::into_inner` 恢复）；`unsafe` 块必须写 `// SAFETY:` 说明。
- TypeScript：`strict` + `noUncheckedIndexedAccess`；相对导入写 `.ts` 扩展名；只用可擦除语法
  （不用 `enum` / `namespace`），以便 `node --test` 直接运行测试。纯逻辑放进 `src/lib/` 并写测试。
- CSS：组件里只用 `base.css` 中的语义令牌（`var(--color-*)` 等），不写裸色值；颜色变化必须同时考虑深浅两套主题。
- 图标：使用 [Phosphor Icons](https://phosphoricons.com/) 的 SVG 路径，内联到 HTML 的 `<symbol>` sprite 中，
  不引入图标库运行时；装饰性图标加 `aria-hidden="true"`，纯图标按钮必须有 `aria-label`。

## UI 设计流程（必须）

所有影响界面外观或交互的改动，都要使用 **ui-ux-pro-max** skill：

1. 安装（skill 不纳入版本库，`.claude/` 已被忽略）：
   ```bash
   git clone --depth 1 https://github.com/nextlevelbuilder/ui-ux-pro-max-skill.git /tmp/uipro
   mkdir -p .claude/skills && cp -r /tmp/uipro/.claude/skills/ui-ux-pro-max .claude/skills/
   ```
2. 先读 [design-system/countdown-timer/MASTER.md](design-system/countdown-timer/MASTER.md)，尤其是顶部的
   「项目适配」一节——它覆盖了自动生成的部分内容。页面级覆盖放在 `design-system/countdown-timer/pages/`。
3. 针对具体问题用 skill 检索，例如：
   ```bash
   python3 .claude/skills/ui-ux-pro-max/scripts/search.py "keyboard focus modal" --domain ux
   python3 .claude/skills/ui-ux-pro-max/scripts/search.py "theme dark light" --stack winui
   ```
4. 交付前过一遍 MASTER.md 末尾的检查清单：对比度 ≥ 4.5:1（深浅两套都要算）、焦点环可见、
   `prefers-reduced-motion` 生效、不用 emoji 当图标、无布局跳动。

## 文档维护（必须）

| 改动类型 | 需要同步更新 |
| --- | --- |
| 新增 / 修改 / 删除用户可感知的功能、快捷键、设置项 | `docs/FEATURES.md`（必要时 `README.md` 的使用方法） |
| 技术栈、目录结构、命令、架构、约定变化 | `AGENTS.md`（必要时 `README.md`） |
| CI / 发布流程变化 | `README.md` 的「CI 与发布」与本文件「CI / 发布」 |
| 视觉规范变化 | `design-system/countdown-timer/MASTER.md` |

文档与代码在**同一个提交**里更新。

## CI / 发布

- 工作流：`.github/workflows/ci.yml`，在 push 到 `main`、Pull Request、手动触发时运行。
  - `frontend`（ubuntu）：`npm run lint` / `typecheck` / `test` / `build`
  - `rust`（windows）：`cargo fmt --check`、`cargo clippy --all-targets --locked -D warnings`、`cargo test --locked`
  - `build`（windows）：`tauri build --bundles nsis`，版本号注入为 `0.1.<run_number>`；有 `TAURI_SIGNING_PRIVATE_KEY`
    时生成更新签名并写出 `latest.json`（没有时跳过，只打普通安装包），上传安装包产物
  - `release`（仅 `main`，依赖前三个 job 全部通过）：删除旧的 `rolling` 发布与标签，用新安装包重新创建并标记为 Latest；
    GitHub 接口偶发 5xx 时最多重试 5 次
  - `e2e`（windows，依赖 `build`）：静默安装刚打出的安装包，用 `tauri-driver` + 与 WebView2 版本匹配的 `msedgedriver`
    运行 `e2e/smoke.ts`，像用户一样走一遍主流程（开始 → 悬浮窗 → 时间到弹窗 → 停止 → 展开 / 缩回（按钮、最小化、关闭）
    → 20-20-20 → 退出），每步有超时，程序卡死即失败；最小化 / 关闭主窗口通过环境变量 `APP_PID` 找到窗口、
    用 PowerShell 发送原生窗口消息；各状态截图上传为产物 `e2e-screenshots`。`release` 依赖它通过
- 发布产物：`CountdownTimer_<版本>_x64-setup.exe`、固定名称的 `CountdownTimer-setup.exe`
  （README 中的永久下载链接依赖后者，改名时同步修改）与 `latest.json`（自动更新清单，
  `url` 指向带版本号的安装包，客户端从 `releases/download/rolling/latest.json` 读取）。
- 推送后用 GitHub CLI 跟踪进度，失败时查看日志修复后再推送：
  ```bash
  gh run list --limit 5
  gh run watch <run-id> --exit-status
  gh run view <run-id> --log-failed
  ```
- `Cargo.lock` 与 `package-lock.json` 必须提交（CI 使用 `--locked` / `npm ci`）。

## 提交规范

使用 Conventional Commits，说明用中文，例如 `feat: 提醒弹窗支持 Esc 停止`、`fix: 暂停后托盘提示未更新`、
`docs: 更新功能说明`、`ci: 缓存 Rust 依赖`。

## 工作流程

功能或修复完成、本地检查通过后，直接提交并推送到 `main`，然后用 GitHub CLI 盯 CI 直到全部通过（含 rolling release）；
失败时查看日志、修复后再推送（命令见「CI / 发布」）。

- 本机（Linux）无法运行 Windows 程序，界面与窗口行为由 CI 的 `e2e` 任务在 Windows 上验证。
  新增或修改窗口、按钮、主流程时，同步更新 `e2e/smoke.ts`，并在 CI 产物 `e2e-screenshots` 中检查真实渲染。
- 修改 CI 本身或调试 `e2e` 时，可以先推到分支，用 `gh workflow run ci.yml --ref <分支>` 手动触发（分支上不会发布），
  通过后再合入 `main`。
