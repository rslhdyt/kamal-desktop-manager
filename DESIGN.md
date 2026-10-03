# Design

How Kamal Desktop Manager (kdm) is built and why. Read this before adding a feature; update it when a decision here changes.

## What it is

A desktop companion for apps deployed with [Kamal](https://kamal-deploy.org). Point it at a project folder and it shows
the hosts, containers, proxy traffic and logs, and runs the everyday kamal commands (deploy, redeploy, rollback, lock,
app boot/stop, Rails console) with a history of every run.

It is a **window onto Kamal and the servers, not a replacement for either.** Nothing lives only in kdm: config comes from
the project, state comes from the hosts, and every action is a kamal command you could have typed yourself.

## Principles

1. **Kamal is the source of truth.** Config is read through `kamal config`, never by parsing `deploy.yml` ourselves, so ERB,
   secrets and destination merging always match the CLI. Actions run the project's own kamal (configured binary →
   `bin/kamal` → `kamal` on the login PATH) inside the project's login-shell environment, so rbenv/mise/asdf pick the
   right Ruby.
2. **Allowlisted commands only.** The UI sends a `KamalCommand` enum (`runner.rs`), never raw argv. Remote shell strings are
   built on the Rust side and user input is single-quote escaped (`logs.rs`).
3. **Safe by default, loud when it matters.**
   - Destructive commands (`deploy`, `redeploy`, `rollback`, `app stop/boot`, `lock release`) always confirm, and the
     confirmation names where the action lands.
   - Production (the base config, or any destination matching `/prod/i`) wears a `production` badge. Deploy, redeploy and
     rollback there require typing the destination name.
   - One run at a time per project + destination.
   - Host keys are never auto-accepted; unknown or changed keys are errors with a fix.
4. **Read-only by default.** Monitoring (metrics, containers, proxy, logs) only reads. The only ways to change a server
   are the confirmed kamal commands and the console.
5. **Show stale data rather than nothing.** When a host poll fails, the last good snapshot stays on screen next to the
   error ("showing last data").
6. **Errors explain the fix.** Known failures are mapped to a title and a hint (`errors.tsx`); the raw message is one click
   away under "Details". New failure modes get a new entry there.
7. **Cheap on the servers.** One SSH connection per `user@host:port`, shared by every poller, log stream and proxy
   follower. One poll per host, however many views watch it. Polls are batched into a single round trip.
8. **Fast to open.** The target is a cold start under 1.5 s (logged as `kdm: ui ready after N ms`). Heavy libraries load
   lazily.

## Architecture

```
┌──────────────────────── React (src/) ────────────────────────┐
│ App ─ sidebar of projects                                     │
│  └ ProjectView ─ header, toolbar, host cards, tabs            │
│      ├ HostCard ×N        (useHost → host:update events)      │
│      ├ ContainersPanel    (useHost ×N, version tree)          │
│      ├ RunTerminal + history   (liveRuns, Channel<RunEvent>)  │
│      ├ LogsPanel          (Channel<LogEvent>)                 │
│      ├ ProxyPanel         (routes + request stream)           │
│      └ ConsolePanel       (PTY, Channel<ConsoleEvent>)        │
└──────────────┬──────────────────────────────▲────────────────┘
      invoke() │ api.ts (typed wrappers)      │ events / channels
┌──────────────▼──────────────────────────────┴──── Rust (src-tauri/src/) ┐
│ lib.rs      commands + AppState                                         │
│ runner.rs   local kamal processes, run registry, run logs              │
│ console.rs  interactive kamal on a local PTY                            │
│ collector.rs  one poller per watched host (5 s focused / 30 s bg)       │
│ pool.rs     one russh connection per host, evicted on failure           │
│ ssh.rs      russh transport: agent → IdentityFile, ssh -G, known_hosts  │
│ metrics.rs / docker.rs   parse /proc + docker ps/stats in one batch     │
│ logs.rs     long-lived remote followers (docker logs -f, proxy log)     │
│ proxy.rs    kamal-proxy list + JSON request log                         │
│ project.rs  parse `kamal config` output, list destinations              │
│ db.rs       SQLite (projects, runs) via sqlx + migrations/              │
└─────────────────────────────────────────────────────────────────────────┘
```

### Two ways kamal is reached

| Path | Used for | Where |
| --- | --- | --- |
| **Local kamal process** | config, deploy/redeploy/rollback, locks, app boot/stop, console | `runner.rs`, `console.rs` |
| **Direct SSH (russh)** | host metrics, containers, logs, proxy routes and requests | `pool.rs`, `collector.rs`, `logs.rs`, `proxy.rs` |

Anything that changes a deployment goes through kamal locally. Monitoring goes straight over SSH because spawning kamal
every 5 seconds would be slow and noisy.

### Frontend ↔ backend

- **Commands** (`invoke`) for request/response: project CRUD, config, run start/cancel, history. All wrapped and typed in
  `api.ts`, the only file that talks to Tauri directly (plus the event listener in `useHost`).
- **Global events** for shared state: `host:update` carries a `HostSnapshot`. `useHost(target)` subscribes, calls
  `host_watch`/`host_unwatch`, and the collector ref-counts watchers so a host is polled once.
- **Channels** for per-subscription streams: run output, log lines, proxy requests, console bytes. Unsubscribing aborts
  the forwarder, which closes the SSH exec channel and ends the remote `-f` command.

### Runs

