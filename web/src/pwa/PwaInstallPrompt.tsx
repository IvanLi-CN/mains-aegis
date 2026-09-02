import * as Dialog from "@radix-ui/react-dialog";
import { Download, ExternalLink, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "../components/ui/button";
import type { PwaInstallAvailability } from "./pwaInstall";

export type PwaInstallPromptProps = {
  availability: PwaInstallAvailability;
  visible: boolean;
  onInstall: () => void | Promise<void>;
  onDismiss: () => void;
  iosGuideOpen?: boolean;
  onIosGuideOpenChange?: (open: boolean) => void;
  placement?: "fixed" | "inline";
};

export function canShowAutomaticPwaInstall(input: {
  routeSection: string;
  demoMode: boolean;
  updatePromptVisible: boolean;
  dialogOpen: boolean;
  availability: PwaInstallAvailability;
  isInstalled: boolean;
  automaticSnoozed: boolean;
  sessionHidden: boolean;
}): boolean {
  return (
    (input.routeSection === "fleet" || input.routeSection === "connect") &&
    !input.demoMode &&
    !input.updatePromptVisible &&
    !input.dialogOpen &&
    !input.isInstalled &&
    input.availability !== "unavailable" &&
    !input.automaticSnoozed &&
    !input.sessionHidden
  );
}

export function PwaInstallPrompt({
  availability,
  visible,
  onInstall,
  onDismiss,
  iosGuideOpen = false,
  onIosGuideOpenChange,
  placement = "fixed",
}: PwaInstallPromptProps) {
  const isIosGuide = availability === "ios-guide";
  const title = "Install Mains Aegis Web";
  const description = isIosGuide
    ? "Add the console to your home screen for quick access."
    : "Install the console for a dedicated window and quick access.";
  const className = [
    "pwa-install-prompt",
    placement === "inline" ? "is-inline" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <>
      {visible ? (
        <aside
          className={className}
          aria-live="polite"
          aria-label="Mains Aegis Web installation"
        >
          <div className="pwa-install-icon" aria-hidden="true">
            <Download size={18} />
          </div>
          <div className="pwa-install-copy">
            <strong>{title}</strong>
            <span>{description}</span>
          </div>
          <Button
            type="button"
            size="sm"
            className="pwa-install-action"
            onClick={() => void onInstall()}
          >
            {isIosGuide ? <ExternalLink size={14} /> : <Download size={14} />}
            <span>{isIosGuide ? "View steps" : "Install"}</span>
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            className="pwa-install-dismiss"
            aria-label="Dismiss install recommendation"
            onClick={onDismiss}
          >
            <X size={16} />
          </Button>
        </aside>
      ) : null}
      <Dialog.Root
        open={iosGuideOpen}
        onOpenChange={(open) => onIosGuideOpenChange?.(open)}
      >
        <Dialog.Portal>
          <Dialog.Overlay className="pwa-install-dialog-overlay" />
          <Dialog.Content
            className="pwa-install-dialog"
            aria-describedby="pwa-install-guide-description"
          >
            <Dialog.Title className="pwa-install-dialog-title">
              Install Mains Aegis Web
            </Dialog.Title>
            <Dialog.Description
              id="pwa-install-guide-description"
              className="pwa-install-dialog-description"
            >
              Open the browser Share menu, choose Add to Home Screen, then select Add.
            </Dialog.Description>
            <div className="pwa-install-dialog-actions">
              <Dialog.Close asChild>
                <Button type="button" variant="secondary">
                  Later
                </Button>
              </Dialog.Close>
              <Dialog.Close asChild>
                <Button type="button">
                  <Download size={15} />
                  <span>Done</span>
                </Button>
              </Dialog.Close>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </>
  );
}

export function usePwaInstallRecommendation(options: {
  routeSection: "fleet" | "connect" | string;
  demoMode: boolean;
  updatePromptVisible: boolean;
  availability: PwaInstallAvailability;
  isInstalled: boolean;
  automaticSnoozed: boolean;
  sessionHidden: boolean;
}): boolean {
  const [qualifiedInteraction, setQualifiedInteraction] = useState(false);
  const [timerElapsed, setTimerElapsed] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  const eligible = canShowAutomaticPwaInstall({
    routeSection: options.routeSection,
    demoMode: options.demoMode,
    updatePromptVisible: options.updatePromptVisible,
    dialogOpen,
    availability: options.availability,
    isInstalled: options.isInstalled,
    automaticSnoozed: options.automaticSnoozed,
    sessionHidden: options.sessionHidden,
  });

  useEffect(() => {
    if (typeof document === "undefined") return undefined;
    const readDialogState = () => {
      setDialogOpen(Boolean(document.body.querySelector('[role="dialog"]')));
    };
    readDialogState();
    const observer = new MutationObserver(readDialogState);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    setQualifiedInteraction(false);
    setTimerElapsed(false);
    if (!eligible || dialogOpen) return undefined;

    const onClick = () => setQualifiedInteraction(true);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Enter" || event.key === " ") setQualifiedInteraction(true);
    };
    document.addEventListener("click", onClick, { once: true });
    document.addEventListener("keydown", onKeyDown, { once: true });
    return () => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [dialogOpen, eligible]);

  useEffect(() => {
    if (!eligible || dialogOpen || !qualifiedInteraction) {
      setTimerElapsed(false);
      return undefined;
    }
    const timeout = window.setTimeout(() => setTimerElapsed(true), 5_000);
    return () => window.clearTimeout(timeout);
  }, [dialogOpen, eligible, qualifiedInteraction]);

  return eligible && !dialogOpen && qualifiedInteraction && timerElapsed;
}
