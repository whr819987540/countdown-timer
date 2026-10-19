# 倒计时 CountdownTimer

[![CI](https://github.com/whr819987540/countdown-timer/actions/workflows/ci.yml/badge.svg)](https://github.com/whr819987540/countdown-timer/actions/workflows/ci.yml)
[![Rolling release](https://img.shields.io/github/v/release/whr819987540/countdown-timer?label=rolling&include_prereleases)](https://github.com/whr819987540/countdown-timer/releases/tag/rolling)

一个轻量的 Windows 倒计时工具：设置时长 → 开始 → 时间到弹窗提醒 → 点击「重置」以相同时长再来一轮。
支持系统托盘后台驻留、开机自启动，安装包只有几 MB，空闲时几乎不占 CPU。

## 下载安装

- **最新版安装包**：[CountdownTimer-setup.exe](https://github.com/whr819987540/countdown-timer/releases/latest/download/CountdownTimer-setup.exe)
- 所有构建信息见 [Rolling release](https://github.com/whr819987540/countdown-timer/releases/tag/rolling)

系统要求：Windows 10 / 11 x64。按当前用户安装，无需管理员权限；
Windows 10 若未安装 WebView2，安装程序会自动下载。安装包未签名，如出现 SmartScreen 提示，
请点击「更多信息 → 仍要运行」。升级时直接运行新的安装包即可。

## 使用方法

1. 在表盘中央输入时 / 分 / 秒，或点击下方的常用时长按钮；
2. 点击「开始」，可随时「暂停」/「继续」/「停止」；
3. 时间到时会弹出置顶提醒窗口（可选响铃）：
   - 点击 **重置**：以相同时长重新开始倒计时；
   - 点击 **停止**：结束计时。
4. 关闭主窗口后程序会驻留在系统托盘继续计时，单击托盘图标恢复，右键托盘图标可直接控制计时或退出。

完整功能列表与键盘操作见 [docs/FEATURES.md](docs/FEATURES.md)。

## 技术栈

| 部分 | 选择 | 理由 |
| --- | --- | --- |
| 桌面框架 | [Tauri v2](https://tauri.app/)（Rust） | 使用系统自带的 WebView2，安装包与内存占用远小于 Electron |
| 计时核心 | Rust 后台线程 + 单调时钟 | 窗口隐藏时不受浏览器定时器节流影响 |
| 前端 | 原生 TypeScript + CSS（Vite 构建） | 无框架，前端产物约 30 KB |
| 质量检查 | Biome、TypeScript、Node test runner、rustfmt、Clippy、cargo test | 全部在 GitHub Actions 上运行 |
| 安装包 | NSIS（按用户安装） | Tauri 原生支持，体积小 |

## 本地开发

前置条件：Node.js ≥ 24、Rust stable；在 Windows 上需安装
[Tauri 前置依赖](https://tauri.app/start/prerequisites/)（MSVC 生成工具、WebView2）。

```bash
npm ci                 # 安装依赖
npm run tauri dev      # 启动开发模式（热更新前端，自动重编 Rust）
npm run check          # 前端检查：Biome + tsc + 单元测试

cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test             # 运行 Rust 单元测试前需先执行一次 npm run build

npm run tauri build -- --bundles nsis   # 本地打包安装程序（仅限 Windows）
```

## 项目结构

```text
├── index.html / alert.html     主窗口与「时间到」提醒弹窗
├── src/                        前端 TypeScript 与样式
│   ├── lib/                    与后端通信（api.ts）和纯函数工具（time.ts + 测试）
│   └── styles/                 设计令牌（base.css）与页面样式
├── src-tauri/                  Rust 后端
│   └── src/                    timer（状态机）、service（计时线程）、tray、ui、commands …
├── design-system/              由 ui-ux-pro-max skill 生成的设计系统
├── docs/FEATURES.md            功能说明
├── AGENTS.md                   AI 编程助手 / 贡献者指南
└── .github/workflows/ci.yml    检查、打包与 rolling release
```

## CI 与发布

每次推送和 Pull Request 都会在 GitHub Actions 上运行：

1. **前端检查**（Ubuntu）：Biome lint / 格式、TypeScript 类型检查、单元测试、Vite 构建；
2. **Rust 检查**（Windows）：`cargo fmt`、`cargo clippy -D warnings`、`cargo test`；
3. **打包**（Windows）：`tauri build` 生成 NSIS 安装包并上传为构建产物。

推送到 `main` 且以上全部通过后，自动删除旧的 `rolling` 发布并用最新安装包重新创建（版本号 `0.1.<构建序号>`）。

## 致谢

- [Tauri](https://tauri.app/)
- 图标：[Phosphor Icons](https://phosphoricons.com/)（MIT）
- UI 设计辅助：[ui-ux-pro-max-skill](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill)

## 许可证

[MIT](LICENSE)
