import { Workbox } from "workbox-window";
import { resolveServiceWorkerTarget } from "./serviceWorkerTarget";

export type PwaServiceWorkerRegistrationOptions = {
  immediate?: boolean;
  onNeedReload?: () => void;
  onNeedRefresh?: () => void;
  onOfflineReady?: () => void;
  onRegisteredSW?: (
    swScriptUrl: string,
    registration: ServiceWorkerRegistration | undefined,
  ) => void;
  onRegisterError?: (error: unknown) => void;
};

export type PwaUpdateServiceWorker = (reloadPage?: boolean) => Promise<void>;

export function registerMainsAegisServiceWorker(
  options: PwaServiceWorkerRegistrationOptions = {},
): PwaUpdateServiceWorker {
  const {
    immediate = false,
    onNeedReload,
    onNeedRefresh,
    onOfflineReady,
    onRegisteredSW,
    onRegisterError,
  } = options;

  let workbox: Workbox | undefined;
  let sendSkipWaitingMessage: (() => void) | undefined;
  const target = resolveServiceWorkerTarget(
    import.meta.env.BASE_URL,
    window.location.pathname,
  );

  const registerPromise = (async () => {
    if (!("serviceWorker" in navigator)) return;

    try {
      workbox = new Workbox(target.scriptUrl, {
        scope: target.scope,
        type: "classic",
      });
      sendSkipWaitingMessage = () => {
        workbox?.messageSkipWaiting();
      };

      let onNeedRefreshCalled = false;
      const showSkipWaitingPrompt = () => {
        onNeedRefreshCalled = true;
        workbox?.addEventListener("controlling", (event) => {
          if (!event.isUpdate) return;
          if (onNeedReload) {
            onNeedReload();
          } else {
            window.location.reload();
          }
        });
        onNeedRefresh?.();
      };

      workbox.addEventListener("installed", (event) => {
        if (typeof event.isUpdate === "undefined") {
          if (typeof event.isExternal !== "undefined") {
            if (event.isExternal) {
              showSkipWaitingPrompt();
            } else if (!onNeedRefreshCalled) {
              onOfflineReady?.();
            }
          } else if (!onNeedRefreshCalled) {
            onOfflineReady?.();
          }
        } else if (!event.isUpdate) {
          onOfflineReady?.();
        }
      });
      workbox.addEventListener("waiting", showSkipWaitingPrompt);

      const registration = await workbox.register({ immediate });
      onRegisteredSW?.(target.scriptUrl, registration);
    } catch (error) {
      onRegisterError?.(error);
    }
  })();

  return async () => {
    await registerPromise;
    sendSkipWaitingMessage?.();
  };
}
