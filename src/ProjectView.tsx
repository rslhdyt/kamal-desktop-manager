import { useCallback, useEffect, useState } from "react";
import { Badge, Button, cn, Select, Tabs, Text } from "@cloudflare/kumo";
import {
  ArrowClockwiseIcon,
  KeyIcon,
  LockIcon,
  LockOpenIcon,
  PlayIcon,
  RocketLaunchIcon,
  StopIcon,
  TrashIcon,
  XCircleIcon,
} from "@phosphor-icons/react";
import { api, KamalCommand, Project, ProjectInfo, Run } from "./api";
import { HostCard } from "./HostCard";
import { ContainersPanel, HostEntry } from "./ContainersPanel";
import { LogSource, LogsPanel } from "./LogsPanel";
import { ProxyPanel } from "./ProxyPanel";
import { ConsolePanel } from "./ConsolePanel";
import { ConfirmRequest } from "./Confirm";
import { ErrorNotice } from "./errors";
import { isLive, startRun } from "./liveRuns";
import { RunTerminal } from "./RunTerminal";
import { SecretsDialog } from "./SecretsDialog";

// The base config (no destination) is usually production for single-destination apps.
const isProduction = (destination: string | null) => destination === null || /prod/i.test(destination);

const DESTRUCTIVE = new Set(["deploy", "redeploy", "rollback", "app_stop", "app_boot", "lock_release"]);

// Select values can't be null; stands in for "no -d flag".
const BASE = "__base__";

const label = (c: KamalCommand) => c.kind.replace("_", " ") + ("version" in c ? ` ${c.version.slice(0, 12)}` : "");

