import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import {
  resolveSpaFallbackInitialPath,
  restoreSpaFallbackHash,
} from "./app/spaFallback";
import { DeviceRegistryProvider } from "./device-registry/DeviceRegistry";
import { PwaUpdateRuntime } from "./pwa/PwaUpdateRuntime";
import "./styles/tokens.css";
import "./styles/globals.css";

const searchParams =
  typeof window === "undefined"
    ? new URLSearchParams()
    : new URLSearchParams(window.location.search);
const initialPath = resolveSpaFallbackInitialPath(searchParams);
restoreSpaFallbackHash(searchParams);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <DeviceRegistryProvider>
      <App initialPath={initialPath} />
      <PwaUpdateRuntime />
    </DeviceRegistryProvider>
  </StrictMode>,
);
