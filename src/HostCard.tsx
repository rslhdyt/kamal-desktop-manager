import { lazy, Suspense, useEffect, useState } from "react";
import { ChartPalette, cn, Loader } from "@cloudflare/kumo";
import { WarningIcon } from "@phosphor-icons/react";
import { HostSnapshot, SshTarget } from "./api";
import { useHost } from "./useHost";
import { describeError } from "./errors";
import { useIsDarkMode } from "./useIsDarkMode";

// ECharts is large; load it only once a host card actually renders a chart.
const UsageChart = lazy(() => import("./UsageChart"));
const CHART_HEIGHT = 140;

export function formatBytes(n: number) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i < 2 ? 0 : 1)} ${units[i]}`;
}

const pct = (used: number, total: number) => (total > 0 ? (100 * used) / total : 0);

export function HostCard({ target }: { target: SshTarget }) {
  const snapshot = useHost(target);
  const m = snapshot?.metrics;
  const history = useHistory(snapshot);
  const isDarkMode = useIsDarkMode();
  const cpuColor = ChartPalette.categorical(0, isDarkMode);
  const memColor = ChartPalette.categorical(1, isDarkMode);
  const health = !snapshot ? "bg-kumo-line" : snapshot.error ? (m ? "bg-kumo-warning" : "bg-kumo-danger") : "bg-kumo-success";

  return (
    <div className="shrink-0 overflow-hidden rounded-lg border border-kumo-hairline bg-kumo-base">
      <div className="flex items-center gap-2 px-3 py-2 text-sm">
        <span className={cn("size-2 shrink-0 rounded-full", health)} />
        <span className="font-mono">{target.host}</span>
        {!snapshot && <Loader size={14} />}
        {snapshot?.error && (
          <span className="flex min-w-0 items-center gap-1 truncate text-xs text-kumo-danger" title={snapshot.error}>
            <WarningIcon /> {describeError(snapshot.error).title}
            {describeError(snapshot.error).hint && <span className="truncate text-kumo-subtle"> — {describeError(snapshot.error).hint}</span>}
            {snapshot.metrics && <span className="text-kumo-subtle"> · showing last data</span>}
          </span>
        )}
      </div>

      {m && (
        <div className="grid grid-cols-3 divide-x divide-kumo-hairline border-t border-kumo-hairline">
          <Gauge
            label="CPU"
            pct={m.cpu_pct}
            color={cpuColor}
            detail={`load ${m.load1.toFixed(2)}  ${m.load5.toFixed(2)}  ${m.load15.toFixed(2)}`}
          />
          <Gauge
            label="Memory"
            pct={pct(m.mem_used, m.mem_total)}
            color={memColor}
            detail={`${formatBytes(m.mem_used)} of ${formatBytes(m.mem_total)}`}
          />
          <Gauge
            label="Disk"
            pct={pct(m.disk_used, m.disk_total)}
            detail={`${formatBytes(m.disk_used)} of ${formatBytes(m.disk_total)} on /`}
            bar
          />
        </div>
      )}
      {m && (
        <div className="border-t border-kumo-hairline px-1 pt-1">
          <Suspense fallback={<div style={{ height: CHART_HEIGHT }} />}>
            <UsageChart
              isDarkMode={isDarkMode}
              height={CHART_HEIGHT}
              loading={history.length < 2}
              data={[
                { name: "CPU", color: cpuColor, data: history.flatMap((h) => (h.cpu === null ? [] : [[h.ts, h.cpu] as [number, number]])) },
                { name: "Memory", color: memColor, data: history.map((h) => [h.ts, h.mem] as [number, number]) },
              ]}
              ariaDescription={`CPU and memory usage on ${target.host} over the last few minutes`}
            />
          </Suspense>
        </div>
      )}
    </div>
  );
}

/** Healthy usage stays neutral; colour only appears once a resource is getting tight. */
const tone = (p: number) => (p >= 90 ? "text-kumo-danger" : p >= 75 ? "text-kumo-warning" : "text-kumo-subtle");

function Gauge({ label, pct, detail, color, bar }: { label: string; pct: number | null; detail: string; color?: string; bar?: boolean }) {
  return (
    <div className="flex min-w-0 flex-col gap-1 px-3 py-2.5">
      <div className="flex items-baseline justify-between gap-2">
        <span className="flex items-center gap-1.5 text-xs text-kumo-subtle">
          {color && <span className="size-2 rounded-full" style={{ background: color }} />}
          {label}
        </span>
        <span className={cn("text-xl font-medium tabular-nums", pct !== null && pct >= 75 ? tone(pct) : "text-kumo-default")}>
          {pct === null ? "…" : `${pct.toFixed(0)}%`}
        </span>
      </div>
      <span className="truncate text-xs whitespace-pre text-kumo-subtle tabular-nums">{detail}</span>
      {bar && pct !== null && (
        <div className="mt-auto h-1.5 w-full overflow-hidden rounded-full bg-kumo-tint">
          <div className={cn("h-full rounded-full bg-current", tone(pct))} style={{ width: `${Math.min(pct, 100)}%` }} />
        </div>
      )}
    </div>
  );
}

const HISTORY = 60; // ~5 min at the focused 5s poll interval

/** Rolling CPU/memory samples, one per successful snapshot. */
function useHistory(snapshot: HostSnapshot | null) {
  const [samples, setSamples] = useState<{ ts: number; cpu: number | null; mem: number }[]>([]);
  useEffect(() => {
    const m = snapshot?.metrics;
    if (!snapshot || snapshot.error || !m) return;
    setSamples((s) => [...s, { ts: snapshot.ts, cpu: m.cpu_pct, mem: pct(m.mem_used, m.mem_total) }].slice(-HISTORY));
  }, [snapshot?.ts]); // eslint-disable-line react-hooks/exhaustive-deps
  return samples;
}
