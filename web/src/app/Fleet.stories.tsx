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

function renderApp(
  initialPath: string,
  seed: string | null = "default",
  options: {
    initialDevdTarget?: string;
    forceHostedHttpServiceApp?: boolean;
    storedTargets?: unknown[];
  } = {},
) {
  window.localStorage.removeItem(STORAGE_KEY);
  if (options.storedTargets) {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify(options.storedTargets),
    );
  }
  const params = new URLSearchParams(window.location.search);
  if (seed) {
    params.set("seed", seed);
  } else {
    params.delete("seed");
  }
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}?${params.toString()}${window.location.hash}`,
  );
  return (
    <DeviceRegistryProvider>
      <App
        initialPath={initialPath}
        initialDevdTarget={options.initialDevdTarget}
        forceHostedHttpServiceApp={options.forceHostedHttpServiceApp}
      />
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
    await expect((await canvas.findAllByRole("button", { name: "Open" })).length).toBeGreaterThan(0);
  },
};

export const LiveDevdDiscovery: Story = {
  name: "Live devd records",
  render: () =>
    renderApp("/", null, {
      forceHostedHttpServiceApp: true,
      initialDevdTarget: "mock:devd-multi",
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      await canvas.findByRole("heading", { name: "UPS Fleet" }),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByText("mains-aegis-a1b2c3"),
    ).toBeInTheDocument();
    await expect(await canvas.findByText("devd record")).toBeInTheDocument();
    await expect(
      await canvas.findByRole("button", { name: "Open" }),
    ).toBeInTheDocument();
  },
};

export const SavedAndLiveMerged: Story = {
  name: "Saved and live merged",
  render: () =>
    renderApp("/", null, {
      forceHostedHttpServiceApp: true,
      initialDevdTarget: "mock:devd-multi",
      storedTargets: [
        {
          deviceId: "mains-aegis-a1b2c3",
          baseUrl: "mock:lab-standby",
          alias: "Lab rack A",
          location: "Bench 1",
          addedAt: "2026-06-07T00:00:00.000Z",
          transport: "http",
          preferredTransport: "http",
        },
      ],
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      await canvas.findByRole("heading", { name: "Lab rack A" }),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByRole("button", { name: "Open" }),
    ).toBeInTheDocument();
  },
};
