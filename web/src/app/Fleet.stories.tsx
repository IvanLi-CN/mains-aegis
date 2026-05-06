import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { DeviceRegistryProvider } from "../device-registry/DeviceRegistry";
import { App } from "./App";

const STORAGE_KEY = "mains-aegis-web.devices.v1";

const meta = {
  title: "UPS Management/Fleet",
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

export const Overview: Story = {
  name: "Overview",
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

export const Mobile: Story = {
  name: "Mobile",
  parameters: {
    viewport: {
      defaultViewport: "mobile",
    },
  },
  render: () => renderApp("/"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByRole("heading", { name: "UPS Fleet" })).toBeInTheDocument();
    await expect((await canvas.findAllByRole("link", { name: "Details" })).length).toBeGreaterThan(0);
  },
};
