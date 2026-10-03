import { useEffect, useState } from "react";
import { Badge, Button, cn, Table } from "@cloudflare/kumo";
import { ArrowCounterClockwiseIcon, CaretRightIcon, ScrollIcon } from "@phosphor-icons/react";
import { Container, HostSnapshot, SshTarget } from "./api";
import { useHost } from "./useHost";
import { formatBytes } from "./HostCard";

export const versionOf = (image: string) => image.split(":").pop() ?? "";

export type HostEntry = {
  target: SshTarget;
  /** Accessory names placed on this host; containers are `<service>-<name>`. */
  accessories: string[];
};

type Props = {
  hosts: HostEntry[];
  service: string;
  destination: string | null;
  /** A run is in progress; rollback is unavailable. */
  busy: boolean;
  onLogs: (host: string, container: Container) => void;
  onRollback: (version: string) => void;
};

type Row = { host: string; container: Container };

export function ContainersPanel({ hosts, service, destination, busy, onLogs, onRollback }: Props) {
  const [snapshots, setSnapshots] = useState<Record<string, HostSnapshot>>({});
  // Versions the user toggled by hand; everything else follows active = open, old = closed.
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  const showHost = hosts.length > 1;

  const app: Row[] = hosts.flatMap(({ target }) =>
    (snapshots[target.host]?.containers ?? [])
      .filter((c) => c.service === service && c.destination === destination)
      .map((container) => ({ host: target.host, container })),
  );
  const byVersion = new Map<string, Row[]>();
  for (const r of app) {
    const v = versionOf(r.container.image);
    byVersion.set(v, [...(byVersion.get(v) ?? []), r]);
  }
  // CreatedAt sorts chronologically as text, so newest version first.
  const versions = [...byVersion]
    .map(([version, rows]) => ({
      version,
      rows: rows.sort((a, b) => (a.container.role ?? "").localeCompare(b.container.role ?? "") || a.host.localeCompare(b.host)),
      running: rows.filter((r) => r.container.state === "running").length,
      created: rows.reduce((max, r) => (r.container.created_at > max ? r.container.created_at : max), ""),
    }))
    .sort((a, b) => b.created.localeCompare(a.created));

  const accessories = hosts.flatMap(({ target, accessories }) =>
    accessories.map((name) => ({
      name,
      host: target.host,
      loaded: target.host in snapshots,
      container: snapshots[target.host]?.containers.find((c) => c.name === `${service}-${name}`),
    })),
  );

  const loaded = Object.keys(snapshots).length > 0;
  const logs = (host: string, c: Container) => (
    <Button size="xs" variant="ghost" icon={<ScrollIcon />} onClick={() => onLogs(host, c)}>
      Logs
    </Button>
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto">
      {hosts.map(({ target }) => (
        <HostFeed key={target.host} target={target} onSnapshot={(s) => setSnapshots((all) => ({ ...all, [target.host]: s }))} />
      ))}

      {!loaded && <div className="text-sm text-kumo-subtle">Waiting for hosts…</div>}
      {loaded && versions.length === 0 && accessories.length === 0 && <div className="text-sm text-kumo-subtle">No containers</div>}

      {versions.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-kumo-hairline bg-kumo-base">
          <div className="px-3 py-2 text-sm">App</div>
          <Table>
            <Table.Body>
              {versions.map(({ version, rows, running }) => {
                const active = running > 0;
                const open = toggled[version] ?? active;
                return [
                  <Table.Row key={version} className="bg-kumo-tint/50">
                    <Table.Cell colSpan={6}>
                      <div className="flex items-center gap-2">
                        <button
                          className="flex items-center gap-2 font-mono text-xs"
                          aria-expanded={open}
                          onClick={() => setToggled((t) => ({ ...t, [version]: !open }))}
                        >
                          <CaretRightIcon className={cn("transition-transform", open && "rotate-90")} />
                          {version.slice(0, 12)}
                        </button>
                        <Badge variant={active ? "success" : "secondary"}>{active ? "active" : "old"}</Badge>
                        <span className="text-xs text-kumo-subtle tabular-nums">
                          {running}/{rows.length} running
                        </span>
                        {!active && (
                          <Button
                            className="ml-auto"
                            size="xs"
                            variant="secondary-destructive"
                            icon={<ArrowCounterClockwiseIcon />}
                            disabled={busy}
                            onClick={() => onRollback(version)}
                          >
                            Rollback
                          </Button>
                        )}
                      </div>
                    </Table.Cell>
                  </Table.Row>,
                  ...(open
                    ? rows.map(({ host, container: c }) => (
                        <Table.Row key={c.id}>
                          <Table.Cell className="pl-9 font-mono text-xs">
                            {c.role}
                            {showHost && <span className="text-kumo-subtle"> @ {host}</span>}
                          </Table.Cell>
                          <Table.Cell>
                            <span className="flex gap-1">
                              <Badge variant={c.state === "running" ? "success" : "secondary"}>{c.state}</Badge>
                              {c.stale && <Badge variant="warning">stale</Badge>}
                            </span>
                          </Table.Cell>
                          <Table.Cell className="text-kumo-subtle">{c.status}</Table.Cell>
                          <Usage cpu={c.cpu_pct} mem={c.mem_bytes} />
                          <Table.Cell className="w-0">{logs(host, c)}</Table.Cell>
                        </Table.Row>
                      ))
                    : []),
                ];
              })}
            </Table.Body>
          </Table>
        </div>
      )}

      {accessories.some((a) => a.loaded) && (
        <div className="overflow-hidden rounded-lg border border-kumo-hairline bg-kumo-base">
          <div className="px-3 py-2 text-sm">Accessories</div>
          <Table>
            <Table.Body>
              {accessories
                .filter((a) => a.loaded)
                .map(({ name, host, container: c }) => (
                  <Table.Row key={`${host}/${name}`}>
                    <Table.Cell className="font-mono text-xs">
                      {name}
                      {showHost && <span className="text-kumo-subtle"> @ {host}</span>}
                    </Table.Cell>
                    <Table.Cell>
                      <Badge variant={c?.state === "running" ? "success" : c ? "error" : "secondary"}>{c?.state ?? "missing"}</Badge>
                    </Table.Cell>
                    <Table.Cell className="text-kumo-subtle">{c?.status}</Table.Cell>
                    <Usage cpu={c?.cpu_pct ?? null} mem={c?.mem_bytes ?? null} />
                    <Table.Cell className="w-0">{c && logs(host, c)}</Table.Cell>
                  </Table.Row>
                ))}
            </Table.Body>
          </Table>
        </div>
      )}
    </div>
  );
}

/** Subscribes to one host and reports each snapshot up (hooks can't run in a loop). */
function HostFeed({ target, onSnapshot }: { target: SshTarget; onSnapshot: (s: HostSnapshot) => void }) {
  const snapshot = useHost(target);
  useEffect(() => {
    if (snapshot) onSnapshot(snapshot);
  }, [snapshot?.ts]); // eslint-disable-line react-hooks/exhaustive-deps
  return null;
}

function Usage({ cpu, mem }: { cpu: number | null; mem: number | null }) {
  return (
    <>
      <Table.Cell className="text-right font-mono text-xs tabular-nums">{cpu !== null && `${cpu.toFixed(1)}%`}</Table.Cell>
      <Table.Cell className="text-right font-mono text-xs tabular-nums text-kumo-subtle">{mem !== null && formatBytes(mem)}</Table.Cell>
    </>
  );
}
