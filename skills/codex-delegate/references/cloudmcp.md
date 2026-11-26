# CloudMCP 委派和接手

## 组装任务

通过 `workspace_list` 取得授权目录名称。调用 `codex_exec` 时传 `workspace`、唯一的 `request_id`、`delegation`、`prompt` 和 `acceptance_criteria`。命令和文件相对路径由 CloudMCP 在选定目录内解析。远端调用者用 MCP 工具访问这些文件和运行命令。

- 实现 brief 包含用户目标、已决定的方案、涉及路径、约束、回归范围和能实际运行的验收检查。把用户的最新决定带进去。
- 审查 brief 写清审查对象和范围，给出已有验证，要求报告可定位、可复现的问题。实现后如需独立复审，用一个新会话。需要将方案审查或调查续成实现时，在同一个会话明确传 `delegation="implement"`。
- 返工 brief 写清哪项验收失败、命令和相关输出、观察到的行为、需要改动的范围，以及应该保留的已有工作。明确告诉 Codex 调用者对上一轮建议的决定。
- 需要只读沙箱审查时，使用支持 `delegation="review"` 的 CloudMCP。旧版 `codex_exec` 始终使用本机完整权限，可在 `prompt` 开头附上 [实现约束](../assets/implement.md) 或 [审查约束](../assets/review.md) 来传达角色要求。

## 读取和修改文件

- 修改已有文件优先用 `fs_edit`：传 `workspace`、相对 `path`、`old_string` 和 `new_string`。`old_string` 须与文件内容逐字节一致，默认恰好匹配一处；确实需要替换全部匹配时传 `replace_all=true`。文件修改前后均最多 1 MiB。新建或整文件重写才用 `fs_write`。
- 定位用 `fs_read` 行模式：传 `line`（从 1 开始）和可选的 `limit`，按 `next_line` 翻页。提取文本时只去掉行号及分隔制表符，保留正文缩进。行模式会去掉 CRLF 中的 CR、截断超长行，并为没有结尾换行的末行补上 LF；涉及换行或长行时，用字节模式（不传 `line`，用 `offset` / `max_bytes`）核对后再填写 `old_string`。两种模式都可能将本机主目录前缀显示为 `~`。

## 验证限制

- `command_exec` 运行在目录沙箱里，清除继承环境并设置固定 `PATH`，用户目录安装的工具链（如 cargo）可能无法调用。临时文件使用工具分配的 `$TMPDIR`。
- `delegation="review"` 的只读沙箱通常无法写构建目录（如 `target/`）和系统临时目录，`cargo test`、`cargo check` 会在加锁或建临时目录时失败。审查轮可做静态审查、只读类型检查、`rustfmt --check` 和 `git diff --check`。
- 当前环境因工具链或沙箱限制无法运行的必要检查（包括 `cargo test` 等写入型检查），另发一轮 `delegation="implement"`，只运行指定检查，写入限于检查产生的构建和临时产物。结束后比对前后的工作树 diff、暂存区 diff 和相关未跟踪文件内容；也可请用户在本机运行。报告里区分“已实际运行”和“仅静态核对”。

## 等待、核实和重试

`wait_seconds` 只控制当前 MCP 请求等待多久。收到 `run_id` 后用 `execution_wait` 等待，不要反复轮询 `execution_get`。`timeout_seconds` 是轮询预算（默认 30、最多 50 秒），末次查询可略超时；执行一离开 starting / running 就返回。仅在状态为 `starting` 或 `running` 时继续等待。`finished` 仅在确认终态时为 `true`；遇到 `unknown` 会立即返回 `finished=false`，此时先按下文核实进程、日志和改动。结束后用 `execution_get` 读结果，必要时通过 `execution_output` 分页读取日志和摘要。真实应用的截图可用 `fs_image` 读取，测试报告可用 `fs_read`。终态和成功退出只描述这次执行，调用者仍需通过实际代码和证据验收。

- 网络响应丢失时，先查 `execution_list`，或用相同 `request_id` 和相同参数重试。需要真正开始新一轮时才用新 `request_id`。
- 同一授权目录有一轮在启动或运行时，等待并检查那一轮。`unknown` 需要先检查进程、日志和改动，确认停止后才用 `execution_review` 登记 `confirm_stopped=true` 和核实说明。
- 续跑使用已登记的 `session_id`。验收记录写当前 `run_id`。接手后看原始要求、验收条件及上一轮的 `verification_note`，据此继续，不随意选最新会话代替原任务。
- 确认实际的进程错误和工具响应后再决定恢复方式。可恢复的错误在本次任务中处理并复验。如果 `session_id` 尚无记录，先确认原轮次已经停止并检查残留工作，再决定如何恢复。
