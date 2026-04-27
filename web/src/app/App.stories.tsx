import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { DeviceRegistryProvider } from "../device-registry/DeviceRegistry";
import { App } from "./App";

const STORAGE_KEY = "mains-aegis-web.devices.v1";

const meta = {
  title: "UPS Management/App",
  component: App,
  parameters: {
    layout: "fullscreen",
  },
} satisfies Meta<typeof App>;

export default meta;
type Story = StoryObj<typeof meta>;

function renderApp(initialPath: string) {
  window.localStorage.removeItem(STORAGE_KEY);
  return (
    <DeviceRegistryProvider>
      <App initialPath={initialPath} />
    </DeviceRegistryProvider>
  );
}

export const FleetOverview: Story = {
  name: "Fleet overview",
  render: () => renderApp("/"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByRole("heading", { name: "UPS Fleet" })).toBeInTheDocument();
    await expect(await canvas.findByText("Protection active")).toBeInTheDocument();
    await expect(canvas.queryByText("OUT A")).not.toBeInTheDocument();
    await expect(canvas.queryByText("Charger")).not.toBeInTheDocument();
    await userEvent.click(await canvas.findByText("Critical"));
    await expect(await canvas.findByRole("heading", { name: "Storage bay" })).toBeInTheDocument();
  },
};

export const FleetMobile: Story = {
  name: "Fleet mobile",
  parameters: {
    viewport: {
      defaultViewport: "mobile",
    },
  },
  render: () => renderApp("/"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByRole("heading", { name: "UPS Fleet" })).toBeInTheDocument();
    await expect((await canvas.findAllByRole("button", { name: "Details" })).length).toBeGreaterThan(0);
  },
};

export const ConnectDevices: Story = {
  name: "Connect devices",
  render: () => renderApp("/connect"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByRole("heading", { name: "Connect devices" })).toBeInTheDocument();
    await userEvent.clear(canvas.getByLabelText("Target"));
    await userEvent.type(canvas.getByLabelText("Target"), "mock:backup");
    await userEvent.click(canvas.getByRole("button", { name: "Add device" }));
    await expect(await canvas.findByText("Connected Router backup")).toBeInTheDocument();
  },
};

export const CriticalDeviceDashboard: Story = {
  name: "Critical device dashboard",
  render: () => renderApp("/devices/mains-aegis-e4f5a6"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByRole("heading", { name: "Storage bay" })).toBeInTheDocument();
    await expect(await canvas.findByText("FAULT")).toBeInTheDocument();
    await userEvent.click(canvas.getByRole("link", { name: "Battery" }));
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
