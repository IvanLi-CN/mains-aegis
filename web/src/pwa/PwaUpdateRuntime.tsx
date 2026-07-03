import { useCallback, useEffect, useRef, useState } from "react";
import {
  PwaUpdatePrompt,
  type PwaUpdateSnapshot,
} from "./PwaUpdatePrompt";
import {
  registerMainsAegisServiceWorker,
  type PwaUpdateServiceWorker,
} from "./serviceWorkerRegistration";

const idleSnapshot: PwaUpdateSnapshot = { status: "idle", error: null };

export function PwaUpdateRuntime() {
  const [snapshot, setSnapshot] = useState<PwaUpdateSnapshot>(idleSnapshot);
  const updateServiceWorker = useRef<PwaUpdateServiceWorker | null>(null);

  useEffect(() => {
    if (!import.meta.env.PROD || typeof window === "undefined") return;
    updateServiceWorker.current = registerMainsAegisServiceWorker({
      immediate: true,
      onNeedRefresh() {
        setSnapshot({ status: "ready", error: null });
      },
      onOfflineReady() {
        setSnapshot({ status: "offlineReady", error: null });
      },
      onRegisterError(error) {
        setSnapshot({
          status: "error",
          error: error instanceof Error ? error.message : String(error),
        });
      },
    });
  }, []);

  const applyUpdate = useCallback(() => {
    if (snapshot.status !== "ready") return;
    setSnapshot({ status: "activating", error: null });
    const update = updateServiceWorker.current;
    if (!update) {
      setSnapshot({
        status: "error",
        error: "The service worker update handler is not available.",
      });
      return;
    }
    void update(true).catch((error: unknown) => {
      setSnapshot({
        status: "error",
        error: error instanceof Error ? error.message : String(error),
      });
    });
  }, [snapshot.status]);

  return (
    <PwaUpdatePrompt
      snapshot={snapshot}
      onUpdate={applyUpdate}
      onDismiss={() => setSnapshot(idleSnapshot)}
    />
  );
}
