import type { UpdateStatus } from "./api.ts";

/** 由更新状态推导出的界面内容 */
export interface UpdateView {
  /** 主窗口底部的提示；省略时显示原来的托盘提示 */
  banner?: { text: string; actionable: boolean };
  /** 设置面板中的状态文字（会被读屏播报，因此不含下载百分比） */
  status: string;
  /** 设置面板中的按钮 */
  button: { label: string; action: "check" | "install" | "none" };
}

/** `timerIdle`：安装会重启程序，只有未在计时时才允许安装 */
export function updateView(status: UpdateStatus, timerIdle: boolean): UpdateView {
  const check = { label: "检查更新", action: "check" } as const;
  switch (status.state) {
    case "idle":
      return { status: "尚未检查更新", button: check };
    case "checking":
      return { status: "正在检查更新…", button: { label: "检查中…", action: "none" } };
    case "upToDate":
      return { status: "已是最新版本", button: check };
    case "error":
      return { status: status.message, button: check };
    case "available": {
      const found = `发现新版本 ${status.version}`;
      return timerIdle
        ? {
            banner: { text: `${found}，点击更新`, actionable: true },
            status: found,
            button: { label: "立即更新", action: "install" },
          }
        : {
            banner: { text: `${found}，停止计时后可更新`, actionable: false },
            status: `${found}，停止计时后可更新`,
            button: { label: "立即更新", action: "none" },
          };
    }
    case "downloading": {
      const percent = status.percent === null ? "" : ` ${status.percent}%`;
      return {
        banner: { text: `正在下载更新${percent}`, actionable: false },
        status: `正在下载 ${status.version}…`,
        button: { label: percent ? `下载中${percent}` : "下载中…", action: "none" },
      };
    }
    case "installing": {
      const text = `正在安装 ${status.version}，程序将自动重启`;
      return {
        banner: { text, actionable: false },
        status: text,
        button: { label: "安装中…", action: "none" },
      };
    }
  }
}
