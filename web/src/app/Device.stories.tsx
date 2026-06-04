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
    await expect(await canvas.findByText("Cell voltages")).toBeInTheDocument();
    await expect(await canvas.findByText("C1")).toBeInTheDocument();
    await expect(await canvas.findByText("3.81 V")).toBeInTheDocument();
    await expect(await canvas.findByText("BMS MOS")).toBeInTheDocument();
    await expect(await canvas.findByText("CHG MOS")).toBeInTheDocument();
    await expect(await canvas.findByText("DSG MOS")).toBeInTheDocument();
    await expect(await canvas.findByText("PCHG MOS")).toBeInTheDocument();
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
