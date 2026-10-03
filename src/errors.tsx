import { Component, ReactNode, useState } from "react";
import { Banner, Button } from "@cloudflare/kumo";

type Described = { title: string; hint?: string };

// Known failures from the Rust side, matched on their message text.
const KNOWN: [RegExp, (m: RegExpMatchArray) => Described][] = [
  [/(\S+) is not in ~\/\.ssh\/known_hosts; connect once with `([^`]+)`/, (m) => ({
    title: `Unknown host key for ${m[1]}`,
    hint: `Kamal Desktop Manager never auto-accepts host keys. Run \`${m[2]}\` in a terminal, check the fingerprint, then retry.`,
  })],
  [/key changed/i, () => ({
    title: "Host key changed",
    hint: "The server's key no longer matches ~/.ssh/known_hosts. If the server was rebuilt, remove the old entry with `ssh-keygen -R <host>`; otherwise treat this as a possible attack.",
  })],
  [/no ssh-agent identity or unencrypted IdentityFile accepted by (\S+)/, (m) => ({
    title: `SSH login rejected for ${m[1]}`,
    hint: "Add your key to the agent with `ssh-add`. Passphrase-protected keys are only used through ssh-agent.",
  })],
  [/timed out connecting to (\S+)/, (m) => ({
    title: `Can't reach ${m[1]}`,
    hint: "Check the host is up and port 22 is reachable from this network (VPN, firewall).",
  })],
  [/connection (refused|reset)/i, () => ({
    title: "SSH connection refused",
    hint: "sshd may be rate-limiting this machine (MaxStartups / PerSourcePenalties). Kamal Desktop Manager backs off and retries.",
  })],
  [/permission denied while trying to connect to the docker daemon/i, () => ({
    title: "No access to Docker on this host",
    hint: "The SSH user needs to be in the `docker` group (the same setup `kamal setup` expects).",
  })],
  [/kamal not found on login PATH|(\S+) not found on login PATH/, () => ({
    title: "kamal not found",
    hint: "Install the kamal gem for this project's Ruby, or add a bin/kamal binstub (`bundle binstubs kamal`).",
  })],
  [/login shell took over 20s|could not read the login shell environment/, () => ({
    title: "Couldn't load your shell environment",
    hint: "Kamal Desktop Manager reads PATH from an interactive login shell to find the project's Ruby. Check that your shell rc files finish without prompting.",
  })],
  [/has no config\/deploy\.yml/, () => ({ title: "Not a Kamal project", hint: "Pick the folder that contains config/deploy.yml or config/deploy.<destination>.yml." })],
  [/is already added/, () => ({ title: "Project already added" })],
  [/kamal config( -d \S+)? failed/, () => ({
    title: "kamal config failed",
    hint: "Kamal Desktop Manager reads config through `kamal config`. Run it in a terminal in the project folder to see the full error (missing secrets, destination, git repo…).",
  })],
  [/a command is already running for (.+)/, (m) => ({ title: `A command is already running for ${m[1]}` })],
];

export function describeError(error: string): Described {
  for (const [re, describe] of KNOWN) {
    const m = error.match(re);
    if (m) return describe(m);
  }
  return { title: error.split("\n")[0].slice(0, 200) };
}

/** A friendly title and fix for known errors, with the raw message on demand. */
export function ErrorNotice({ error, onRetry }: { error: string; onRetry?: () => void }) {
  const [open, setOpen] = useState(false);
  const { title, hint } = describeError(error);
  const detail = title !== error;

  return (
    <Banner variant="error">
      <div className="flex flex-col gap-1">
        <b>{title}</b>
        {hint && <span className="text-sm">{hint}</span>}
        {detail && open && <pre className="max-h-48 overflow-auto whitespace-pre-wrap text-xs opacity-80">{error}</pre>}
        {(detail || onRetry) && (
          <span className="flex gap-2">
            {detail && (
              <Button size="xs" variant="ghost" onClick={() => setOpen(!open)}>
                {open ? "Hide details" : "Details"}
              </Button>
            )}
            {onRetry && (
              <Button size="xs" variant="secondary" onClick={onRetry}>
                Retry
              </Button>
            )}
          </span>
        )}
      </div>
    </Banner>
  );
}

/** Keeps one broken view from blanking the whole window. */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="m-auto max-w-lg p-6">
        <ErrorNotice error={`Something went wrong in this view.\n\n${this.state.error.stack ?? this.state.error.message}`} onRetry={() => this.setState({ error: null })} />
      </div>
    );
  }
}
