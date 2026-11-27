# CloudMCP

为远端 Agent 提供本机目录工具、后台命令执行与 Codex 会话记录。远端主动发起，本机保存执行过程；切换远端 Agent 后，可按授权目录名称接手。

```text
远端 Agent / MCP 客户端
  → HTTPS / Cloudflare Tunnel
    → 本机 CloudMCP
      → 授权目录内的文件与命令工具
      → 本机后台执行器 → Codex（默认完整权限）
      → SQLite 执行记录 / 会话 ID / 有限日志
    ← 执行编号、状态、摘要；详细结果按需查询
```

CloudMCP 自动记录工具执行，不提供业务任务创建或排队界面。远端 Agent 负责推理和调度；`codex_exec` 使用本机已有的 Codex 账号、配置与会话存储。无需接入 Claude Code，也不转发消息给其他 Agent。

## 下载与升级

从 GitHub Releases 下载对应芯片的 DMG，将 CloudMCP 拖到「应用程序」。[下载最新版本](https://github.com/kohlarnhin/CloudMcp/releases/latest)

- Apple Silicon（M 系列）选择 `aarch64.dmg`。
- Intel Mac 选择 `x86_64.dmg`。

当前 DMG 使用 macOS 临时签名。首次打开时，可能需要在「系统设置 → 隐私与安全性」确认运行。

正式版启动时会检查更新，也可在「设置 → 关于」手动检查。确认安装后，应用会验证更新包的签名并重启。请先保存工作，重启会断开当前的 MCP 连接并停止正在运行的执行。早期的本地构建需要先通过 DMG 安装 `0.1.0`，应用会沿用原来的本机数据目录。

## 技术栈

Tauri 2、React 18、TypeScript、Vite、Tailwind CSS 4、shadcn/ui（Radix）。Rust 后端使用官方 `rmcp` SDK、Axum、SQLite、Tokio 和 `cap-std`。界面沿用 Bento（`~/Software/apphub`）的 H1 磨砂白风格，等宽字体为内置的 JetBrains Mono；在「应用设置 → 外观」可选择跟随系统、浅色或深色。

## 运行与构建

需要 Node.js / pnpm、Rust 1.88+ 和 Tauri 对应平台的构建依赖。

```sh
pnpm install
pnpm tauri dev
```

构建内置 cloudflared 的 macOS 应用：

```sh
pnpm tauri build --bundles app --config src-tauri/tauri.macos-bundle.conf.json
```

前端逻辑测试用 `node --test 'scripts/*.test.mjs'` 运行，后端测试用 `cd src-tauri && cargo test` 运行。

文件与普通命令工具不依赖 Codex 或 Python；使用 `codex_exec` 需要本机已安装并配置 Codex CLI。macOS 发布包内置 cloudflared，源码开发或单独运行的二进制可使用本机的 cloudflared。

## 发布

发布前在 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 中同步版本号，运行 `cargo check --manifest-path src-tauri/Cargo.toml` 更新 Rust 锁文件，然后提交并推送。用不带 `v` 的 `X.Y.Z` 标签触发 GitHub Actions，首个标签是 `0.1.0`。

```sh
git tag -a 0.1.1 -m 'CloudMCP 0.1.1'
git push origin 0.1.1
```

工作流在两种架构都完成打包和产物检查后，统一发布 DMG、签名更新包和 `latest.json`。仓库的 Actions Secrets `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 用于签署更新包。后续发布须继续使用与应用内公钥配对的签名密钥。

## 连接远端 Agent

1. 启动 CloudMCP，在“授权目录”选择本机目录，确认唯一的对外名称。默认使用文件夹名称，同名文件夹可设置不同别名。
2. 在“应用设置”检查 cloudflared、Codex 路径及默认执行时限。
3. 在“连接概览”启动 MCP 服务，再连接隧道，获取 HTTPS 地址。
4. 复制配置与令牌，添加到远端 Agent 的 MCP 客户端。
5. 在远端 Agent 中发起操作；它通过 MCP 调用本机工具。

要使用完整的委派交付流程，把 `skills/codex-delegate` 整个目录导入远端 Agent 的 Skills，并在远端任务中调用 `codex-delegate`。调用者决定方案、独立验收、发起同会话返工，以及完成已授权的提交和发版。 [codex-delegate Skill](skills/codex-delegate/SKILL.md)

通用 HTTP 客户端配置示例，具体字段可按远端 Harness 调整：

```json
{
  "mcpServers": {
    "cloudmcp": {
      "type": "http",
      "url": "https://YOUR-HOST/mcp",
      "headers": {
        "Authorization": "Bearer YOUR-LOCAL-TOKEN"
      }
    }
  }
}
```

本机默认监听 `127.0.0.1:8787`，服务使用 Streamable HTTP 无状态模式和 JSON 响应。只暴露 MCP 工具，目录授权和应用配置不开放远程修改。

## MCP 工具

| 工具 | 用途 |
| --- | --- |
| `workspace_list` | 查询授权目录，使用其唯一 name 作为 workspace，路径显示为 `~/…` |
| `fs_list` | 分页浏览目录，最多 100 项 |
| `fs_read` | 分页读取文本，单次最多 32 KiB；传 `line` 按行读取并带行号，否则按字节 offset 分页 |
| `fs_image` | 返回 PNG / JPEG / WebP 图片内容，最多 2 MiB |
| `fs_search` | 字面文本搜索，最多 80 个命中，并限制扫描规模 |
| `fs_edit` | 原子替换已有 UTF-8 文本文件中的精确字符串，默认要求唯一匹配，修改已有文件时优先使用 |
| `fs_write` | 原子创建或替换文本文件，单次最多 1 MiB |
| `fs_mutate` | mkdir / rename / remove，递归操作需显式指定 |
| `command_exec` | 自动记录并后台执行目录内的命令，短暂等待后返回执行编号、状态与有限输出 |
| `codex_exec` | 新建或明确续跑本机 Codex 会话，默认完整权限，可选委派实施或只读审查角色 |
| `codex_portal` | 用 `action=status` 查询本机 Codex Portal 额度，或用 `action=switch` 触发 Portal 自行决定是否切换账号 |
| `execution_list` | 按目录名称分页查询执行；可用 kind=codex 只查 Codex 会话 |
| `execution_get` | 查询执行状态、会话 ID、原始要求、验收条件、摘要和输出末尾 |
| `execution_output` | 分页读取 stdout / stderr / prompt / acceptance_criteria / summary / worker，每次最多 32 KiB |
| `execution_wait` | 等待执行结束，最多 50 秒，结束即返回；替代反复调用 `execution_get` |
| `execution_cancel` | 请求停止执行进程组，保留已有文件改动与执行记录 |
| `execution_review` | 根据检查证据标记待验收、已完成或需要继续；明确核实已停止后可处理 unknown 状态 |

工具使用 `workspace`（授权目录的唯一名称）和目录内相对路径；文件与命令工具继续兼容旧 `scope_id` 参数和目录 UUID。目录别名只在本机修改，改名后执行记录仍关联原目录。`~` 是本机路径的显示别名，不应按远端机器的 HOME 展开。

目录内具备读取、新建、修改、重命名、删除和命令执行权限。直接文件操作使用 `cap-std` 目录能力句柄，拒绝绝对路径、父路径跳转和逃出授权根的符号链接。授权目录不能重叠，也不能包含 CloudMCP 应用数据目录。

普通 `command_exec` 使用 macOS 原生 `sandbox-exec`。授权目录可读写，系统运行时目录只读，另有临时目录。`HOME` 为所选授权根，命令使用相对路径。`codex_exec` 默认以完整权限启动 Codex，工作目录用来定位项目。用 `delegation="implement"` 发送委派实施规则，或用 `delegation="review"` 进行只读审查。续跑省略 `delegation` 时沿用上一轮角色。

`command_exec` 对 macOS 当前选用的 Apple 开发工具目录入口及其 `usr` 子目录提供只读访问，供系统 Git 加载运行依赖。由于命令的 `HOME` 是项目目录，可在仓库级 Git 配置中设置提交作者。

实际执行时限默认 3,600 秒，本机设置或单次 `timeout_seconds` 可设为 1–86,400 秒。`wait_seconds` 仅控制当前 MCP 请求等待多久（默认 1 秒，最多 10 秒），之后返回 `run_id`；需要等待长时间执行时，用 `execution_wait`（`timeout_seconds` 默认 30、最多 50 秒），执行一离开 starting / running 状态就返回，并附带 `finished` 和 `waited_seconds`。`timeout_seconds` 是轮询预算，耗尽后不再发起新查询，但最后一次查询可能略超时；`finished` 只在确认的终态为 true，`unknown` 也会立即返回但为 false，需先核实进程；后台执行不依赖 HTTP 连接或远端 Agent 额度。执行结束会停止受管子进程组，超时或停止不回滚已有文件改动。非 macOS 平台的普通命令工具暂不支持。

`fs_search` 跳过依赖和构建目录、符号链接、二进制和大文件；需要更多结果时缩小子目录范围。图片按需读取，不将完整目录或全部日志自动返回远端。

`fs_read` 有两种模式。行模式传 `line`（从 1 开始）和可选 `limit`（默认 400 行，最多 2000 行），返回 `cat -n` 风格的带行号文本，以及 `start_line`、`end_line`、`next_line`；文件不超过 8 MiB 时附带 `total_lines`；本页返回的行中有 CRLF 结尾时附带 `line_ending: "crlf"`。单行超过 2000 字符会截断并标注，`truncated` 只表示是否还有下一页；整段在 32 KiB 内按整行结束。行模式对返回的字符拒绝非法 UTF-8（被截断舍弃的部分不校验）。行号只用于定位，`fs_edit` 的 `old_string` 不要包含行号前缀。不传 `line` 时保持字节模式，用 `offset` / `max_bytes` 分页；`line` 与 `offset` 不能同时传。

`fs_edit` 支持已存在且不含 NUL 的 UTF-8 普通文件，按原样匹配 `old_string`，保留空白和换行差异。默认要求只匹配一处，设置 `replace_all=true` 可替换全部匹配。修改前后均最多 1 MiB，写回时保留文件权限。返回替换次数 `replacements`、写入字节数 `written_bytes` 和首处匹配的行号 `first_line`（从 1 开始）。

## 路径隐私与认证

内部使用真实绝对路径定位文件。MCP 对外返回的目录、路径元数据、错误和命令输出中的本机主目录前缀统一显示为 `~`，不在授权目录列表中暴露用户名。

文件不会因为路径展示而被改写。返回文本中的主目录前缀也会归一化，`fs_read` 的分页 offset 仍基于原始文件字节。

所有 `/mcp` 请求均需携带 Bearer 令牌。应用数据目录权限为 0700，保存授权、令牌、执行要求、会话 ID、验收说明与输出。stdout/stderr 各保留最先的 16 MiB，持续捕获最新 32 KiB 末尾，返回时按字符上限截断；达到日志上限仍继续消费 Codex 会话事件。截图留在授权目录，使用 `fs_image` 按需读取。复制使用 Tauri 原生剪贴板接口。

执行器是独立本机进程，远端对话结束、HTTP 断开或主程序意外退出后仍可继续保存记录。主动停止服务、退出应用、撤销对应目录或更新令牌会请求停止执行；重新启动后仍能查询历史。若执行器失联，状态改为 `unknown`，不自动重跑、不猜测额度不足。接手 Agent 应先检查进程、日志和文件改动，再通过 `execution_review(confirm_stopped=true)` 确认已经停止。

旧任务数据保留但不恢复执行；本版本使用独立的执行记录表，不安装或改动全局 codex-delegate skill。

## 换 Agent 接手

Agent A 通过工具启动时，本机先提交执行记录，再启动后台进程。收到 `thread.started` 立即记录准确的 Codex 会话 ID，无需等待整轮结束或依赖远端交接。

```json
{
  "workspace": "cloudmcp",
  "request_id": "implement-directory-history-1",
  "prompt": "实现约定的功能，并执行适用的验证。",
  "acceptance_criteria": "满足需求，并说明实际验证结果和未完成项。",
  "timeout_seconds": 3600,
  "wait_seconds": 1
}
```

以上为 `codex_exec` 参数。相同目录和 `request_id` 重试返回原执行；同编号换内容会被拒绝。普通命令也应传稳定的 `request_id`；旧客户端未传时自动生成编号，此时无法对未收到响应的请求去重。

Agent A 额度耗尽后，Agent B 连接同一个 MCP 地址与令牌，只需知道目录名称：

1. `workspace_list` 确认目录，再 `execution_list({"workspace":"cloudmcp","kind":"codex"})`。
2. 用 `execution_get` 查询记录。运行中则继续观察；已结束则检查摘要、验收条件、测试和截图；状态不明先排查。
3. 需要继续时，调用 `codex_exec`，传原 `session_id`、新的 `request_id` 和后续要求。每个目录及会话同时只允许一个 Codex 执行，避免重复续跑。
4. 检查验收条件后用 `execution_review` 标记 `completed` 或 `needs_work`。正常退出只代表本轮执行结束，默认仍为 `unverified`。

续跑沿用原 `CODEX_HOME`，不使用 `--last` 或 `--ephemeral`；必须保留本机 Codex 会话文件。未传给 Codex 的远端对话不会自动进入会话，因此启动时保存原始要求和验收条件。远端 Agent 与本机 Codex 的额度分别由各自运行环境决定。

## Cloudflare 隧道

临时隧道使用内置的 cloudflared，无需 Cloudflare 账号和域名，会生成临时 HTTPS 地址。重新创建后地址可能变化。固定隧道需要提前配置账号、域名、凭据和路由。

在「设置 → 连接」的可执行文件字段填 `cloudflared` 会优先使用应用包内置版本。也可填写自定义路径。「设置 → 诊断 → 运行环境」显示实际使用的版本和位置。构建时按目标架构下载固定版本、校验 SHA-256 并随应用附上 Apache 2.0 许可证。

参考：[Cloudflare 下载](https://developers.cloudflare.com/tunnel/downloads/)、[临时隧道](https://developers.cloudflare.com/tunnel/get-started/quick-tunnels/)、[MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)。

## 无界面启动

同一可执行文件支持无界面模式：

```sh
cloudmcp --headless /path/to/cloudmcp.json
```

本机配置文件示例：

```json
{
  "data_dir": "~/.local/share/cloudmcp-server",
  "port": 8787,
  "command_timeout_seconds": 3600,
  "codex_path": "/usr/local/bin/codex",
  "token": "REPLACE-WITH-A-RANDOM-TOKEN-AT-LEAST-32-BYTES",
  "directories": [{"name":"my-project","path":"~/projects/my-project"}]
}
```

省略 token 时使用此数据目录中已保存的令牌，首次启动会生成令牌。directories 也兼容路径字符串，支持显式 name；用于在本机添加授权，不会自动撤销其他授权。服务只监听回环地址，公网访问可单独运行 cloudflared。

当前验证的构建平台为 macOS；其他平台需具备其 Tauri 编译与运行依赖，尚未做部署验证。
