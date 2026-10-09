import { useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { Button, Select } from "@cloudflare/kumo";
import { TerminalWindowIcon, XIcon } from "@phosphor-icons/react";
import { Alias, api, ConsoleEvent } from "./api";

// Select values can't be null; stands in for the built-in Rails console.
const RAILS = "__rails__";

type Props = { projectId: number; destination: string | null; aliases: Alias[]; rails: boolean; target: string };

/**
 * Rails console (`kamal app exec -i --reuse`) or a deploy-config alias, on a
 * local PTY. Stays alive while hidden.
 */
export function ConsolePanel({ projectId, destination, aliases, rails, target }: Props) {
  const el = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const session = useRef<number | null>(null);
  const [state, setState] = useState<"idle" | "starting" | "open" | "closed">("idle");
  const [choice, setChoice] = useState<string | null>(null);
  const commands: Record<string, string> = {
    ...(rails && { [RAILS]: "Rails console" }),
    ...Object.fromEntries(aliases.map((a) => [a.name, a.name])),
  };
  // Keep the pick only while the config still offers it.
  const selected = choice !== null && choice in commands ? choice : (Object.keys(commands)[0] ?? null);

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
    if (selected === null) return;
    const alias = selected === RAILS ? null : selected;
    const t = term.current!;
    t.reset();
    const dest = destination ? ` -d ${destination}` : "";
    const expansion = aliases.find((a) => a.name === alias)?.command;
    t.writeln(`\x1b[2m$ kamal ${alias ?? 'app exec -i --reuse "bin/rails console"'}${dest}${expansion ? `  # ${expansion}` : ""}\x1b[0m`);
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
      session.current = await api.consoleOpen(projectId, destination, alias, t.rows, t.cols, channel);
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
        ) : selected === null ? null : (
          <>
            {Object.keys(commands).length > 1 && (
              <Select aria-label="Console command" size="sm" className="w-48" value={selected} onValueChange={(v) => v && setChoice(String(v))} items={commands} />
            )}
            <Button size="sm" variant="primary" icon={<TerminalWindowIcon />} onClick={open}>
              {state === "closed" ? "Reopen" : "Open"} {commands[selected]}
            </Button>
          </>
        )}
        <span className="text-xs text-kumo-subtle">
          {state === "starting"
            ? "starting…"
            : selected === null
              ? "No bin/rails and no aliases in config/deploy.yml — add an alias (e.g. shell: app exec -i --reuse bash) to open one here."
              : selected === RAILS
                ? `runs in the live app container on ${target}`
                : aliases.find((a) => a.name === selected)?.command}
        </span>
      </div>
      <div className="term min-h-64 min-w-0 flex-1 overflow-hidden rounded-lg bg-black" ref={el} />
    </div>
  );
}
