import { useEffect, useState } from "react";
import { Badge, Button, Dialog, Input, Select, Switch, Text } from "@cloudflare/kumo";
import { CopyIcon, PlusIcon } from "@phosphor-icons/react";
import { api, SecretSource, SecretsScan } from "./api";
import { ErrorNotice } from "./errors";

type Adapter = { label: string; account?: string; accountRequired?: boolean; from?: string; hint: string };

// Kamal's `kamal secrets fetch --adapter` names. Requirements mirror kamal 2's adapters.
const ADAPTERS: Record<string, Adapter> = {
  "1password": {
    label: "1Password",
    account: "Account (e.g. my.1password.com)",
    accountRequired: true,
    from: "Vault/Item",
    hint: "Needs the `op` CLI. Turn on Settings → Developer → Integrate with 1Password CLI in the desktop app so kamal can unlock with Touch ID; there's no terminal for a password prompt.",
  },
  bitwarden: {
    label: "Bitwarden",
    account: "Email",
    accountRequired: true,
    from: "Item",
    hint: "Needs the `bw` CLI. Run `bw login` once in a terminal, then export BW_SESSION (from `bw unlock --raw`) in your shell rc: kamal can't prompt for the master password here.",
  },
  "bitwarden-sm": {
    label: "Bitwarden Secrets Manager",
    from: "Project ID (optional)",
    hint: "Needs the `bws` CLI and BWS_ACCESS_TOKEN exported in your shell rc. Fetches all secrets (of the project, if set) and picks keys by name.",
  },
  lastpass: {
    label: "LastPass",
    account: "Email",
    accountRequired: true,
    from: "Folder (optional)",
    hint: "Needs the `lpass` CLI. Run `lpass login <email>` in a terminal first; set LPASS_AGENT_TIMEOUT=0 so the session doesn't expire.",
  },
  aws_secrets_manager: {
    label: "AWS Secrets Manager",
    account: "AWS profile (optional)",
    from: "Secret name",
    hint: "Needs the `aws` CLI with credentials for the profile (run `aws sso login` in a terminal if you use SSO). Keys are read from the secret's JSON.",
  },
  gcp: {
    label: "Google Secret Manager",
    account: "Account (default or email)",
    accountRequired: true,
    from: "Project (optional)",
    hint: "Needs the `gcloud` CLI. Run `gcloud auth login` in a terminal first. Each key is a secret of the same name.",
  },
  doppler: {
    label: "Doppler",
    from: "project/config",
    hint: "Needs the `doppler` CLI. Run `doppler login` in a terminal, or export a DOPPLER_TOKEN service token in your shell rc (then project/config can be left empty).",
  },
  enpass: {
    label: "Enpass",
    from: "Vault path",
    hint: "Needs `enpass-cli`, able to open the vault without a prompt. Each key is an item title (or Item/field).",
  },
  passbolt: {
    label: "Passbolt",
    from: "Folder (optional)",
    hint: "Needs the `passbolt` CLI, configured so `passbolt verify` passes in a terminal.",
  },
};

const SOURCES: Record<SecretSource, string> = { vault: "Password manager", env: "Environment", skip: "Skip" };

type Props = { projectId: number; destination: string | null; onCheck: () => void; onClose: () => void };

