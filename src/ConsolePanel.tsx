import { useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { Button } from "@cloudflare/kumo";
import { TerminalWindowIcon, XIcon } from "@phosphor-icons/react";
import { api, ConsoleEvent } from "./api";

/** Rails console via `kamal app exec -i --reuse` on a local PTY. Stays alive while hidden. */
export function ConsolePanel({ projectId, destination, target }: { projectId: number; destination: string | null; target: string }) {
  const el = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const session = useRef<number | null>(null);
  const [state, setState] = useState<"idle" | "starting" | "open" | "closed">("idle");

  useEffect(() => {
    const t = new Terminal({ fontSize: 13, scrollback: 10_000, cursorBlink: true });
    const f = new FitAddon();
    t.loadAddon(f);
    t.open(el.current!);
    f.fit();
    t.onData((data) => session.current !== null && api.consoleWrite(session.current, data).catch(() => {}));
    t.onResize(({ rows, cols }) => session.current !== null && api.consoleResize(session.current, rows, cols).catch(() => {}));
    const observer = new ResizeObserver(() => el.current!.offsetWidth > 0 && f.fit());
    observer.observe(el.current!);
    term.current = t;
    fit.current = f;
    return () => {
      observer.disconnect();
      if (session.current !== null) api.consoleClose(session.current);
      t.dispose();
    };
  }, []);

  async function open() {
    const t = term.current!;
    t.reset();
    t.writeln(`\x1b[2m$ kamal app exec -i --reuse "bin/rails console"${destination ? ` -d ${destination}` : ""}\x1b[0m`);
    setState("starting");
    const channel = new Channel<ConsoleEvent>();
    channel.onmessage = (e) => {
      if (e.kind === "output") {
        t.write(e.data);
        setState("open");
      } else {
        session.current = null;
        t.writeln("\r\n\x1b[2m— console closed\x1b[0m");
        setState("closed");
      }
    };
    try {
      session.current = await api.consoleOpen(projectId, destination, t.rows, t.cols, channel);
      t.focus();
    } catch (e) {
      t.writeln(`\x1b[31m${e}\x1b[0m`);
      setState("closed");
    }
  }

  const live = state === "starting" || state === "open";

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex items-center gap-2">
        {live ? (
          <Button size="sm" variant="secondary" icon={<XIcon />} onClick={() => session.current !== null && api.consoleClose(session.current)}>
            Close console
          </Button>
        ) : (
          <Button size="sm" variant="primary" icon={<TerminalWindowIcon />} onClick={open}>
            {state === "closed" ? "Reopen Rails console" : "Open Rails console"}
          </Button>
        )}
        <span className="text-xs text-kumo-subtle">
          {state === "starting" ? "starting…" : `runs in the live app container on ${target}`}
        </span>
      </div>
      <div className="term min-h-64 min-w-0 flex-1 overflow-hidden rounded-lg bg-black" ref={el} />
    </div>
  );
}
