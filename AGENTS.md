# AGENTS.md

给 AI 编程助手（以及人类贡献者）的项目指南。动手前先读完本文件。

## 项目概览

轻量的 Windows 倒计时桌面应用：设置时长 → 开始 → 时间到弹出置顶提醒 → 用户点击「重置」以相同时长再来一轮。
支持系统托盘后台驻留、开机自启动。功能细节以 [docs/FEATURES.md](docs/FEATURES.md) 为准。

- 桌面框架：Tauri v2（Rust，系统 WebView2）
- 前端：原生 TypeScript + CSS，Vite 构建，**不引入 UI 框架**
- 分发：NSIS 安装包，GitHub Actions 在 `main` 上做 rolling release

「轻量」是硬性约束：新增依赖（npm 或 crate）前先确认无法用现有能力或几十行代码实现，并在提交说明里写明理由。

## 目录结构

```text
index.html                主窗口（含内联 SVG 图标 sprite）
alert.html                「时间到」提醒弹窗（无边框、置顶，由后端按需创建 / 销毁）
src/
  main.ts                 主窗口逻辑：渲染状态、时长编辑、设置面板
  alert.ts                提醒弹窗逻辑
  lib/api.ts              所有 Tauri 命令与事件的类型化封装（前端只通过它和后端通信）
  lib/time.ts             纯函数工具（格式化、解析、步进），配套 time.test.ts
  styles/base.css         设计令牌（颜色 / 间距 / 字体 / 动效）与通用组件
  styles/main.css         主窗口样式
  styles/alert.css        提醒弹窗样式
src-tauri/
  tauri.conf.json         窗口、CSP、打包（NSIS）配置
  capabilities/           前端权限（最小化：core:default + 拖动窗口）
  icons/                  由 assets/app-icon.svg 生成，不要手改
  src/main.rs             组装插件、托盘、窗口事件、命令
  src/timer.rs            倒计时状态机（纯逻辑 + 单元测试）
  src/service.rs          AppState、后台计时线程、状态同步（publish）
  src/settings.rs         settings.json 读写（+ 单元测试）
  src/commands.rs         暴露给前端的命令
  src/tray.rs             托盘图标与菜单
  src/ui.rs               主窗口 / 提醒弹窗管理
  src/sound.rs            Windows 闹钟提示音（PlaySoundW）
assets/app-icon.svg       应用图标源文件
design-system/            ui-ux-pro-max 生成的设计系统（MASTER.md 为视觉规范）
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
- **线程模型**（见 `service.rs` 顶部注释）：
  - 命令和托盘菜单修改状态机后调用 `service::refresh`；
  - 后台线程 `countdown-ticker` 用 `Condvar` 等待到下一个整秒边界，检测本轮是否结束；
  - 所有界面副作用（推送 `timer-state` 事件、更新托盘、开关提醒弹窗、提示音）都排队到主线程的
    `publish` 执行，执行时读取**最新**状态，因此不会乱序。不要在持有 `timer` 锁时调用任何会派发到主线程并等待的
    Tauri API。
- **前后端契约**：命令名、参数和 `Snapshot` / `Settings` 结构在 `src/lib/api.ts` 与 Rust 两侧必须同步修改。
  Rust 结构体统一 `#[serde(rename_all = "camelCase")]`。命令不返回快照，状态一律通过 `timer-state` 事件下发。
- **提醒弹窗按需创建**，用户操作后 `destroy()`，不常驻内存；用 `Alt+F4` 关闭等同于「停止」。
- **主窗口启动时隐藏**（`visible: false`），在 `setup` 中设置与主题一致的背景色后再显示，避免闪白；
  带 `--hidden` 参数（开机自启动）时不显示。
- 设置保存在 `app_config_dir()/settings.json`；开机自启动状态以 `tauri-plugin-autostart`（注册表）为准，不存文件。

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
  - `build`（windows）：`tauri build --bundles nsis`，版本号注入为 `0.1.<run_number>`，上传安装包产物
  - `release`（仅 `main`，依赖前三个 job 全部通过）：删除旧的 `rolling` 发布与标签，用新安装包重新创建并标记为 Latest
- 发布产物：`CountdownTimer_<版本>_x64-setup.exe` 与固定名称的 `CountdownTimer-setup.exe`
  （README 中的永久下载链接依赖后者，改名时同步修改）。
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
