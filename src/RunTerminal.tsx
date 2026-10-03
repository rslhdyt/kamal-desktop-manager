import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { api } from "./api";
import { followRun } from "./liveRuns";

/** Shows one run: live output if it's running in this session, else its saved log. */
export function RunTerminal({ runId }: { runId: number | null }) {
  const el = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);

  useEffect(() => {
    const t = new Terminal({ convertEol: true, fontSize: 12, scrollback: 10_000 });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(el.current!);
    fit.fit();
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(el.current!);
    term.current = t;
    return () => {
      observer.disconnect();
      t.dispose();
    };
  }, []);

  useEffect(() => {
    const t = term.current!;
    t.reset();
    if (runId === null) return;
    const unfollow = followRun(runId, (e) => {
      if (e.kind === "line") t.writeln(e.text);
      else t.writeln(`\x1b[2m— exited ${e.code ?? "by signal"}\x1b[0m`);
    });
    if (unfollow) return unfollow;

    let cancelled = false;
    api.runLog(runId).then(
      (log) => !cancelled && t.write(log),
      (e) => !cancelled && t.writeln(`\x1b[31m${e}\x1b[0m`),
    );
    return () => {
      cancelled = true;
    };
  }, [runId]);

  return <div className="term min-w-0 flex-1 overflow-hidden rounded-lg bg-black" ref={el} />;
}
