import { useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Badge, Button, cn, Select, Table } from "@cloudflare/kumo";
import { ArrowClockwiseIcon, LockSimpleIcon } from "@phosphor-icons/react";
import { api, ProxyRequest, ProxyRoute, SshTarget } from "./api";
import { useHost } from "./useHost";
import { ErrorNotice } from "./errors";

const KEEP = 1000;
const MINUTES = 30;
const CLASSES = [
  { key: 2, label: "2xx", color: "bg-kumo-success" },
  { key: 3, label: "3xx", color: "bg-kumo-info" },
  { key: 4, label: "4xx", color: "bg-kumo-warning" },
  { key: 5, label: "5xx", color: "bg-kumo-danger" },
];

type Props = { targets: SshTarget[]; services: string[] };

export function ProxyPanel({ targets, services }: Props) {
  const [host, setHost] = useState(targets[0].host);
  const target = targets.find((t) => t.host === host) ?? targets[0];

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto">
      {targets.length > 1 && (
        <Select
          aria-label="Proxy host"
          size="sm"
          className="w-56"
          value={host}
          onValueChange={(v) => v && setHost(String(v))}
          items={Object.fromEntries(targets.map((t) => [t.host, t.host]))}
        />
      )}
      <Routes key={`routes-${host}`} target={target} services={services} />
      <Requests key={`requests-${host}`} target={target} services={services} />
    </div>
  );
}

function Routes({ target, services }: { target: SshTarget; services: string[] }) {
  const [routes, setRoutes] = useState<ProxyRoute[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);
  const snapshot = useHost(target);

  const refresh = () =>
    api.proxyRoutes(target).then(
      (r) => {
        setRoutes(r);
        setError(null);
      },
      (e) => setError(String(e)),
    );

  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, 30_000);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target.host]);

  // Targets are `<short container id>:<port>`; show the container name instead.
  const containerName = (t: string) => {
    const id = t.split(":")[0];
    return snapshot?.containers.find((c) => c.id.startsWith(id))?.name;
  };
  const mine = (routes ?? []).filter((r) => services.includes(r.service));
  const shown = showAll ? (routes ?? []) : mine;

  return (
    <section className="rounded-lg border border-kumo-hairline bg-kumo-base">
      <div className="flex items-center gap-2 px-3 py-2 text-sm">
        <b>Routes</b>
        <span className="text-kumo-subtle">kamal-proxy on {target.host}</span>
        <span className="flex-1" />
        {routes && routes.length > mine.length && (
          <Button size="xs" variant="ghost" onClick={() => setShowAll(!showAll)}>
            {showAll ? "Only this app" : `All ${routes.length} services`}
          </Button>
        )}
        <Button size="xs" variant="ghost" shape="square" aria-label="Refresh routes" icon={<ArrowClockwiseIcon />} onClick={refresh} />
      </div>
      {error && (
        <div className="px-3 pb-2">
          <ErrorNotice error={error} onRetry={refresh} />
        </div>
      )}
      {routes && shown.length === 0 && <div className="px-3 pb-2 text-sm text-kumo-subtle">No routes for {services.join(", ")}</div>}
      {shown.length > 0 && (
        <Table>
          <Table.Header>
            <Table.Row>
              <Table.Head>Service</Table.Head>
              <Table.Head>Host</Table.Head>
              <Table.Head>Path</Table.Head>
              <Table.Head>Target</Table.Head>
              <Table.Head>State</Table.Head>
              <Table.Head>TLS</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {shown.map((r) => (
              <Table.Row key={`${r.service}-${r.host}-${r.path}`}>
                <Table.Cell className={cn(services.includes(r.service) && "font-medium")}>{r.service}</Table.Cell>
                <Table.Cell>{r.host || <span className="text-kumo-subtle">any</span>}</Table.Cell>
                <Table.Cell className="font-mono text-xs">{r.path}</Table.Cell>
                <Table.Cell className="font-mono text-xs">
                  {r.target}
                  {containerName(r.target) && <div className="text-kumo-subtle">{containerName(r.target)}</div>}
                </Table.Cell>
                <Table.Cell>
                  <Badge variant={r.state === "running" ? "success" : "warning"}>{r.state}</Badge>
                </Table.Cell>
                <Table.Cell>{r.tls ? <LockSimpleIcon aria-label="TLS on" /> : <span className="text-kumo-subtle">off</span>}</Table.Cell>
              </Table.Row>
            ))}
          </Table.Body>
        </Table>
      )}
    </section>
  );
}

