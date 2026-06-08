import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { DeviceRegistryProvider } from "../device-registry/DeviceRegistry";
import { App } from "./App";

const STORAGE_KEY = "mains-aegis-web.devices.v1";

const meta = {
  title: "UPS Management/Device",
  component: App,
  parameters: {
    layout: "fullscreen",
  },
} satisfies Meta<typeof App>;

export default meta;
type Story = StoryObj<typeof meta>;

function renderApp(initialPath: string, seed = "default") {
  window.localStorage.removeItem(STORAGE_KEY);
  const params = new URLSearchParams(window.location.search);
  params.set("seed", seed);
  window.history.replaceState(null, "", `${window.location.pathname}?${params.toString()}${window.location.hash}`);
  return (
    <DeviceRegistryProvider>
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
    await expect(await canvas.findByText("pressure_vindpm")).toBeInTheDocument();
    await expect(await canvas.findByText("100 mA")).toBeInTheDocument();
  },
};

export const PowerCooldown: Story = {
  name: "Power cooldown",
  render: () => renderApp("/devices/mains-aegis-a1b2c3/power", "power-cooldown"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("cooldown")).toBeInTheDocument();
    await expect(await canvas.findByText("cooldown_retry_wait")).toBeInTheDocument();
    await expect(await canvas.findByText("0 mA")).toBeInTheDocument();
  },
};
