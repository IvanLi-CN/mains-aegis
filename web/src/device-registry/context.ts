import { createContext, useContext } from "react";
import type {
  AdvancedPowerSettings,
  ChargeControlDetail,
  DeviceRecord,
  DeviceSettings,
  DeviceTarget,
  DevdDevice,
  WifiApplyNetwork,
} from "../api/types";
import type { SerialPortLike } from "../serial/transport";
import type { DemoSeed } from "../fixtures/mockDevices";

export type AddDeviceInput = {
  target: string;
  alias?: string;
  location?: string;
  devdDeviceId?: string;
  ignoreFirmwareMismatch?: boolean;
  rememberedHttpBaseUrl?: string;
  rememberedHttpMdnsHost?: string;
  rememberedHttpFallbackBaseUrl?: string;
};

export type DeviceChannelTransport = NonNullable<DeviceTarget["transport"]>;

export type AddDeviceResult =
  | { ok: true; record: DeviceRecord }
  | { ok: false; error: DeviceRecord["error"] };

export type CommandResult =
  | {
      ok: true;
      message?: string;
      network?: WifiApplyNetwork;
      detail?: ChargeControlDetail | null;
    }
  | {
      ok: false;
      error: DeviceRecord["error"];
      detail?: ChargeControlDetail | null;
    };

export type WifiConfigInput = {
  ssid: string;
  psk: string;
};

export type WifiProvisioningProgress = {
  phase: "saving" | "clearing" | "starting" | "connecting" | "ip" | "connected" | "disabled";
  message: string;
  network?: WifiApplyNetwork;
};

export type ManualChargePrefsInput = DeviceSettings["manual_charge"];
export type ManualChargeControlInput = {
  action: "start" | "stop";
  confirm_loop?: boolean;
};
export type AdvancedPowerInput = AdvancedPowerSettings;

export type DeviceRegistryContextValue = {
  records: DeviceRecord[];
  demoSeed: DemoSeed | null;
  stageDeviceRecord: (record: DeviceRecord) => void;
  addDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  addDevdDevice: (input: AddDeviceInput) => Promise<AddDeviceResult>;
  confirmDevdCompanionLan: (deviceId: string, devdBaseUrl: string) => Promise<AddDeviceResult>;
  dismissDevdCompanionLan: (deviceId: string, devdBaseUrl: string) => Promise<AddDeviceResult>;
  connectUsbSerialDevice: (input?: Pick<AddDeviceInput, "alias" | "location" | "ignoreFirmwareMismatch">) => Promise<AddDeviceResult>;
  connectKnownDeviceChannel: (deviceId: string, transport: DeviceChannelTransport, options?: Pick<AddDeviceInput, "ignoreFirmwareMismatch">) => Promise<AddDeviceResult>;
  rememberDiscoveredChannels: (devdBaseUrl: string, devices: DevdDevice[]) => void;
  prepareWebSerialFlashPort: (deviceId: string) => Promise<SerialPortLike | null>;
  attachMockUsbSerialDevice: () => AddDeviceResult;
  disconnectUsbSerialDevice: (deviceId: string) => Promise<void>;
  sendWifiConfig: (deviceId: string, input: WifiConfigInput, onProgress?: (progress: WifiProvisioningProgress) => void) => Promise<CommandResult>;
  clearWifiConfig: (deviceId: string, onProgress?: (progress: WifiProvisioningProgress) => void) => Promise<CommandResult>;
  setSerialLogLevel: (deviceId: string, level: DeviceSettings["log_level"]) => Promise<CommandResult>;
  setManualChargePrefs: (deviceId: string, prefs: ManualChargePrefsInput) => Promise<CommandResult>;
  refreshChargeControlDetail: (deviceId: string) => Promise<CommandResult>;
  previewManualCharge: (deviceId: string, prefs: ManualChargePrefsInput) => Promise<CommandResult>;
  controlManualCharge: (deviceId: string, input: ManualChargeControlInput) => Promise<CommandResult>;
  setAdvancedPower: (deviceId: string, advancedPower: AdvancedPowerInput) => Promise<CommandResult>;
  resetAdvancedPower: (deviceId: string) => Promise<CommandResult>;
  removeDevice: (deviceId: string) => void;
  refreshDevice: (deviceId: string) => Promise<void>;
  setDemoSeed: (seed: DemoSeed) => void;
  resetDemo: () => void;
};

export const DeviceRegistryContext = createContext<DeviceRegistryContextValue | null>(null);

export function useDeviceRegistry(): DeviceRegistryContextValue {
  const context = useContext(DeviceRegistryContext);
  if (!context) throw new Error("useDeviceRegistry must be used inside DeviceRegistryProvider");
  return context;
}
