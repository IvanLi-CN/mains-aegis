import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  clearPwaInstallSnooze,
  isIosDevice,
  isStandaloneDisplayMode,
  readPwaInstallSnooze,
  requestNativePwaInstall,
  resolvePwaInstallAvailability,
  type BeforeInstallPromptEventLike,
  type PwaInstallAvailability,
  writePwaInstallSnooze,
} from "./pwaInstall";

export type PwaInstallRequestResult =
  | "accepted"
  | "dismissed"
  | "ios-guide"
  | "unavailable"
  | "failed";

export type PwaInstallController = {
  availability: PwaInstallAvailability;
  isInstalled: boolean;
  automaticSnoozed: boolean;
  sessionHidden: boolean;
  iosGuideOpen: boolean;
  requestInstall: () => Promise<PwaInstallRequestResult>;
  dismissAutomaticRecommendation: () => void;
  closeIosGuide: () => void;
};

const unavailableController: PwaInstallController = {
  availability: "unavailable",
  isInstalled: false,
  automaticSnoozed: false,
  sessionHidden: false,
  iosGuideOpen: false,
  requestInstall: async () => "unavailable",
  dismissAutomaticRecommendation: () => undefined,
  closeIosGuide: () => undefined,
};

const PwaInstallContext = createContext<PwaInstallController>(unavailableController);

function readStandaloneState(): boolean {
  if (typeof window === "undefined") return false;
  const standalone = (navigator as Navigator & { standalone?: boolean }).standalone;
  return isStandaloneDisplayMode({
    standalone,
    matchMedia:
      typeof window.matchMedia === "function"
        ? (query) => window.matchMedia(query)
        : undefined,
  });
}

function readStorage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

function readIosState(): boolean {
  if (typeof navigator === "undefined") return false;
  return isIosDevice({
    userAgent: navigator.userAgent,
    maxTouchPoints: navigator.maxTouchPoints,
  });
}

export function PwaInstallRuntime({
  children,
  initialNativePrompt,
}: {
  children: ReactNode;
  initialNativePrompt?: BeforeInstallPromptEventLike;
}) {
  const deferredPrompt = useRef<BeforeInstallPromptEventLike | null>(
    initialNativePrompt ?? null,
  );
  const [hasNativePrompt, setHasNativePrompt] = useState(
    initialNativePrompt !== undefined,
  );
  const [isInstalled, setIsInstalled] = useState(readStandaloneState);
  const [automaticSnoozed, setAutomaticSnoozed] = useState(() =>
    readPwaInstallSnooze(readStorage()) !== null,
  );
  const [sessionHidden, setSessionHidden] = useState(false);
  const [iosGuideOpen, setIosGuideOpen] = useState(false);
  const iosDevice = readIosState();

  const clearSnooze = useCallback(() => {
    clearPwaInstallSnooze(readStorage());
    setAutomaticSnoozed(false);
  }, []);

  const snooze = useCallback(() => {
    const until = writePwaInstallSnooze(readStorage());
    setAutomaticSnoozed(until > Date.now());
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") return undefined;

    const onBeforeInstallPrompt = (event: Event) => {
      event.preventDefault();
      deferredPrompt.current = event as BeforeInstallPromptEventLike;
      setHasNativePrompt(true);
      setSessionHidden(false);
    };
    const onAppInstalled = () => {
      deferredPrompt.current = null;
      setHasNativePrompt(false);
      setIsInstalled(true);
      setSessionHidden(true);
      setIosGuideOpen(false);
      clearSnooze();
    };

    window.addEventListener("beforeinstallprompt", onBeforeInstallPrompt);
    window.addEventListener("appinstalled", onAppInstalled);

    const media =
      typeof window.matchMedia === "function"
        ? window.matchMedia("(display-mode: standalone)")
        : null;
    const onDisplayModeChange = () => {
      if (!media?.matches) return;
      setIsInstalled(true);
      setSessionHidden(true);
      clearSnooze();
    };
    if (media) {
      media.addEventListener?.("change", onDisplayModeChange);
      if (!media.addEventListener) media.addListener?.(onDisplayModeChange);
      if (media.matches) onDisplayModeChange();
    }

    return () => {
      window.removeEventListener("beforeinstallprompt", onBeforeInstallPrompt);
      window.removeEventListener("appinstalled", onAppInstalled);
      if (media) {
        media.removeEventListener?.("change", onDisplayModeChange);
        if (!media.removeEventListener) media.removeListener?.(onDisplayModeChange);
      }
    };
  }, [clearSnooze]);

  const availability = resolvePwaInstallAvailability({
    isInstalled,
    hasNativePrompt,
    isIos: iosDevice,
  });

  const requestInstall = useCallback(async (): Promise<PwaInstallRequestResult> => {
    if (isInstalled) return "unavailable";
    const event = deferredPrompt.current;
    if (event) {
      deferredPrompt.current = null;
      setHasNativePrompt(false);
      setSessionHidden(true);
      try {
        const choice = await requestNativePwaInstall(event);
        if (choice === "dismissed") {
          snooze();
          return "dismissed";
        }
        return "accepted";
      } catch {
        setSessionHidden(false);
        return "failed";
      }
    }
    if (iosDevice) {
      setIosGuideOpen(true);
      return "ios-guide";
    }
    return "unavailable";
  }, [iosDevice, isInstalled, snooze]);

  const dismissAutomaticRecommendation = useCallback(() => {
    setSessionHidden(true);
    snooze();
  }, [snooze]);

  const closeIosGuide = useCallback(() => {
    setIosGuideOpen(false);
    snooze();
  }, [snooze]);

  const value = useMemo<PwaInstallController>(
    () => ({
      availability,
      isInstalled,
      automaticSnoozed,
      sessionHidden,
      iosGuideOpen,
      requestInstall,
      dismissAutomaticRecommendation,
      closeIosGuide,
    }),
    [
      automaticSnoozed,
      availability,
      closeIosGuide,
      dismissAutomaticRecommendation,
      iosGuideOpen,
      isInstalled,
      requestInstall,
      sessionHidden,
    ],
  );

  return <PwaInstallContext.Provider value={value}>{children}</PwaInstallContext.Provider>;
}

export function usePwaInstall(): PwaInstallController {
  return useContext(PwaInstallContext);
}
