import { Channel, invoke } from "@tauri-apps/api/core";

export type Project = {
  id: number;
  name: string;
  path: string;
  kamal_bin: string | null;
  default_destination: string | null;
  created_at: number;
};
export type ProjectConfig = {
  service: string;
  repository: string;
  version: string;
  roles: string[];
  hosts: string[];
  primary_host: string | null;
  ssh_user: string;
  ssh_port: number;
  accessories: Accessory[];
};
export type Accessory = { name: string; hosts: string[] };
export type ProxyRoute = { service: string; host: string; path: string; target: string; state: string; tls: boolean };
export type ProxyRequest = {
  time: string;
  service: string;
  method: string;
  host: string;
  path: string;
  status: number;
  duration_ms: number;
  request_id: string;
};
export type ConsoleEvent = { kind: "output"; data: string } | { kind: "exit" };
export type ProjectInfo = { destinations: string[]; destination: string | null; has_base_config: boolean; config: ProjectConfig };
export type Container = {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  created_at: string;
  service: string | null;
  role: string | null;
  destination: string | null;
  stale: boolean;
  cpu_pct: number | null;
  mem_bytes: number | null;
};
export type HostMetrics = {
  cpu_pct: number | null;
  load1: number;
  load5: number;
  load15: number;
  mem_used: number;
  mem_total: number;
  disk_used: number;
  disk_total: number;
};
export type HostSnapshot = { key: string; ts: number; metrics: HostMetrics | null; containers: Container[]; error: string | null };
export type SshTarget = { host: string; port: number; user: string };
export type LogOptions = { lines?: number; since?: string; grep?: string };
export type LogEvent = { kind: "line"; text: string } | { kind: "end"; code: number | null };

export const hostKey = (t: SshTarget) => `${t.user}@${t.host}:${t.port}`;
export type Run = {
  id: number;
  project_id: number;
  destination: string | null;
  command: string;
  args_json: string;
  git_sha: string | null;
  started_at: number;
  finished_at: number | null;
  exit_code: number | null;
};
export type KamalCommand =
  | { kind: "deploy" | "redeploy" | "lock_release" | "lock_status" | "app_boot" | "app_stop" }
  | { kind: "rollback"; version: string }
  | { kind: "lock_acquire"; message: string };
export type RunEvent = { kind: "line"; stream: "stdout" | "stderr"; text: string } | { kind: "exit"; code: number | null };

export const api = {
  projectAdd: (path: string) => invoke<Project>("project_add", { path }),
  projectList: () => invoke<Project[]>("project_list"),
  projectRemove: (id: number) => invoke<void>("project_remove", { id }),
  projectConfig: (id: number, destination: string | null) => invoke<ProjectInfo>("project_config", { id, destination }),
  runStart: (projectId: number, destination: string | null, command: KamalCommand, onEvent: Channel<RunEvent>) =>
    invoke<number>("run_start", { projectId, destination, command, onEvent }),
  clearDockerLogin: (server: string) => invoke<number>("clear_docker_login", { server }),
  runCancel: (runId: number, force: boolean) => invoke<void>("run_cancel", { runId, force }),
  runsList: (projectId: number, limit = 50) => invoke<Run[]>("runs_list", { projectId, limit }),
  runLog: (runId: number) => invoke<string>("run_log", { runId }),
  hostWatch: (target: SshTarget) => invoke<HostSnapshot | null>("host_watch", { target }),
  hostUnwatch: (target: SshTarget) => invoke<void>("host_unwatch", { target }),
  logsSubscribe: (target: SshTarget, container: string, opts: LogOptions, onEvent: Channel<LogEvent>) =>
    invoke<number>("logs_subscribe", { target, container, opts, onEvent }),
  logsUnsubscribe: (subId: number) => invoke<void>("logs_unsubscribe", { subId }),
  proxyRoutes: (target: SshTarget) => invoke<ProxyRoute[]>("proxy_routes", { target }),
  proxyRequestsSubscribe: (target: SshTarget, services: string[], since: string, onEvent: Channel<ProxyRequest>) =>
    invoke<number>("proxy_requests_subscribe", { target, services, since, onEvent }),
  consoleOpen: (projectId: number, destination: string | null, rows: number, cols: number, onEvent: Channel<ConsoleEvent>) =>
    invoke<number>("console_open", { projectId, destination, rows, cols, onEvent }),
  consoleWrite: (sessionId: number, data: string) => invoke<void>("console_write", { sessionId, data }),
  consoleResize: (sessionId: number, rows: number, cols: number) => invoke<void>("console_resize", { sessionId, rows, cols }),
  consoleClose: (sessionId: number) => invoke<void>("console_close", { sessionId }),
};
