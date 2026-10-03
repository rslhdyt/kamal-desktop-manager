import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, hostKey, HostSnapshot, SshTarget } from "./api";

/** Live snapshot for a host. The backend polls each host once, however many views watch it. */
export function useHost(target: SshTarget): HostSnapshot | null {
  const [snapshot, setSnapshot] = useState<HostSnapshot | null>(null);
  const key = hostKey(target);

  useEffect(() => {
    let active = true;
    setSnapshot(null);
    const unlisten = listen<HostSnapshot>("host:update", (e) => {
      if (active && e.payload.key === key) setSnapshot(e.payload);
    });
    api.hostWatch(target).then((s) => active && s && setSnapshot(s));
    return () => {
      active = false;
      unlisten.then((f) => f());
      api.hostUnwatch(target);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return snapshot;
}
