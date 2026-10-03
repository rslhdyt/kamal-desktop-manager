import { Channel } from "@tauri-apps/api/core";
import { api, KamalCommand, RunEvent } from "./api";
import { describeError } from "./errors";
import { toasts } from "./toasts";

type Listener = (event: RunEvent) => void;
type LiveRun = { lines: string[]; done: boolean; listeners: Set<Listener> };

// Output of runs started in this session, kept outside React so switching
// projects or views doesn't drop lines of a run still in progress.
const live = new Map<number, LiveRun>();

export async function startRun(projectId: number, destination: string | null, command: KamalCommand, onExit: () => void) {
  const run: LiveRun = { lines: [], done: false, listeners: new Set() };
  const channel = new Channel<RunEvent>();
  let registry: string | null = null;
  let staleLogin = false;
  channel.onmessage = (event) => {
    if (event.kind === "line") {
      run.lines.push(event.text);
      const text = stripAnsi(event.text);
      registry = text.match(/docker login (\S+) -u/)?.[1] ?? registry;
      staleLogin ||= /already exists in the keychain/.test(text);
    } else {
      run.done = true;
      onExit();
      if (staleLogin && registry) offerClearLogin(registry);
    }
    run.listeners.forEach((l) => l(event));
  };
  const id = await api.runStart(projectId, destination, command, channel);
  live.set(id, run);
  return id;
}

/** Replays buffered lines, then follows. Returns null for runs not started in this session. */
export function followRun(id: number, listener: Listener): (() => void) | null {
  const run = live.get(id);
  if (!run) return null;
  run.lines.forEach((text) => listener({ kind: "line", stream: "stdout", text }));
  if (run.done) return () => {};
  run.listeners.add(listener);
  return () => run.listeners.delete(listener);
}

export function isLive(id: number) {
  const run = live.get(id);
  return !!run && !run.done;
}

const stripAnsi = (text: string) => text.replace(/\x1b\[[0-9;]*m/g, "");

// Kamal's `docker login` fails when the macOS keychain already holds an entry
// for the registry that the credential helper can't update (-25299).
function offerClearLogin(server: string) {
  const id = toasts.add({
    variant: "warning",
    title: `Stale ${server} login in your keychain`,
    description: `Docker couldn't replace an old ${server} login in the macOS keychain. Remove the old entries, then run the command again. Kamal logs in fresh on every deploy.`,
    timeout: 0,
    actions: [
      {
        children: "Remove stale login",
        variant: "primary",
        onClick: () => {
          toasts.close(id);
          api.clearDockerLogin(server).then(
            (n) => toasts.add({ variant: "success", title: `Removed ${n} ${server} keychain ${n === 1 ? "entry" : "entries"}`, description: "Run the command again." }),
            (e) => toasts.add({ variant: "error", title: describeError(String(e)).title, timeout: 0 }),
          );
        },
      },
    ],
  });
}