function formatTime(ms: number) {
  return new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

function runStatus(run: Run): { text: string; variant: "success" | "error" | "warning" | "info" } {
  if (run.finished_at === null) return isLive(run.id) ? { text: "running", variant: "info" } : { text: "interrupted", variant: "warning" };
  if (run.exit_code === null) return { text: "cancelled", variant: "warning" };
  return run.exit_code === 0 ? { text: "ok", variant: "success" } : { text: `exit ${run.exit_code}`, variant: "error" };
}

type Tab = "containers" | "runs" | "logs" | "proxy" | "console";

type Props = { project: Project; confirm: (r: ConfirmRequest) => void; onRemove: () => void };

export function ProjectView({ project, confirm, onRemove }: Props) {
  const [destination, setDestination] = useState(project.default_destination);
  const [info, setInfo] = useState<ProjectInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [tab, setTab] = useState<Tab>("containers");
  const [logSource, setLogSource] = useState<LogSource | null>(null);
  const [runs, setRuns] = useState<Run[]>([]);
  const [selectedRun, setSelectedRun] = useState<number | null>(null);
  const [secretsOpen, setSecretsOpen] = useState(false);

  const refreshRuns = useCallback(() => api.runsList(project.id).then(setRuns), [project.id]);

  const load = useCallback(
    async (dest: string | null) => {
      setLoading(true);
      setError(null);
      try {
        const info = await api.projectConfig(project.id, dest);
        setInfo(info);
        setDestination(info.destination);
      } catch (e) {
        setInfo(null);
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [project.id],
  );

  useEffect(() => {
    load(destination);
    refreshRuns();
    // Only on project switch; destination changes call load directly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.id]);

  const running = runs.some((r) => r.finished_at === null && isLive(r.id) && r.destination === destination);
  const activeRun = runs.find((r) => r.id === selectedRun && isLive(r.id));
  const service = info?.config.service ?? project.name;
  // Confirmations always name where the action lands (PRD: destructive actions name the destination).
  const target = destination ?? "the base config (production)";
  const prod = isProduction(destination);
  // Standalone configs (`-c config/deploy.<dest>.yml`) run without a kamal destination,
  // so their containers carry no destination label or name suffix.
  const kamalDestination = info?.has_base_config === false ? null : destination;

  async function execute(command: KamalCommand) {
    try {
      const id = await startRun(project.id, destination, command, refreshRuns);
      setSelectedRun(id);
      setTab("runs");
      refreshRuns();
    } catch (e) {
      setError(String(e));
    }
  }

  function request(command: KamalCommand) {
    if (!DESTRUCTIVE.has(command.kind)) return execute(command);
    confirm({
      title: `${label(command)} ${service} on ${target}?`,
      typed: prod && ["deploy", "redeploy", "rollback"].includes(command.kind) ? (destination ?? service) : undefined,
      destructive: true,
      onConfirm: () => execute(command),
    });
  }

  const hosts: HostEntry[] = info
    ? [...new Set([...info.config.hosts, ...info.config.accessories.flatMap((a) => a.hosts)])].map((host) => ({
        target: { host, port: info.config.ssh_port, user: info.config.ssh_user },
        accessories: info.config.accessories.filter((a) => a.hosts.includes(host)).map((a) => a.name),
      }))
    : [];

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-3 p-4">
      <header className="flex flex-wrap items-center gap-2">
        <Text variant="heading2" as="h1">
          {project.name}
        </Text>
        {info && info.destinations.length > 0 && (
          <Select
            aria-label="Destination"
            size="sm"
            className="w-44"
            value={destination ?? BASE}
            onValueChange={(v) => {
              const dest = !v || v === BASE ? null : String(v);
              setDestination(dest);
              load(dest);
            }}
            items={{
              ...(info.has_base_config && { [BASE]: "(base config)" }),
              ...Object.fromEntries(info.destinations.map((d) => [d, d])),
            }}
          />
        )}
        {prod && <Badge variant="destructive">production</Badge>}
        <div className="text-sm text-kumo-subtle min-w-0 flex-1 truncate">
          {project.path}
        </div>
        <Button size="sm" variant="ghost" icon={<KeyIcon />} onClick={() => setSecretsOpen(true)}>
          Secrets
        </Button>
        <Button size="sm" variant="secondary" icon={<ArrowClockwiseIcon />} loading={loading} onClick={() => load(destination)}>
          Reload
        </Button>
        <Button
          size="sm"
          variant="ghost"
          shape="square"
          aria-label="Remove project"
          icon={<TrashIcon />}
          onClick={() =>
            confirm({
              title: `Remove ${project.name}?`,
              description: "Its run history is deleted from Kamal Desktop Manager. Files on disk are untouched.",
              destructive: true,
              onConfirm: onRemove,
            })
          }
        />
      </header>

      {error && (
        <ErrorNotice
          error={error}
          onRetry={info ? undefined : () => load(destination)}
          action={
            /Secret '[^']+' not found/.test(error) && (
              <Button size="xs" variant="primary" icon={<KeyIcon />} onClick={() => setSecretsOpen(true)}>
                Set up secrets
              </Button>
            )
          }
        />
      )}
      {secretsOpen && (
        <SecretsDialog projectId={project.id} destination={destination} onCheck={() => load(destination)} onClose={() => setSecretsOpen(false)} />
      )}

      {info && (
        <>
          <div className="flex flex-col gap-0.5">
            <Text size="sm">
              {info.config.repository} · local HEAD <span className="font-mono">{info.config.version.slice(0, 12)}</span>
            </Text>
            <Text variant="secondary" size="sm">
              roles: {info.config.roles.join(", ")} · ssh {info.config.ssh_user}:{info.config.ssh_port}
              {info.config.accessories.length > 0 && <> · accessories: {info.config.accessories.map((a) => a.name).join(", ")}</>}
            </Text>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="primary" icon={<RocketLaunchIcon />} disabled={running} onClick={() => request({ kind: "deploy" })}>
              Deploy
            </Button>
            <Button size="sm" variant="secondary" disabled={running} onClick={() => request({ kind: "redeploy" })}>
              Redeploy
            </Button>
            <div className="mx-1 h-5 w-px bg-kumo-line" />
            <Button
              size="sm"
              variant="secondary"
              icon={<LockIcon />}
              disabled={running}
              onClick={() =>
                confirm({
                  title: `Lock deploys to ${target}`,
                  input: "Reason, e.g. running migration",
                  onConfirm: (message) => execute({ kind: "lock_acquire", message }),
                })
              }
            >
              Lock
            </Button>
            <Button size="sm" variant="secondary" icon={<LockOpenIcon />} disabled={running} onClick={() => request({ kind: "lock_release" })}>
              Unlock
            </Button>
            <Button size="sm" variant="ghost" disabled={running} onClick={() => request({ kind: "lock_status" })}>
              Lock status
            </Button>
            <div className="mx-1 h-5 w-px bg-kumo-line" />
            <Button size="sm" variant="secondary" icon={<PlayIcon />} disabled={running} onClick={() => request({ kind: "app_boot" })}>
              App boot
            </Button>
            <Button size="sm" variant="secondary-destructive" icon={<StopIcon />} disabled={running} onClick={() => request({ kind: "app_stop" })}>
              App stop
            </Button>
            {activeRun && (
              <>
                <div className="mx-1 h-5 w-px bg-kumo-line" />
                <Button size="sm" variant="secondary" icon={<XCircleIcon />} onClick={() => api.runCancel(activeRun.id, false).catch((e) => setError(String(e)))}>
                  Cancel
                </Button>
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={() =>
                    confirm({
                      title: `Kill ${activeRun.command} on ${activeRun.destination ?? "the base config (production)"}?`,
                      description:
                        "Force-stops kamal immediately. A half-finished deploy can leave the deploy lock held or old and new containers both running; check the Containers tab afterwards and Unlock if needed.",
                      destructive: true,
                      onConfirm: () => api.runCancel(activeRun.id, true).catch((e) => setError(String(e))),
                    })
                  }
                >
                  Kill
                </Button>
              </>
            )}
          </div>

          <div className="flex max-h-[40vh] flex-col gap-4 overflow-auto">
            {hosts.map(({ target }) => (
              <HostCard key={target.host} target={target} />
            ))}
          </div>
        </>
      )}

      <Tabs
        variant="underline"
        size="sm"
        value={tab}
        onValueChange={(v) => setTab(v as Tab)}
        tabs={[
          { value: "containers", label: "Containers" },
          { value: "runs", label: "Runs" },
          { value: "logs", label: logSource ? `Logs · ${logSource.label}` : "Logs" },
          { value: "proxy", label: "Proxy" },
          { value: "console", label: "Console" },
        ]}
      />
      {info && (
        <div className={cn("flex min-h-0 flex-1", tab !== "containers" && "hidden")}>
          <ContainersPanel
            key={`${info.config.service}/${kamalDestination ?? ""}`}
            hosts={hosts}
            service={info.config.service}
            destination={kamalDestination}
            busy={running}
            onRollback={(version) => request({ kind: "rollback", version })}
            onLogs={(host, c) => {
              const label = c.role ?? c.name.replace(`${info.config.service}-`, "");
              setLogSource({ target: { host, port: info.config.ssh_port, user: info.config.ssh_user }, container: c.name, label: `${label} @ ${host}` });
              setTab("logs");
            }}
          />
        </div>
      )}
      {tab === "proxy" && info && (
        <ProxyPanel
          targets={(info.config.primary_host ? [info.config.primary_host, ...info.config.hosts] : info.config.hosts)
            .filter((h, i, all) => all.indexOf(h) === i)
            .map((host) => ({ host, port: info.config.ssh_port, user: info.config.ssh_user }))}
          services={info.config.roles.map((r) => [info.config.service, r, kamalDestination].filter(Boolean).join("-"))}
        />
      )}
      {info && (
        <div className={cn("flex min-h-0 flex-1", tab !== "console" && "hidden")}>
          <ConsolePanel
            key={destination ?? ""}
            projectId={project.id}
            destination={destination}
            aliases={info.aliases}
            rails={info.rails}
            target={info.config.primary_host ?? info.config.hosts[0]}
          />
        </div>
      )}
      {tab === "logs" &&
        (logSource ? (
          <LogsPanel key={`${logSource.target.host}/${logSource.container}`} source={logSource} />
        ) : (
          <div className="text-sm text-kumo-subtle">Pick a container's Logs button above.</div>
        ))}
      <div className={cn("flex min-h-52 flex-1 gap-3", tab !== "runs" && "hidden")}>
        <RunTerminal runId={selectedRun} />
        <aside className="flex w-64 shrink-0 flex-col gap-1 overflow-auto">
          <div className="text-xs text-kumo-subtle px-1 uppercase tracking-wide">
            History
          </div>
          {runs.length === 0 && (
            <div className="text-sm text-kumo-subtle px-1">
              No runs yet
            </div>
          )}
          {runs.map((run) => {
            const status = runStatus(run);
            return (
              <button
                key={run.id}
                onClick={() => setSelectedRun(run.id)}
                className={cn(
                  "flex flex-col items-start gap-1 rounded-md border border-kumo-hairline px-2 py-1.5 text-left text-sm hover:bg-kumo-tint",
                  run.id === selectedRun && "border-kumo-brand bg-kumo-tint",
                )}
              >
                <span className="flex w-full items-center justify-between gap-2">
                  <span className="truncate">
                    {run.command}
                    {run.destination && <span className="text-kumo-subtle"> -d {run.destination}</span>}
                  </span>
                  <Badge variant={status.variant}>{status.text}</Badge>
                </span>
                <span className="text-xs text-kumo-subtle">
                  {formatTime(run.started_at)}
                  {run.finished_at && ` · ${Math.round((run.finished_at - run.started_at) / 1000)}s`}
                  {run.git_sha && ` · ${run.git_sha.slice(0, 7)}`}
                </span>
              </button>
            );
          })}
        </aside>
      </div>
    </div>
  );
}
