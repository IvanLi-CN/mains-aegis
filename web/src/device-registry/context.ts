import { createContext, useContext } from "react";
import type { DeviceRecord, SafeSettingsState, WifiApplyNetwork } from "../api/types";
import type { SerialPortLike } from "../serial/transport";

export type AddDeviceInput = {
  target: string;
  alias?: string;
  location?: string;
  devdDeviceId?: string;
  ignoreFirmwareMismatch?: boolean;
};

export type AddDeviceResult =
  | { ok: true; record: DeviceRecord }
  | { ok: false; error: DeviceRecord["error"] };

export type CommandResult =
  | { ok: true; message?: string; network?: WifiApplyNetwork }
  | { ok: false; error: DeviceRecord["error"] };

export type WifiConfigInput = {
  ssid: string;
  psk: string;
};

export type WifiProvisioningProgress = {
  phase: "saving" | "clearing" | "starting" | "connecting" | "ip" | "connected" | "disabled";
  message: string;
  network?: WifiApplyNetwork;
};

export type ManualChargePrefsInput = SafeSettingsState["manual_charge"];

export type DeviceRegistryContextValue = {
  records: DeviceRecord[];
  addDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  addDevdDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  connectUsbSerialDevice: (input?: Pick<AddDeviceInput, "alias" | "location" | "ignoreFirmwareMismatch">) => Promise<AddDeviceResult>;
  prepareWebSerialFlashPort: (deviceId: string) => Promise<SerialPortLike | null>;
  attachMockUsbSerialDevice: () => AddDeviceResult;
  disconnectUsbSerialDevice: (deviceId: string) => Promise<void>;
  sendWifiConfig: (deviceId: string, input: WifiConfigInput, onProgress?: (progress: WifiProvisioningProgress) => void) => Promise<CommandResult>;
  clearWifiConfig: (deviceId: string, onProgress?: (progress: WifiProvisioningProgress) => void) => Promise<CommandResult>;
  setSerialLogLevel: (deviceId: string, level: SafeSettingsState["log_level"]) => Promise<CommandResult>;
  setManualChargePrefs: (deviceId: string, prefs: ManualChargePrefsInput) => Promise<CommandResult>;
  removeDevice: (deviceId: string) => void;
  refreshDevice: (deviceId: string) => Promise<void>;
  resetDemo: () => void;
};

export const DeviceRegistryContext = createContext<DeviceRegistryContextValue | null>(null);

export function useDeviceRegistry(): DeviceRegistryContextValue {
  const context = useContext(DeviceRegistryContext);
  if (!context) throw new Error("useDeviceRegistry must be used inside DeviceRegistryProvider");
  return context;
}
