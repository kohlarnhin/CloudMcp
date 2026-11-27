use crate::{
    executions, files, portal, privacy,
    store::{AppResult, Store},
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock as Content, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router, ServerHandler,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Tools that only exist while Codex delegation is enabled in the local settings.
const CODEX_TOOLS: [&str; 2] = ["codex_exec", "codex_portal"];

#[derive(Clone)]
pub struct Gateway {
    store: Arc<Store>,
    codex_enabled: bool,
    pi_enabled: bool,
    tool_router: ToolRouter<Self>,
}
impl Gateway {
    pub fn new(store: Arc<Store>) -> Self {
        // Settings can only change while the server is stopped, so each session reads them once.
        let config = store.config();
        let codex_enabled = config.codex_enabled;
        let pi_enabled = config.pi_enabled();
        let mut tool_router = Self::tool_router();
        if !pi_enabled {
            tool_router.remove_route("pi_exec");
        }
        if !codex_enabled {
            for name in CODEX_TOOLS {
                tool_router.remove_route(name);
            }
            if let Some(route) = tool_router.map.get_mut("command_exec") {
                route.attr.description = Some(COMMAND_EXEC_DIRECT.into());
            }
        }
        Self {
            store,
            codex_enabled,
            pi_enabled,
            tool_router,
        }
    }
    fn result(&self, action: &str, target: &str, result: AppResult<Value>) -> CallToolResult {
        self.store.audit(action, target, result.is_ok());
        match result {
            Ok(v) => CallToolResult::structured(privacy::value(v)),
            Err(e) => CallToolResult::error(vec![Content::text(privacy::text(&e))]),
        }
    }
}
#[derive(Deserialize, schemars::JsonSchema)]
struct FileArgs {
    #[serde(alias = "scope_id")]
    workspace: String,
    #[serde(default = "dot")]
    path: String,
    offset: Option<u64>,
    /// Accepted for compatibility; fs_list and fs_image ignore it.
    #[allow(dead_code)]
    max_bytes: Option<usize>,
}
#[derive(Deserialize, schemars::JsonSchema)]
struct ReadArgs {
    #[serde(alias = "scope_id")]
    workspace: String,
    #[serde(default = "dot")]
    path: String,
    /// Byte mode: byte offset to start from. Cannot be combined with line.
    offset: Option<u64>,
    /// Byte mode: bytes to read, at most 32768 (default 12000).
    max_bytes: Option<usize>,
    /// Line mode: first line to read, starting at 1. Returns numbered lines.
    line: Option<u64>,
    /// Line mode: number of lines to read, default 400, at most 2000.
    limit: Option<usize>,
}
fn dot() -> String {
    ".".into()
}
#[derive(Deserialize, schemars::JsonSchema)]
struct WriteArgs {
    #[serde(alias = "scope_id")]
    workspace: String,
    path: String,
    content: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
struct EditArgs {
    #[serde(alias = "scope_id")]
    workspace: String,
    path: String,
    old_string: String,
    new_string: String,
    replace_all: Option<bool>,
}
#[derive(Deserialize, schemars::JsonSchema)]
struct MutateArgs {
    #[serde(alias = "scope_id")]
    workspace: String,
    action: String,
    path: String,
    destination: Option<String>,
    recursive: Option<bool>,
}
#[derive(Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    #[serde(alias = "scope_id")]
    workspace: String,
    #[serde(default = "dot")]
    path: String,
    query: String,
}
const COMMAND_EXEC_DIRECT: &str = "Start a durably recorded shell command in an authorized directory, identified by workspace name. Codex delegation is off, so this is the build and delivery tool: the macOS sandbox keeps writes to authorized directories, toolchain caches and temp directories, while HOME is the user's real home with Git/SSH config, Rust (cargo, rustup), Node (pnpm, npm, yarn, bun) and Xcode command line tools available. Execution timeout defaults to the local setting (1 hour), up to 86400 seconds. wait_seconds only controls this response (0–10 seconds); a running process continues independently of HTTP and remote-agent disconnection; use execution_wait for long builds and tests. Reuse request_id on retries. Returns run_id, status and bounded output.";

#[tool_router]
impl Gateway {
    #[tool(
        description = "Start a durably recorded shell command in an authorized directory, identified by workspace name. Uses the macOS system sandbox; HOME is the workspace. Execution timeout defaults to the local setting (1 hour), up to 86400 seconds. wait_seconds only controls this response (0–10 seconds); a running process continues independently of HTTP and remote-agent disconnection. Reuse request_id on retries. Returns run_id, status and bounded output. Query execution_list/get/output to take over from another agent. Use codex_exec for persistent full-access Codex sessions."
    )]
    async fn command_exec(
        &self,
        Parameters(p): Parameters<executions::CommandArgs>,
    ) -> CallToolResult {
        let id = p.workspace.clone();
        self.result(
            "command.exec",
            &id,
            executions::command(self.store.clone(), p).await,
        )
    }
    #[tool(
        description = "Start or resume local Codex from an authorized workspace name. Codex has full host access by default. Set delegation to implement for a delegated implementation or review for read-only analysis and review. Omit session_id for a new session or use a registered ID from execution_list/get to resume. A resumed run inherits its prior role when delegation is omitted. CloudMCP saves the exact session_id from thread.started. One active Codex execution per directory/session. Fetch execution_get before acceptance, since a successful exit only describes the process. Reuse request_id for retries of the same operation."
    )]
    async fn codex_exec(&self, Parameters(p): Parameters<executions::CodexArgs>) -> CallToolResult {
        let workspace = p.workspace.clone();
        self.result(
            "codex.exec",
            &workspace,
            executions::codex(self.store.clone(), p).await,
        )
    }
    #[tool(
        description = "Start or resume the local Pi coding agent (pi.dev) in an authorized workspace. Pi runs with the user's own Pi configuration: models, API keys, settings, MCP servers and codemode from ~/.pi, and its read/write/edit/bash tools have full access to this machine. Omit session_id for a new session; CloudMCP assigns the session ID and returns it immediately. To continue, pass a registered session_id from execution_list/get with a new request_id once the previous run has stopped. One active Pi execution per directory/session. Follow with execution_wait; the summary holds Pi's final answer. Verify the diff and checks yourself before acceptance, then record it with execution_review. Reuse request_id for retries of the same operation."
    )]
    async fn pi_exec(&self, Parameters(p): Parameters<executions::PiArgs>) -> CallToolResult {
        let workspace = p.workspace.clone();
        self.result(
            "pi.exec",
            &workspace,
            executions::pi(self.store.clone(), p).await,
        )
    }
    #[tool(
        description = "Query local Codex Portal quota with action=status or request a smart account switch with action=switch. Portal applies its own rule and chooses the usable account whose short-cycle window expires earliest. When the current account is still usable, Portal usually keeps it active (switched=false). When allExhausted=true, wait until earliestRecoveryAt before calling switch again. If Portal reports ‘正在执行或等待重试’, a switch is already running or pending retry. Wait before trying again."
    )]
    async fn codex_portal(&self, Parameters(p): Parameters<portal::Args>) -> CallToolResult {
        let action = p.action;
        self.result("codex.portal", action.as_str(), portal::call(action).await)
    }
    #[tool(
        description = "List persistent executions for an authorized directory NAME, newest first, with pagination. Start here when switching remote agents: find run_id, session_id, execution status, acceptance criteria and verification. A remote-agent quota failure does not imply local Codex stopped. Running executions should be observed; completed executions should be checked before resuming. Unknown means the local runner needs inspection, not a confirmed quota failure."
    )]
    fn execution_list(&self, Parameters(p): Parameters<executions::ListArgs>) -> CallToolResult {
        let workspace = p.workspace.clone();
        self.result(
            "execution.list",
            &workspace,
            executions::list(&self.store, p),
        )
    }
    #[tool(
        description = "Read one execution's current status, explicit Codex session_id, original request, acceptance criteria, verification, final summary and bounded output tails. The original prompt and logs can be read in pages through execution_output. Never infer business completion from a zero exit code alone."
    )]
    fn execution_get(&self, Parameters(p): Parameters<executions::RunArgs>) -> CallToolResult {
        let id = p.run_id.clone();
        self.result("execution.get", &id, executions::get(&self.store, p))
    }
    #[tool(
        description = "Read a bounded byte-offset page of locally persisted stdout, stderr, prompt, acceptance_criteria, summary, or runner startup diagnostics (worker). At most 32 KiB per call; stdout/stderr logs retain their first 16 MiB, while execution_get supplies current bounded tails. Paths are normalized to ~/ on return. Use fs_image with evidence_paths to fetch screenshots only when needed."
    )]
    fn execution_output(
        &self,
        Parameters(p): Parameters<executions::OutputArgs>,
    ) -> CallToolResult {
        let id = p.run_id.clone();
        self.result("execution.output", &id, executions::output(&self.store, p))
    }
    #[tool(
        description = "Wait for a recorded execution to stop running, up to timeout_seconds (default 30, max 50; 0 checks once). Returns as soon as the status leaves starting/running, with the same fields as execution_get plus finished and waited_seconds. timeout_seconds is the polling budget: no new status check starts after it, though the last check may finish slightly later. finished is true only for a confirmed end state; an unknown status also returns at once with finished false, so inspect the processes before acceptance. Use it instead of polling execution_get for long commands and Codex runs; call again while the status is still running."
    )]
    async fn execution_wait(
        &self,
        Parameters(p): Parameters<executions::WaitArgs>,
    ) -> CallToolResult {
        let id = p.run_id.clone();
        self.result(
            "execution.wait",
            &id,
            executions::wait(self.store.clone(), p).await,
        )
    }
    #[tool(
        description = "Request cancellation of a recorded local execution. The local worker stops its process group and persists the outcome. File changes are retained. Unknown executions still require inspection before they can be resumed or marked verified."
    )]
    fn execution_cancel(&self, Parameters(p): Parameters<executions::RunArgs>) -> CallToolResult {
        let id = p.run_id.clone();
        self.result("execution.cancel", &id, executions::cancel(&self.store, p))
    }
    #[tool(
        description = "Record acceptance separately from execution state: unverified, completed, or needs_work, with an inspection note and optional relative evidence_paths. Running executions cannot be marked complete. To resolve an unknown execution, inspect processes, logs and changes first, then explicitly set confirm_stopped=true; recorded processes must have stopped. Do not label an unknown disconnect as quota exhaustion without evidence."
    )]
    fn execution_review(
        &self,
        Parameters(p): Parameters<executions::ReviewArgs>,
    ) -> CallToolResult {
        let id = p.run_id.clone();
        self.result("execution.review", &id, executions::review(&self.store, p))
    }
    #[tool(
        description = "List locally authorized directories. Use the unique directory name as workspace for file, command and execution tools; legacy scope_id/UUID inputs remain accepted for file tools. Public paths use ~/ instead of the local username. Only the local desktop app can change grants or directory names. When taking over from another agent, call execution_list with the chosen directory name."
    )]
    fn workspace_list(&self) -> CallToolResult {
        self.result("workspace.list","",self.store.scopes().map(|s|json!({"workspaces":s,"permissions":["read","write","create","rename","delete","execute"]})))
    }
    #[tool(
        description = "List a directory using its authorized workspace name and a relative path. Bounded to 100 entries. offset is a directory pagination offset."
    )]
    fn fs_list(&self, Parameters(p): Parameters<FileArgs>) -> CallToolResult {
        self.result(
            "fs.list",
            &p.path,
            files::list(
                &self.store,
                &p.workspace,
                &p.path,
                p.offset.unwrap_or(0) as usize,
            ),
        )
    }
    #[tool(
        description = "Read UTF-8 text from an authorized directory, at most 32 KiB per call. Line mode: set line (1-based) and optional limit (default 400, max 2000) to get lines prefixed with right-aligned line numbers and a tab, plus start_line, end_line, next_line and total_lines when the file is at most 8 MiB. A line over 2000 characters is cut and marked; truncated only says whether next_line has more. line_ending is crlf when a returned line ends with CRLF. Line mode rejects invalid UTF-8. Line numbers are only for locating text; never include them in fs_edit old_string. Byte mode: omit line and use offset/max_bytes for byte pagination with next_offset. offset and line cannot be combined."
    )]
    async fn fs_read(&self, Parameters(p): Parameters<ReadArgs>) -> CallToolResult {
        let result = match (p.line, p.offset) {
            (Some(_), Some(_)) => {
                Err("offset 与 line 只能二选一：offset 用于字节模式，line 用于行模式".to_owned())
            }
            (Some(line), None) => {
                let store = self.store.clone();
                let (workspace, path, limit) = (p.workspace.clone(), p.path.clone(), p.limit);
                tokio::task::spawn_blocking(move || {
                    files::read_lines(&store, &workspace, &path, line, limit)
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
            }
            (None, offset) => files::read(
                &self.store,
                &p.workspace,
                &p.path,
                offset.unwrap_or(0),
                p.max_bytes.unwrap_or(12000),
            ),
        };
        self.result("fs.read", &p.path, result)
    }
    #[tool(
        description = "Read a PNG/JPEG/WebP image inside an authorized workspace as MCP image content, at most 2 MiB. Uses workspace name and a relative file path; offset/max_bytes are ignored for images."
    )]
    fn fs_image(&self, Parameters(p): Parameters<FileArgs>) -> CallToolResult {
        match files::image(&self.store, &p.workspace, &p.path) {
            Ok(v) => {
                self.store.audit("fs.image", &p.path, true);
                CallToolResult::success(vec![Content::image(
                    v["data"].as_str().unwrap_or_default(),
                    v["mime_type"].as_str().unwrap_or("image/png"),
                )])
            }
            Err(e) => self.result("fs.image", &p.path, Err(e)),
        }
    }
    #[tool(
        description = "Atomically create or replace a text file inside an authorized directory; at most 1 MiB. Parent directories must already exist."
    )]
    fn fs_write(&self, Parameters(p): Parameters<WriteArgs>) -> CallToolResult {
        self.result(
            "fs.write",
            &p.path,
            files::write(&self.store, &p.workspace, &p.path, &p.content),
        )
    }
    #[tool(
        description = "Precisely replace literal text in an existing UTF-8 file using an authorized workspace and a relative path. By default old_string must match exactly once. Set replace_all=true to replace every match. The file must be at most 1 MiB before and after editing. Prefer fs_edit over fs_write for changes to existing files."
    )]
    fn fs_edit(&self, Parameters(p): Parameters<EditArgs>) -> CallToolResult {
        self.result(
            "fs.edit",
            &p.path,
            files::edit(
                &self.store,
                &p.workspace,
                &p.path,
                &p.old_string,
                &p.new_string,
                p.replace_all.unwrap_or(false),
            ),
        )
    }
    #[tool(
        description = "Create a directory (mkdir), rename a file/directory (rename), or delete it (remove). Relative paths only. recursive is explicit and defaults to false."
    )]
    fn fs_mutate(&self, Parameters(p): Parameters<MutateArgs>) -> CallToolResult {
        self.result(
            "fs.mutate",
            &p.path,
            files::mutate(
                &self.store,
                &p.workspace,
                &p.action,
                &p.path,
                p.destination.as_deref(),
                p.recursive.unwrap_or(false),
            ),
        )
    }
    #[tool(
        description = "Literal text search within a workspace. At most 80 matches and 3000 files; skips dependency/build directories, symlinks and large/binary files. Use specific subdirectories for large projects."
    )]
    async fn fs_search(&self, Parameters(p): Parameters<SearchArgs>) -> CallToolResult {
        let store = self.store.clone();
        let target = p.path.clone();
        let result = tokio::task::spawn_blocking(move || {
            files::search(&store, &p.workspace, &p.path, &p.query)
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
        self.result("fs.search", &target, result)
    }
}
const INSTRUCTIONS_DIRECT: &str = concat!(
    "CloudMCP exposes local filesystem tools and persistent background commands to remote agents. ",
    "Codex delegation is turned off on this machine: there is no codex_exec, and you, the calling agent, read, edit, build, test, commit and release directly. ",
    "Start with workspace_list and use the authorized directory NAME as workspace. ",
    "Prefer fs_edit when modifying existing files; read with fs_read line mode to locate the text first, and fs_search to find code. ",
    "command_exec runs with the developer toolchain: Git with the user's config and SSH keys, cargo/rustup, pnpm/npm/yarn/bun, and Xcode command line tools, all inside a sandbox that only writes to authorized directories, toolchain caches and temp directories. ",
    "Start long builds and tests with command_exec and follow them with execution_wait and execution_output instead of polling. ",
    "Verify the diff and the project's checks yourself before committing; commit, push or release only when the user asked for it. ",
    "When switching agents, query execution_list for the directory and inspect execution_get. Reuse request_id when retrying lost responses. ",
    "Public home paths use ~/ as an alias for this machine's home directory. ",
    "Use relative file paths, paginated logs and fs_image to retrieve only needed evidence."
);

const INSTRUCTIONS_PI: &str = concat!(
    "CloudMCP exposes local filesystem tools, persistent background commands and the local Pi coding agent to remote agents. ",
    "Start with workspace_list and use the authorized directory NAME as workspace. ",
    "Delegate implementation work to Pi with pi_exec: give the goal, constraints and acceptance criteria, then follow it with execution_wait. ",
    "Pi uses the user's own models, keys, MCP servers and codemode, and works with full access on this machine. ",
    "Keep decisions with yourself: after each Pi round, inspect the diff with fs_read/fs_search or command_exec (git diff) and run the project's checks. ",
    "If acceptance fails, record needs_work with execution_review and resume the same Pi session_id with a new request_id and the confirmed findings. ",
    "Small edits can be made directly with fs_edit; command_exec runs with the developer toolchain (Git with the user's config, cargo, pnpm/npm, Xcode tools) in a sandbox that writes only to authorized directories, toolchain caches and temp directories. ",
    "Commit, push or release only when the user asked for it. ",
    "When switching agents, query execution_list for the directory and inspect execution_get. Reuse request_id when retrying lost responses. ",
    "Public home paths use ~/ as an alias for this machine's home directory. ",
    "Use relative file paths, paginated logs and fs_image to retrieve only needed evidence."
);

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Gateway {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.server_info = rmcp::model::Implementation::new("cloudmcp", env!("CARGO_PKG_VERSION"));
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        if self.pi_enabled {
            info.instructions = Some(INSTRUCTIONS_PI.into());
            return info;
        }
        if !self.codex_enabled {
            info.instructions = Some(INSTRUCTIONS_DIRECT.into());
            return info;
        }
        info.instructions = Some(
            concat!(
                "CloudMCP exposes local filesystem tools and persistent background executions to remote agents. ",
                "Start with workspace_list and use the authorized directory NAME as workspace. ",
                "Prefer fs_edit when modifying existing files; read with fs_read line mode to locate the text first. ",
                "Use execution_wait instead of repeatedly calling execution_get while a command or Codex run is in progress. ",
                "When switching agents, query execution_list for the directory and inspect execution_get. ",
                "Resume the exact registered Codex session_id only after the previous execution has stopped. ",
                "Reuse request_id when retrying lost responses. command_exec uses the macOS directory sandbox. ",
                "codex_exec uses local Codex credentials and gives full host access by default. Use delegation=review for read-only review. ",
                "Execution records, session IDs, and local process state are independent of remote connections and account limits. ",
                "For development delivery with codex-delegate, use delegation=implement to leave the changes in the worktree. The calling agent owns decisions, tags, commits, pushes, releases and deployments. ",
                "Independently verify the diff and applicable tests after each round. If acceptance fails, record needs_work with execution_review, then send the confirmed findings to codex_exec with the same session_id and a new request_id. Verify again after rework. ",
                "On acceptance, record completed and perform the authorized scoped Git commit through command_exec. When release is requested and authorized, run the project's release workflow as the caller. Finish after confirming acceptance and the requested commit or release results, or report a concrete blocker. ",
                "For an unknown execution, inspect the process and explicitly confirm it has stopped before starting another round. ",
                "Public home paths use ~/ as an alias for this machine's home directory. ",
                "Use relative file paths, paginated logs and fs_image to retrieve only needed evidence."
            )
            .into(),
        );
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn fs_edit_registers_parameters_defaults_to_unique_match_and_audits_results() {
        let router = Gateway::tool_router();
        let tool = router.get("fs_edit").unwrap();
        let properties = tool.input_schema["properties"].as_object().unwrap();
        assert_eq!(properties.len(), 5);
        for name in [
            "workspace",
            "path",
            "old_string",
            "new_string",
            "replace_all",
        ] {
            assert!(properties.contains_key(name));
        }

        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("file.txt"), "old\nold\n").unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        let scope = store.add_scope(&project, Some("project")).unwrap();
        let gateway = Gateway::new(store.clone());
        let args = serde_json::from_value(json!({
            "scope_id": scope.id,
            "path": "file.txt",
            "old_string": "old",
            "new_string": "new"
        }))
        .unwrap();
        let result = gateway.fs_edit(Parameters(args));
        assert_eq!(result.is_error, Some(true));
        assert!(result.content[0].as_text().unwrap().text.contains("2 处"));

        let args = serde_json::from_value(json!({
            "workspace": "project",
            "path": "file.txt",
            "old_string": "old",
            "new_string": "new",
            "replace_all": true
        }))
        .unwrap();
        let result = gateway.fs_edit(Parameters(args));
        assert_eq!(
            result.structured_content,
            Some(json!({"replacements":2,"written_bytes":8,"first_line":1}))
        );
        let audits = store.audits().unwrap();
        assert_eq!(audits[0].action, "fs.edit");
        assert_eq!(audits[0].target, "file.txt");
        assert!(audits[0].success);
        assert_eq!(audits[1].action, "fs.edit");
        assert!(!audits[1].success);
    }
    #[tokio::test]
    async fn fs_read_line_mode_and_offset_are_exclusive() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("file.txt"), "a\nb\n").unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        store.add_scope(&project, Some("project")).unwrap();
        let gateway = Gateway::new(store.clone());

        let args = serde_json::from_value(json!({
            "workspace": "project",
            "path": "file.txt",
            "line": 2
        }))
        .unwrap();
        let result = gateway.fs_read(Parameters(args)).await;
        let value = result.structured_content.unwrap();
        assert_eq!(value["text"], json!("     2\tb\n"));
        assert_eq!(value["total_lines"], json!(2));

        let args = serde_json::from_value(json!({
            "workspace": "project",
            "path": "file.txt",
            "line": 1,
            "offset": 0
        }))
        .unwrap();
        let result = gateway.fs_read(Parameters(args)).await;
        assert_eq!(result.is_error, Some(true));
        assert!(result.content[0].as_text().unwrap().text.contains("二选一"));
    }

    #[test]
    fn codex_toggle_controls_tools_and_instructions() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        let enabled = Gateway::new(store.clone());
        assert!(enabled.tool_router.has_route("codex_exec"));
        assert!(enabled.tool_router.has_route("codex_portal"));
        assert!(enabled
            .get_info()
            .instructions
            .unwrap()
            .contains("codex-delegate"));

        let mut settings = store.config();
        settings.local_agent = "none".into();
        settings.codex_path = String::new();
        store.save_settings(settings).unwrap();
        let direct = Gateway::new(store.clone());
        for name in CODEX_TOOLS {
            assert!(!direct.tool_router.has_route(name));
        }
        assert!(direct.tool_router.has_route("command_exec"));
        let names: Vec<_> = direct
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert!(!names.iter().any(|n| n.starts_with("codex")));
        let description = direct
            .tool_router
            .get("command_exec")
            .unwrap()
            .description
            .clone()
            .unwrap();
        assert!(!description.contains("codex_exec"));
        let instructions = direct.get_info().instructions.unwrap();
        assert!(
            !instructions.contains("codex_exec uses") && !instructions.contains("codex-delegate")
        );
        assert!(instructions.contains("calling agent"));
    }

    #[test]
    fn pi_choice_offers_pi_exec_without_codex() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        assert!(!Gateway::new(store.clone()).tool_router.has_route("pi_exec"));
        let mut settings = store.config();
        settings.local_agent = "pi".into();
        settings.pi_path = "pi".into();
        store.save_settings(settings).unwrap();
        assert!(!store.config().codex_enabled);
        let gateway = Gateway::new(store.clone());
        assert!(gateway.tool_router.has_route("pi_exec"));
        assert!(!gateway.tool_router.has_route("codex_exec"));
        assert!(gateway.get_info().instructions.unwrap().contains("pi_exec"));
    }

    #[test]
    fn execution_wait_is_registered() {
        let router = Gateway::tool_router();
        let tool = router.get("execution_wait").unwrap();
        let properties = tool.input_schema["properties"].as_object().unwrap();
        for name in ["workspace", "run_id", "timeout_seconds"] {
            assert!(properties.contains_key(name));
        }
    }
}