/** Generates `.kamal/secrets` lines to copy. kdm never writes the file or reads secret values. */
export function SecretsDialog({ projectId, destination, onCheck, onClose }: Props) {
  const [scan, setScan] = useState<SecretsScan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adapter, setAdapter] = useState("1password");
  const [account, setAccount] = useState("");
  const [from, setFrom] = useState("");
  const [sources, setSources] = useState<Record<string, SecretSource>>({});
  const [missingOnly, setMissingOnly] = useState(true);
  const [newKey, setNewKey] = useState("");
  const [snippet, setSnippet] = useState("");
  const [snippetError, setSnippetError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    api.secretsScan(projectId, destination).then(setScan, (e) => setError(String(e)));
  }, [projectId, destination]);

  const keys = scan?.keys ?? [];
  const defined = (name: string) => keys.find((k) => k.name === name)?.defined_in ?? null;
  const sourceOf = (name: string): SecretSource => (missingOnly && defined(name) ? "skip" : (sources[name] ?? "vault"));
  const meta = ADAPTERS[adapter];

  useEffect(() => {
    if (!scan) return;
    const request = {
      file: scan.file,
      adapter,
      account: meta.account ? account : null,
      from: meta.from ? from : null,
      keys: scan.keys.map((k) => ({ name: k.name, source: sourceOf(k.name) })),
    };
    api.secretsSnippet(request).then(
      (text) => {
        setSnippet(text);
        setSnippetError(null);
      },
      (e) => setSnippetError(String(e)),
    );
    // sourceOf reads only the state listed here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scan, adapter, account, from, sources, missingOnly]);

  function addKey() {
    const name = newKey.trim();
    if (!scan || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) || scan.keys.some((k) => k.name === name)) return;
    setScan({ ...scan, keys: [...scan.keys, { name, defined_in: null }] });
    setNewKey("");
  }

  async function copy() {
    await navigator.clipboard.writeText(snippet);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  const usesVault = keys.some((k) => sourceOf(k.name) === "vault");

  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog size="xl" className="flex max-h-[85vh] flex-col gap-4 overflow-auto p-6">
        <Dialog.Title className="text-lg font-semibold">Set up secrets</Dialog.Title>
        <Dialog.Description className="text-kumo-subtle">
          Generates lines for <code className="font-mono">{scan?.file ?? ".kamal/secrets"}</code> that fetch secrets from your password manager.
          Kamal Desktop Manager doesn't write the file or read any secret values.
        </Dialog.Description>
        {error && <ErrorNotice error={error} />}

        {scan && (
          <>
            <div className="flex flex-wrap items-end gap-2">
              <Select
                label="Password manager"
                size="sm"
                className="w-56"
                value={adapter}
                onValueChange={(v) => v && setAdapter(String(v))}
                items={Object.fromEntries(Object.entries(ADAPTERS).map(([id, a]) => [id, a.label]))}
              />
              {meta.account && (
                <Input size="sm" className="w-56" label={meta.account} value={account} onChange={(e) => setAccount(e.target.value)} />
              )}
              {meta.from && <Input size="sm" className="w-56" label={meta.from} value={from} onChange={(e) => setFrom(e.target.value)} />}
            </div>
            <Text variant="secondary" size="sm">
              {meta.hint}
            </Text>

            <div className="flex items-center justify-between gap-2">
              <Text size="sm" bold>
                Secrets used by the deploy config
              </Text>
              <Switch label="Missing keys only" checked={missingOnly} onCheckedChange={setMissingOnly} />
            </div>
            {keys.length === 0 && (
              <Text variant="secondary" size="sm">
                No secret names found in the deploy config. Add them below.
              </Text>
            )}
            <div className="flex flex-col gap-1">
              {keys.map((k) => (
                <div key={k.name} className="flex items-center gap-2">
                  <code className="min-w-0 flex-1 truncate font-mono text-sm">{k.name}</code>
                  {k.defined_in && <Badge variant="secondary">in {k.defined_in}</Badge>}
                  <Select
                    aria-label={`Source for ${k.name}`}
                    size="xs"
                    className="w-40"
                    disabled={missingOnly && !!k.defined_in}
                    value={sourceOf(k.name)}
                    onValueChange={(v) => v && setSources({ ...sources, [k.name]: v as SecretSource })}
                    items={SOURCES}
                  />
                </div>
              ))}
            </div>
            <form
              className="flex items-end gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                addKey();
              }}
            >
              <Input size="sm" className="w-56" aria-label="Add a secret name" placeholder="ADD_SECRET_NAME" value={newKey} onChange={(e) => setNewKey(e.target.value)} />
              <Button type="submit" size="sm" variant="secondary" icon={<PlusIcon />}>
                Add
              </Button>
            </form>

            {snippetError ? (
              <ErrorNotice error={snippetError} />
            ) : (
              <pre className="overflow-auto rounded-md border border-kumo-fill bg-kumo-base p-3 font-mono text-xs whitespace-pre">{snippet}</pre>
            )}
            <Text variant="secondary" size="sm">
              {missingOnly ? "Append these lines to" : "Use this as"} <code className="font-mono">{scan.file}</code>
              {usesVault && ", make sure the CLI above can sign in without a prompt,"} then Check. Values come from the password manager when kamal runs.
            </Text>
          </>
        )}

        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
          <Button variant="secondary" icon={<CopyIcon />} disabled={!snippet || !!snippetError} onClick={copy}>
            {copied ? "Copied" : "Copy"}
          </Button>
          <Button
            variant="primary"
            onClick={async () => {
              await api.projectEnvReset(projectId);
              onClose();
              onCheck();
            }}
          >
            Check
          </Button>
        </div>
      </Dialog>
    </Dialog.Root>
  );
}
