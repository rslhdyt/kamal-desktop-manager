import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Button } from "@cloudflare/kumo";
import { ArrowCircleUpIcon } from "@phosphor-icons/react";

/** Version footer; offers a signed update from the releases feed when one exists. */
export function UpdateBanner() {
  const [version, setVersion] = useState("");
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setVersion);
    // Dev builds and offline starts just stay quiet.
    check().then(setUpdate, () => {});
  }, []);

  async function install() {
    if (!update) return;
    let total = 0;
    let done = 0;
    try {
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        if (e.event === "Progress") {
          done += e.data.chunkLength;
          setProgress(total ? `${Math.round((100 * done) / total)}%` : "downloading…");
        }
        if (e.event === "Finished") setProgress("restarting…");
      });
      await relaunch();
    } catch (e) {
      setProgress(`update failed: ${e}`);
    }
  }

  return (
    <div className="mt-auto flex flex-col gap-1 px-2 pt-2 text-xs text-kumo-subtle">
      {update && (
        <Button size="xs" variant="primary" icon={<ArrowCircleUpIcon />} disabled={progress !== null} onClick={install}>
          {progress ?? `Update to ${update.version}`}
        </Button>
      )}
      {version && <span>Kamal Desktop Manager {version}</span>}
    </div>
  );
}
