import { useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import "@xterm/xterm/css/xterm.css";
import { Button, Input, Select } from "@cloudflare/kumo";
import { CopyIcon, MagnifyingGlassIcon, PauseIcon, PlayIcon } from "@phosphor-icons/react";
import { api, LogEvent, LogOptions, SshTarget } from "./api";
import { describeError } from "./errors";

const RING = 10_000;

export type LogSource = { target: SshTarget; container: string; label: string };

/** Follows `docker logs -f` for one container. Lines beyond 10k scroll off. */
export function LogsPanel({ source }: { source: LogSource }) {
  const el = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const search = useRef<SearchAddon | null>(null);
  const paused = useRef<string[] | null>(null);
  const [isPaused, setPaused] = useState(false);
  const [status, setStatus] = useState("");
  const [draft, setDraft] = useState<LogOptions>({ lines: 500, since: "", grep: "" });
  const [opts, setOpts] = useState<LogOptions>(draft);
  const [find, setFind] = useState("");

  useEffect(() => {
    const t = new Terminal({ convertEol: true, fontSize: 12, scrollback: RING });
    const fit = new FitAddon();
    const s = new SearchAddon();
    t.loadAddon(fit);
    t.loadAddon(s);
    t.open(el.current!);
    fit.fit();
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(el.current!);
    term.current = t;
    search.current = s;
    return () => {
      observer.disconnect();
      t.dispose();
    };
  }, []);

  useEffect(() => {
    const t = term.current!;
    t.reset();
    paused.current = isPaused ? [] : null;
    setStatus("connecting…");
    let subId: number | null = null;
    let cancelled = false;

    const channel = new Channel<LogEvent>();
    channel.onmessage = (e) => {
      if (e.kind === "end") return setStatus(`stream ended${e.code !== null ? ` (exit ${e.code})` : ""}`);
      if (paused.current) {
        paused.current.push(e.text);
        if (paused.current.length > RING) paused.current.shift();
      } else t.writeln(e.text);
    };
    api.logsSubscribe(source.target, source.container, opts, channel).then(
      (id) => {
        if (cancelled) return api.logsUnsubscribe(id);
        subId = id;
        setStatus("following");
      },
      (e) => setStatus(describeError(String(e)).title),
    );
    return () => {
      cancelled = true;
      if (subId !== null) api.logsUnsubscribe(subId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source.target.host, source.container, opts]);

  function togglePause() {
    if (paused.current) {
      paused.current.forEach((line) => term.current!.writeln(line));
      paused.current = null;
      setPaused(false);
    } else {
      paused.current = [];
      setPaused(true);
    }
  }

  async function copyAll() {
    const buf = term.current!.buffer.active;
    const lines: string[] = [];
    for (let i = 0; i < buf.length; i++) lines.push(buf.getLine(i)?.translateToString(true) ?? "");
    await navigator.clipboard.writeText(lines.join("\n").trimEnd());
    setStatus("copied");
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-2">
      <form
        className="flex flex-wrap items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          setOpts({ ...draft });
        }}
      >
        <span className="font-mono text-sm">{source.label}</span>
        <Input size="sm" className="w-44" placeholder="grep" aria-label="grep" value={draft.grep} onChange={(e) => setDraft({ ...draft, grep: e.target.value })} />
        <Input size="sm" className="w-24" placeholder="since 10m" aria-label="since" value={draft.since} onChange={(e) => setDraft({ ...draft, since: e.target.value })} />
        <Select
          aria-label="Lines"
          size="sm"
          className="w-28"
          value={String(draft.lines)}
          onValueChange={(v) => setDraft({ ...draft, lines: Number(v) })}
          items={{ "100": "100 lines", "500": "500 lines", "2000": "2000 lines", "10000": "10k lines" }}
        />
        <Button size="sm" type="submit" variant="secondary">
          Apply
        </Button>
        <Button size="sm" variant="ghost" icon={isPaused ? <PlayIcon /> : <PauseIcon />} onClick={togglePause}>
          {isPaused ? "Resume" : "Pause"}
        </Button>
        <Button size="sm" variant="ghost" icon={<CopyIcon />} onClick={copyAll}>
          Copy
        </Button>
        <Input
          size="sm"
          className="w-40"
          placeholder="find"
          aria-label="find"
          value={find}
          onChange={(e) => setFind(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              if (e.shiftKey) search.current?.findPrevious(find);
              else search.current?.findNext(find);
            }
          }}
        />
        <MagnifyingGlassIcon className="text-kumo-subtle" />
        <span className="text-xs text-kumo-subtle">{isPaused ? "paused" : status}</span>
      </form>
      <div className="term min-h-0 min-w-0 flex-1 overflow-hidden rounded-lg bg-black" ref={el} />
    </div>
  );
}
