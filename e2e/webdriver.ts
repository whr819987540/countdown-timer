/**
 * 最小的 W3C WebDriver 客户端：只实现冒烟测试用到的几个命令，
 * 直接用 Node 自带的 fetch，避免为测试引入 WebdriverIO 等框架依赖。
 */

/** 单个请求的超时：程序卡死时请求不会一直挂起 */
const REQUEST_TIMEOUT_MS = 20_000;
/** 创建会话要启动程序并等 WebView2 就绪，冷启动的 CI 机器上可能较慢 */
const SESSION_TIMEOUT_MS = 120_000;

export class WebDriverError extends Error {}

async function request<T>(
  method: string,
  url: string,
  body?: unknown,
  timeoutMs = REQUEST_TIMEOUT_MS,
): Promise<T> {
  let response: Response;
  try {
    response = await fetch(url, {
      method,
      headers: body === undefined ? undefined : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(timeoutMs),
    });
  } catch (error) {
    throw new WebDriverError(
      `${method} ${url} 未在 ${timeoutMs / 1000} 秒内响应：${String(error)}`,
    );
  }
  const text = await response.text();
  let value: unknown;
  try {
    value = (JSON.parse(text) as { value: unknown }).value;
  } catch {
    throw new WebDriverError(`${method} ${url} 返回了非 JSON 内容：${text.slice(0, 200)}`);
  }
  if (!response.ok) {
    const { error, message } = (value ?? {}) as { error?: string; message?: string };
    throw new WebDriverError(`${method} ${url} 失败：${error ?? response.status} ${message ?? ""}`);
  }
  return value as T;
}

export class Session {
  private readonly url: string;

  private constructor(url: string) {
    this.url = url;
  }

  static async create(server: string, capabilities: Record<string, unknown>): Promise<Session> {
    const value = await request<{ sessionId: string }>(
      "POST",
      `${server}/session`,
      { capabilities: { alwaysMatch: capabilities } },
      SESSION_TIMEOUT_MS,
    );
    return new Session(`${server}/session/${value.sessionId}`);
  }

  private command<T>(method: string, path: string, body?: unknown): Promise<T> {
    return request<T>(method, `${this.url}${path}`, body);
  }

  handles(): Promise<string[]> {
    return this.command("GET", "/window/handles");
  }

  async switchTo(handle: string): Promise<void> {
    await this.command("POST", "/window", { handle });
  }

  /** 在当前窗口执行脚本；脚本中用 arguments[i] 读取参数 */
  execute<T>(script: string, ...args: unknown[]): Promise<T> {
    return this.command("POST", "/execute/sync", { script, args });
  }

  /** 当前窗口页面内容的 PNG 截图 */
  async screenshot(): Promise<Buffer> {
    return Buffer.from(await this.command<string>("GET", "/screenshot"), "base64");
  }

  async delete(): Promise<void> {
    await request("DELETE", this.url);
  }
}