function statusVariant(status: number) {
  if (status >= 500) return "error" as const;
  if (status >= 400) return "warning" as const;
  if (status >= 300) return "info" as const;
  return "success" as const;
}

function Requests({ target, services }: { target: SshTarget; services: string[] }) {
  const buffer = useRef<ProxyRequest[]>([]);
  const [requests, setRequests] = useState<ProxyRequest[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    buffer.current = [];
    let subId: number | null = null;
    let cancelled = false;
    const channel = new Channel<ProxyRequest>();
    channel.onmessage = (r) => {
      buffer.current.push(r);
      if (buffer.current.length > KEEP) buffer.current.splice(0, buffer.current.length - KEEP);
    };
    api.proxyRequestsSubscribe(target, services, `${MINUTES}m`, channel).then(
      (id) => {
        if (cancelled) api.logsUnsubscribe(id);
        else subId = id;
      },
      (e) => setError(String(e)),
    );
    // Re-render at most once a second, however busy the proxy is.
    const timer = setInterval(() => setRequests([...buffer.current]), 1000);
    return () => {
      cancelled = true;
      clearInterval(timer);
      if (subId !== null) api.logsUnsubscribe(subId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target.host, services.join(",")]);

  // Status classes per minute over the last MINUTES minutes.
  const now = Math.floor(Date.now() / 60_000);
  const buckets = Array.from({ length: MINUTES }, () => [0, 0, 0, 0]);
  for (const r of requests) {
    const age = now - Math.floor(Date.parse(r.time) / 60_000);
    const cls = Math.floor(r.status / 100) - 2;
    if (age >= 0 && age < MINUTES && cls >= 0 && cls < 4) buckets[MINUTES - 1 - age][cls]++;
  }
  const peak = Math.max(1, ...buckets.map((b) => b.reduce((a, n) => a + n, 0)));
  const totals = CLASSES.map((_, i) => buckets.reduce((a, b) => a + b[i], 0));

  return (
    <section className="rounded-lg border border-kumo-hairline bg-kumo-base">
      <div className="flex flex-wrap items-center gap-3 px-3 py-2 text-sm">
        <b>Requests</b>
        <span className="text-kumo-subtle">last {MINUTES} min · {services.join(", ")}</span>
        <span className="flex-1" />
        {CLASSES.map((c, i) => (
          <span key={c.key} className="flex items-center gap-1 text-xs">
            <span className={cn("h-2 w-2 rounded-sm", c.color)} /> {c.label} {totals[i]}
          </span>
        ))}
      </div>
      {error && (
        <div className="px-3 pb-2">
          <ErrorNotice error={error} />
        </div>
      )}
      <div className="flex h-20 items-end gap-0.5 px-3" aria-label="Requests per minute by status class">
        {buckets.map((b, i) => (
          <div key={i} className="flex h-full flex-1 flex-col-reverse" title={`${MINUTES - 1 - i} min ago: ${b.join(" / ")}`}>
            {CLASSES.map((c, j) => (
              <div key={c.key} className={c.color} style={{ height: `${(100 * b[j]) / peak}%` }} />
            ))}
          </div>
        ))}
      </div>
      <div className="max-h-72 overflow-auto">
        <Table>
          <Table.Body>
            {requests
              .slice(-100)
              .reverse()
              .map((r) => (
                <Table.Row key={r.request_id || r.time}>
                  <Table.Cell className="whitespace-nowrap font-mono text-xs text-kumo-subtle">{new Date(r.time).toLocaleTimeString()}</Table.Cell>
                  <Table.Cell>
                    <Badge variant={statusVariant(r.status)}>{r.status}</Badge>
                  </Table.Cell>
                  <Table.Cell className="font-mono text-xs">{r.method}</Table.Cell>
                  <Table.Cell className="max-w-96 truncate font-mono text-xs" title={r.path}>
                    {r.host} {r.path}
                  </Table.Cell>
                  <Table.Cell className="text-right font-mono text-xs">{r.duration_ms.toFixed(1)} ms</Table.Cell>
                </Table.Row>
              ))}
          </Table.Body>
        </Table>
        {requests.length === 0 && !error && <div className="px-3 py-2 text-sm text-kumo-subtle">Waiting for requests…</div>}
      </div>
    </section>
  );
}
