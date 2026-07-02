import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { DeviceRegistryProvider } from "../device-registry/DeviceRegistry";
import { App } from "./App";
import type { DemoSeed } from "../fixtures/mockDevices";

const STORAGE_KEY = "mains-aegis-web.devices.v1";

const meta = {
  title: "UPS Management/Device",
  component: App,
  tags: ["autodocs"],
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Device-level fallback stories for the UPS console. These stories cover the owner-facing hardware, battery, power, and API surfaces when a dedicated ui_demo route is not used.",
      },
    },
  },
} satisfies Meta<typeof App>;

export default meta;
type Story = StoryObj<typeof meta>;

function renderApp(initialPath: string, seed: DemoSeed = "default") {
  window.localStorage.removeItem(STORAGE_KEY);
  const params = new URLSearchParams(window.location.search);
  params.set("demo", "true");
  window.history.replaceState(null, "", `${window.location.pathname}?${params.toString()}${window.location.hash}`);
  return (
    <DeviceRegistryProvider initialDemoSeed={seed}>
      <App initialPath={initialPath} />
    </DeviceRegistryProvider>
  );
}

export const CriticalDashboard: Story = {
  name: "Critical dashboard",
  render: () => renderApp("/devices/mains-aegis-e4f5a6"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByRole("heading", { name: "Storage bay" })).toBeInTheDocument();
    await expect(await canvas.findByText("FAULT")).toBeInTheDocument();
    await userEvent.click(canvas.getByRole("link", { name: "Battery" }));
    await expect(await canvas.findByText("BMS readiness")).toBeInTheDocument();
  },
};

export const BatteryDetail: Story = {
  name: "Battery detail",
  render: () => renderApp("/devices/mains-aegis-a1b2c3/battery"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("Live")).toBeInTheDocument();
    await expect(await canvas.findByText("Cell voltages")).toBeInTheDocument();
    await expect(await canvas.findByText("Delta 13 mV")).toBeInTheDocument();
    await expect(await canvas.findByText("BAL MULTI")).toBeInTheDocument();
    await expect(await canvas.findByText("Start 3 mV")).toBeInTheDocument();
    await expect(await canvas.findByText("C1")).toBeInTheDocument();
    await expect(await canvas.findByText("3.81 V")).toBeInTheDocument();
    await expect(await canvas.findAllByText("BAL")).toHaveLength(2);
    await expect(await canvas.findByText("BMS MOS")).toBeInTheDocument();
    await expect(await canvas.findByText("CHG MOS")).toBeInTheDocument();
    await expect(await canvas.findByText("DSG MOS")).toBeInTheDocument();
    await expect(await canvas.findByText("PCHG MOS")).toBeInTheDocument();
  },
};

export const StreamStateClarity: Story = {
  name: "Stream state clarity",
  render: () => renderApp("/devices/mains-aegis-f7a8b9/battery", "offline"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("Offline")).toBeInTheDocument();
    await expect(await canvas.findByText("device is offline")).toBeInTheDocument();
    await expect(await canvas.findByText("BMS readiness")).toBeInTheDocument();
  },
};

export const ApiDebug: Story = {
  name: "API debug",
  render: () => renderApp("/devices/mains-aegis-e4f5a6/api"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("/api/v1/status")).toBeInTheDocument();
    await expect(await canvas.findByText(/battery_protection/)).toBeInTheDocument();
  },
};

export const PowerDetail: Story = {
  name: "Power detail",
  render: () => renderApp("/devices/mains-aegis-a1b2c3/power", "power-headroom"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("Input")).toBeInTheDocument();
    await expect(await canvas.findByText("Pressure")).toBeInTheDocument();
    await expect(await canvas.findByText("Policy target")).toBeInTheDocument();
    await expect(await canvas.findByText("Limit reason")).toBeInTheDocument();
    await expect(await canvas.findByText("headroom")).toBeInTheDocument();
    await expect(await canvas.findByText("500 mA")).toBeInTheDocument();
    await expect(await canvas.findByText("42 mA / 100 mA")).toBeInTheDocument();
  },
};

