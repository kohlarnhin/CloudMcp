export interface Scope {
  id: string;
  name: string;
  path: string;
  created_at: string;
}

export interface Settings {
  port: number;
  cloudflared_path: string;
  tunnel_mode: 'quick' | 'named';
  tunnel_name: string;
  public_url: string;
  result_chars: number;
  command_timeout_seconds: number;
  codex_enabled: boolean;
  codex_path: string;
}

export type ExecutionStatus = 'starting' | 'running' | 'succeeded' | 'failed' | 'cancelled' | 'timed_out' | 'unknown' | 'interrupted';
export type ExecutionVerification = 'unverified' | 'completed' | 'needs_work';
export type ExecutionKind = 'shell' | 'codex';

export interface Execution {
  run_id: string;
  workspace: string;
  request_id: string;
  kind: ExecutionKind;
  status: ExecutionStatus;
  session_id: string | null;
  verification: ExecutionVerification;
  created_at: string;
  updated_at: string;
  heartbeat_at: string;
  ended_at: string | null;
  timeout_seconds: number;
  summary: string;
  request_brief: string;
  acceptance_criteria: string;
  verification_note: string;
  evidence_paths: string[];
  error: string | null;
  full_access: boolean;
  command?: string;
  prompt?: string;
  stdout?: string;
  stderr?: string;
  cancel_requested?: boolean;
}

export interface Audit {
  id: number;
  time: string;
  action: string;
  target: string;
  success: boolean;
}

export interface Snapshot {
  settings: Settings;
  scopes: Scope[];
  audit: Audit[];
  executions: Execution[];
  server_running: boolean;
  tunnel_running: boolean;
  public_url: string | null;
  tunnel_error: string | null;
  data_dir: string;
  platform: string;
}

export interface Dependency {
  name: string;
  path: string;
  available: boolean;
  version: string;
}

export interface PortalAccount {
  name: string;
  usable: boolean;
  shortRemainingPercent: number;
  shortResetsAt: string | null;
  longRemainingPercent: number;
  longResetsAt: string | null;
}

/** Codex Portal 本地 IPC `status` 的返回值（只读）。 */
export interface PortalStatus {
  current: PortalAccount | null;
  next: PortalAccount | null;
  allExhausted: boolean;
  earliestRecoveryAt: string | null;
}
