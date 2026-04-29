import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { DeviceRegistryProvider } from "./device-registry/DeviceRegistry";
import "./styles/tokens.css";
import "./styles/globals.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <DeviceRegistryProvider>
      <App />
    </DeviceRegistryProvider>
  </StrictMode>,
);
