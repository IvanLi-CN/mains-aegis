export const PWA_INSTALL_SNOOZE_KEY = "mains-aegis-web.pwa-install-snooze.v1";
export const PWA_INSTALL_SNOOZE_MS = 30 * 24 * 60 * 60 * 1000;

export type PwaInstallAvailability = "native" | "ios-guide" | "unavailable";

export type PwaInstallChoice = "accepted" | "dismissed";

export type BeforeInstallPromptEventLike = Event & {
  prompt: () => Promise<void>;
  userChoice: Promise<{ outcome: PwaInstallChoice }>;
};

export async function requestNativePwaInstall(
  event: BeforeInstallPromptEventLike,
): Promise<PwaInstallChoice> {
  await event.prompt();
  const choice = await event.userChoice;
  return choice.outcome;
}

export type PwaInstallPlatform = {
  userAgent?: string;
  maxTouchPoints?: number;
  standalone?: boolean;
  matchMedia?: (query: string) => { matches: boolean };
};

export function isIosDevice({
  userAgent = "",
  maxTouchPoints = 0,
}: PwaInstallPlatform = {}): boolean {
  return /iPhone|iPad|iPod/i.test(userAgent) ||
    (/Macintosh/i.test(userAgent) && maxTouchPoints > 1);
}

export function isStandaloneDisplayMode({
  standalone = false,
  matchMedia,
}: PwaInstallPlatform = {}): boolean {
  return standalone || Boolean(matchMedia?.("(display-mode: standalone)").matches);
}

export function resolvePwaInstallAvailability(input: {
  isInstalled: boolean;
  hasNativePrompt: boolean;
  isIos: boolean;
}): PwaInstallAvailability {
  if (input.isInstalled) return "unavailable";
  if (input.hasNativePrompt) return "native";
  if (input.isIos) return "ios-guide";
  return "unavailable";
}

export function readPwaInstallSnooze(
  storage: Pick<Storage, "getItem" | "removeItem"> | null | undefined,
  now = Date.now(),
): number | null {
  if (!storage) return null;
  try {
    const raw = storage.getItem(PWA_INSTALL_SNOOZE_KEY);
    if (!raw) return null;
    const until = Number(raw);
    if (!Number.isFinite(until) || until <= now) {
      storage.removeItem(PWA_INSTALL_SNOOZE_KEY);
      return null;
    }
    return until;
  } catch {
    return null;
  }
}

export function writePwaInstallSnooze(
  storage: Pick<Storage, "setItem"> | null | undefined,
  now = Date.now(),
): number {
  const until = now + PWA_INSTALL_SNOOZE_MS;
  try {
    storage?.setItem(PWA_INSTALL_SNOOZE_KEY, String(until));
  } catch {
    // Private browsing and blocked storage must not prevent manual install.
  }
  return until;
}

export function clearPwaInstallSnooze(
  storage: Pick<Storage, "removeItem"> | null | undefined,
): void {
  try {
    storage?.removeItem(PWA_INSTALL_SNOOZE_KEY);
  } catch {
    // Storage cleanup is best effort.
  }
}
