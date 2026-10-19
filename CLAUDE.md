@AGENTS.md

## 本机环境经验（Claude Code）

### 网络代理：访问 GitHub API 时使用登录 shell 中配置的代理

- **现象**：Claude Code 会话注入的 `HTTP(S)_PROXY=http://127.0.0.1:44011` 对 `api.github.com` 的 CONNECT
  返回 502 / 超时（github.com、crates.io、npm 都正常）。结果是 `gh auth status` 误报 "token is invalid"，
  `gh` 的所有 API 操作（建仓库、看 Actions、发 Release）都失败——**token 其实是好的**。
- **做法**：执行 `gh`（以及其他访问 `api.github.com` 的命令）时，改用用户登录 shell（`zsh -lic`）里配置的代理：
  ```bash
  P=$(env -u HTTP_PROXY -u HTTPS_PROXY -u http_proxy -u https_proxy zsh -lic 'printf %s "$HTTPS_PROXY"')
  HTTPS_PROXY="$P" https_proxy="$P" HTTP_PROXY="$P" http_proxy="$P" gh run list
  ```
- 该代理 URL 含认证信息：**不要打印、不要写入文件或提交记录**。
- **排查顺序**：遇到 `gh` 报 token 无效，先用
  `curl -s -o /dev/null -w '%{http_code}' https://api.github.com/zen` 判断是不是网络问题，再怀疑 token；
  不要自建转发器去绕过会话代理。
- `git` 走 SSH（`git@github.com`）推送，不受上述问题影响；`gh` 的 token 没有 `workflow` scope，
  若改用 HTTPS 推送，修改 `.github/workflows/` 的提交会被拒绝。
- 拉取 crates 索引超时（`[28] Timeout was reached`）时，给 cargo 加 `CARGO_HTTP_MULTIPLEXING=false`。
