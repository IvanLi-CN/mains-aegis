import { createContext, useContext } from "react";
import type { DeviceRecord, SafeSettingsState } from "../api/types";

export type AddDeviceInput = {
  target: string;
  alias?: string;
  location?: string;
};

export type AddDeviceResult =
  | { ok: true; record: DeviceRecord }
  | { ok: false; error: DeviceRecord["error"] };

export type CommandResult = { ok: true } | { ok: false; error: DeviceRecord["error"] };

export type WifiConfigInput = {
  ssid: string;
  psk: string;
};

export type ManualChargePrefsInput = SafeSettingsState["manual_charge"];

export type DeviceRegistryContextValue = {
  records: DeviceRecord[];
  addDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  addLocalAdapterDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  connectUsbSerialDevice: (input?: Pick<AddDeviceInput, "alias" | "location">) => Promise<AddDeviceResult>;
  attachMockUsbSerialDevice: () => AddDeviceResult;
  disconnectUsbSerialDevice: (deviceId: string) => Promise<void>;
  sendWifiConfig: (deviceId: string, input: WifiConfigInput) => Promise<CommandResult>;
  clearWifiConfig: (deviceId: string) => Promise<CommandResult>;
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