export const PowerWatch: Story = {
  name: "Power watch",
  render: () => renderApp("/devices/mains-aegis-a1b2c3/power", "power-watch"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("watch")).toBeInTheDocument();
    await expect(await canvas.findByText("vin_drop_watch")).toBeInTheDocument();
    await expect(await canvas.findByText("300 mA")).toBeInTheDocument();
  },
};

export const PowerLimited: Story = {
  name: "Power limited",
  render: () => renderApp("/devices/mains-aegis-a1b2c3/power", "power-limited"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("limited")).toBeInTheDocument();
    await expect(await canvas.findByText("TPS output current")).toBeInTheDocument();
    await expect(
      await canvas.findByText("Stopped: TPS output current 128 mA > 100 mA"),
    ).toBeInTheDocument();
    await expect(await canvas.findByText("100 mA")).toBeInTheDocument();
  },
};

export const PowerCooldown: Story = {
  name: "Power cooldown",
  render: () => renderApp("/devices/mains-aegis-a1b2c3/power", "power-cooldown"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("cooldown")).toBeInTheDocument();
    await expect(await canvas.findByText("Cooldown retry wait")).toBeInTheDocument();
    await expect(
      await canvas.findByText("Stopped: TPS output current 116 mA > 100 mA"),
    ).toBeInTheDocument();
    await expect(await canvas.findByText("0 mA")).toBeInTheDocument();
  },
};

export const SettingsAdvancedPower: Story = {
  name: "Settings advanced power",
  render: () => renderApp("/devices/mains-aegis-a1b2c3/settings"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      await canvas.findByRole("heading", { name: "Advanced Power" }),
    ).toBeInTheDocument();
    await expect(await canvas.findByText("19V profile")).toBeInTheDocument();
    await expect(
      await canvas.findByText(
        /output_profile=19v · rated_vout_mv=19000/i,
      ),
    ).toBeInTheDocument();
    await expect(await canvas.findByDisplayValue("1200")).toBeInTheDocument();
    await expect(await canvas.findByDisplayValue("600")).toBeInTheDocument();
    await expect(
      await canvas.findByRole("button", { name: "Apply advanced power" }),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByRole("button", { name: "Reset to device default" }),
    ).toBeInTheDocument();
    const applyPrefsButton = await canvas.findByRole("button", {
      name: "Apply prefs",
    });
    expect(applyPrefsButton.getBoundingClientRect().height).toBeLessThanOrEqual(
      80,
    );
    const manualPrefsForm = applyPrefsButton.closest("form");
    expect(manualPrefsForm).not.toBeNull();
    const manualPreferenceControls = Array.from(
      manualPrefsForm?.querySelectorAll(".ui-segmented-control.is-compact") ??
        [],
    );
    expect(manualPreferenceControls).toHaveLength(3);
    for (const control of manualPreferenceControls) {
      expect(control.getBoundingClientRect().height).toBeLessThanOrEqual(38);
    }
    const manualPreferenceRows = Array.from(
      manualPrefsForm?.querySelectorAll(".control-row") ?? [],
    );
    expect(manualPreferenceRows).toHaveLength(3);
    for (const row of manualPreferenceRows) {
      expect(row.getBoundingClientRect().height).toBeLessThanOrEqual(120);
    }
  },
};

export const DeviceHardwareCapabilities: Story = {
  name: "Device hardware capabilities",
  render: () => renderApp("/devices/mains-aegis-a1b2c3/device"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      await canvas.findByRole("heading", { name: "Hardware capabilities" }),
    ).toBeInTheDocument();
    await expect(await canvas.findByText("output_profile")).toBeInTheDocument();
    await expect(await canvas.findByText("19v")).toBeInTheDocument();
    await expect(await canvas.findByText("rated_vout_mv")).toBeInTheDocument();
    await expect(await canvas.findByText("19000 mV")).toBeInTheDocument();
    await expect(
      await canvas.findByText("Hardware identity"),
    ).toBeInTheDocument();
  },
};