- `runner::start` records a row in `runs`, streams merged stdout/stderr to the UI and to `<data dir>/runs/<id>.log`, and
  closes the row with the exit code however the process ends.
- Cancel sends SIGINT to the run's process group; Kill sends SIGKILL.
- `liveRuns.ts` buffers output of runs started this session outside React, so switching projects or tabs never drops lines.
  Older runs replay from the log file.
- Output of `kamal config` is never logged: it contains resolved secrets.

### Persistence

SQLite in the app data dir, schema in `src-tauri/migrations/`. It stores only what kdm itself owns: the project list
(path, kamal binary, last destination) and run history. Everything else is fetched live.

### Destinations

A project has either a base `config/deploy.yml` (destinations layer on top, `-d <dest>`) or only standalone
`config/deploy.<dest>.yml` files (selected with `-c`, and their containers carry no destination label). `kamalDestination`
in `ProjectView` is the destination as it appears on container labels; use it when matching containers.

## UI

### Layout

- **Sidebar:** project list, "Add project", version and update notice at the bottom.
- **Project header:** name, destination select, `production` badge, path, Reload, Remove.
- **Summary line:** repository, local HEAD, roles, SSH user and port, accessories.
- **Toolbar:** Deploy, Redeploy, lock and app commands, and Cancel/Kill while a run is active. Destructive buttons use the
  destructive variants.
- **Host cards:** one per host. Health dot, CPU/memory/disk gauges, and a live CPU + memory chart (about 5 minutes).
- **Tabs:** Containers (default) · Runs · Logs · Proxy · Console. Starting a run switches to Runs; a container's Logs
  button switches to Logs.
  - **Containers:** the App section is a tree grouped by image version, newest first. A version with anything running is
    `active` and starts expanded; the rest are `old`, start collapsed and carry a Rollback button. Accessories are a flat
    list. Rows show `role @ host` when there is more than one host.
  - **Console** stays mounted while hidden so the session survives tab switches. Containers does the same so expand
    state survives.

### Visual style

- **Components:** [Kumo](https://github.com/cloudflare/kumo) (`@cloudflare/kumo`) for every control, table, badge,
  dialog, banner, toast and chart. Don't hand-roll something Kumo has.
- **Styling:** Tailwind v4 utility classes, using Kumo's semantic tokens only (`bg-kumo-base`, `bg-kumo-canvas`,
  `bg-kumo-tint`, `border-kumo-hairline`, `text-kumo-subtle`, `text-kumo-danger`, …). No raw hex colours in components.
  `App.css` holds only globals.
- **Icons:** Phosphor (`@phosphor-icons/react`), `*Icon` names.
- **Theme:** follows the OS appearance through Kumo's `data-mode` (`main.tsx`); `useIsDarkMode` feeds charts.
- **Surfaces:** content sits in `rounded-lg border border-kumo-hairline bg-kumo-base` cards on the canvas background. Section
  titles are `text-sm`; small uppercase `text-xs text-kumo-subtle tracking-wide` labels head side lists.
- **Density:** a tool for developers, so it's compact. Buttons use `size="sm"` in toolbars and `size="xs"` in rows; tables
  use `text-xs` for data.
- **Monospace** for anything machine-shaped: hosts, container roles, SHAs, CPU and memory figures. Numbers use
  `tabular-nums` and are right-aligned.
- **Colour means something.** Healthy usage stays neutral; warning appears at 75% and danger at 90%. Badges carry state:
  `success` running/ok, `secondary` stopped/old, `warning` stale/interrupted/cancelled, `error` failed/missing,
  `destructive` production. Don't add colour for decoration.
- **Terminals:** xterm.js for run output, logs and the console. It renders its own dark background in both themes.
- **Copy:** short and literal. Confirmations are questions that name the action and target ("Rollback app on production?").
  Hints tell the user what to run.

## Code conventions

- **Frontend:** function components and hooks, plain `useState`/`useEffect`. No state library, no router. `ProjectView`
  owns project-level state and passes callbacks down. Hooks can't run in loops, so per-host subscriptions use a small
  child component (`HostFeed` in `ContainersPanel`).
- **One file per panel or concern**, named after what it renders (`LogsPanel.tsx`) or does (`liveRuns.ts`). Shared helpers
  are exported from where they're first needed (`formatBytes` in `HostCard`).
- **Rust:** one module per concern, `anyhow::Result` inside, `String` errors at the command boundary (`format!("{e:#}")`
  keeps the context chain so `errors.tsx` can match it). The `SshTransport` trait leaves room for a system-`ssh` fallback
  (ProxyJump, 2FA).
- **Comments explain why**, not what: an external quirk, a constraint, a PRD target. Keep them one or two lines.
- **Parse tool output defensively.** Docker and kamal-proxy output is human-oriented; parsers fail loudly on unknown
  layouts instead of guessing.
- **Keep the main bundle small.** ECharts is lazy-loaded (`UsageChart`) and vendors are split into chunks (`vite.config.ts`).

## Testing and release

- `cargo test` in `src-tauri/` covers parsers, the runner and the DB. `live_spike` (`--ignored`) runs the whole read-only
  path against a real project: `KDM_PROJECT=/path/to/app cargo test live_spike -- --ignored --nocapture`.
- `pnpm build` runs `tsc` and vite; it must pass before a change is done.
- Releases are tagged `v*`, built by `.github/workflows/release.yml` for macOS and Linux, signed for the Tauri updater,
  and published to `kdm-releases`. See README.
