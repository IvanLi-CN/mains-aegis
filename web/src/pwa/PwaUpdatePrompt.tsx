import * as Dialog from "@radix-ui/react-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  Loader2,
  RefreshCw,
  X,
} from "lucide-react";
import { useState } from "react";
import { Button } from "../components/ui/button";

export type PwaUpdateStatus =
  | "idle"
  | "offlineReady"
  | "ready"
  | "activating"
  | "updated"
  | "error";

export type PwaUpdateSnapshot = {
  status: PwaUpdateStatus;
  error: string | null;
};

export type PwaUpdatePromptProps = {
  snapshot: PwaUpdateSnapshot;
  onUpdate: () => void;
  onDismiss?: () => void;
  placement?: "fixed" | "inline";
};

export function shouldShowPwaUpdatePrompt(status: PwaUpdateStatus): boolean {
  return (
    status === "offlineReady" ||
    status === "ready" ||
    status === "activating" ||
    status === "error"
  );
}

export function PwaUpdatePrompt({
  snapshot,
  onUpdate,
  onDismiss,
  placement = "fixed",
}: PwaUpdatePromptProps) {
  const [confirmOpen, setConfirmOpen] = useState(false);
  if (!shouldShowPwaUpdatePrompt(snapshot.status)) return null;

  const isReady = snapshot.status === "ready";
  const isActivating = snapshot.status === "activating";
  const isOfflineReady = snapshot.status === "offlineReady";
  const isError = snapshot.status === "error";
  const title = isReady
    ? "New version available"
    : isActivating
      ? "Updating Mains Aegis Web"
      : isOfflineReady
        ? "Ready for offline use"
        : "Update check failed";
  const description = isReady
    ? "The new app shell has been downloaded. Update when you are ready to refresh this page."
    : isActivating
      ? "Switching to the downloaded version. The page will refresh shortly."
      : isOfflineReady
        ? "This browser can reopen the app shell after the first successful load."
        : (snapshot.error ?? "The service worker could not be registered.");
  const Icon = isReady
    ? RefreshCw
    : isActivating
      ? Loader2
      : isOfflineReady
        ? CheckCircle2
        : AlertTriangle;
  const className = [
    "pwa-update-prompt",
    placement === "inline" ? "is-inline" : "",
    isError ? "is-error" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <aside
      className={className}
      aria-live="polite"
      aria-label="Mains Aegis Web update status"
    >
      <div className="pwa-update-icon" aria-hidden="true">
        <Icon size={18} className={isActivating ? "spin-icon" : undefined} />
      </div>
      <div className="pwa-update-copy">
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
      {isReady || isActivating ? (
        <Button
          type="button"
          size="sm"
          onClick={() => setConfirmOpen(true)}
          disabled={!isReady}
          className="pwa-update-action"
        >
          {isActivating ? (
            <Loader2 size={14} className="spin-icon" />
          ) : (
            <RefreshCw size={14} />
          )}
          <span>{isActivating ? "Updating" : "Update"}</span>
        </Button>
      ) : null}
      {onDismiss ? (
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="pwa-update-dismiss"
          aria-label="Dismiss update status"
          onClick={onDismiss}
        >
          <X size={16} />
        </Button>
      ) : null}
      <Dialog.Root open={confirmOpen} onOpenChange={setConfirmOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="pwa-update-dialog-overlay" />
          <Dialog.Content
            className="pwa-update-dialog"
            aria-describedby="pwa-update-confirm-description"
          >
            <Dialog.Title className="pwa-update-dialog-title">Update Mains Aegis Web</Dialog.Title>
            <Dialog.Description
              id="pwa-update-confirm-description"
              className="pwa-update-dialog-description"
            >
              Updating will refresh the current page and switch to the downloaded app shell. Finish any device operation before continuing.
            </Dialog.Description>
            <div className="pwa-update-dialog-actions">
              <Dialog.Close asChild>
                <Button type="button" variant="secondary">
                  Later
                </Button>
              </Dialog.Close>
              <Button
                type="button"
                onClick={() => {
                  setConfirmOpen(false);
                  onUpdate();
                }}
              >
                <RefreshCw size={15} />
                <span>Update</span>
              </Button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </aside>
  );
}
