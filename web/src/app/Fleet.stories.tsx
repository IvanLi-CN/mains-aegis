import type { Meta, StoryObj } from "@storybook/react-vite";
import type { ReactNode } from "react";
import { expect, userEvent, within } from "storybook/test";
import { DeviceRegistryProvider } from "../device-registry/DeviceRegistry";
import { App } from "./App";
import { PwaInstallRuntime } from "../pwa/PwaInstallRuntime";
import type { BeforeInstallPromptEventLike } from "../pwa/pwaInstall";
import type { DemoSeed } from "../fixtures/mockDevices";

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
  seed: DemoSeed | null = "default",
  options: {
    initialDevdTarget?: string;
    forceHostedHttpServiceApp?: boolean;
    storedTargets?: unknown[];
    nativeInstall?: boolean;
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
    params.set("demo", "true");
  } else {
    params.delete("demo");
  }
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}?${params.toString()}${window.location.hash}`,
  );
  const app = (
    <App
      initialPath={initialPath}
      initialDevdTarget={options.initialDevdTarget}
      forceHostedHttpServiceApp={options.forceHostedHttpServiceApp}
    />
  );
  return (
    <DeviceRegistryProvider initialDemoSeed={seed ?? undefined}>
      {options.nativeInstall ? (
        <PwaInstallStoryHarness>{app}</PwaInstallStoryHarness>
      ) : (
        app
      )}
    </DeviceRegistryProvider>
  );
}

function PwaInstallStoryHarness({ children }: { children: ReactNode }) {
  const event = {
    preventDefault: () => undefined,
    prompt: async () => undefined,
    userChoice: Promise.resolve({ outcome: "accepted" as const }),
  } as unknown as BeforeInstallPromptEventLike;
  return <PwaInstallRuntime initialNativePrompt={event}>{children}</PwaInstallRuntime>;
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
    await expect(await canvas.findByRole("heading", { name: "Overview" })).toBeInTheDocument();
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

export const DemoControl: Story = {
  name: "Demo control panel",
  render: () => renderApp("/"),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.queryByRole("button", { name: "Install app" }),
    ).not.toBeInTheDocument();
    await expect(
      await canvas.findByRole("button", { name: "Open demo control panel" }),
    ).toBeInTheDocument();
    await userEvent.click(
      canvas.getByRole("button", { name: "Open demo control panel" }),
    );
    await expect(await canvas.findByText("Demo Control")).toBeInTheDocument();
    await expect(
      canvas.getByRole("button", { name: "Move demo control panel" }),
    ).toBeInTheDocument();
    await userEvent.click(canvas.getByRole("combobox", { name: "Scenario" }));
    await userEvent.click(await within(document.body).findByRole("option", { name: "Empty fleet" }));
    await expect(await canvas.findByText("Empty fleet")).toBeInTheDocument();
    await expect(canvas.queryByText("Protection active")).not.toBeInTheDocument();
    await userEvent.click(
      canvas.getByRole("button", { name: "Close demo control panel" }),
    );
    await expect(canvas.queryByText("Demo Control")).not.toBeInTheDocument();
  },
};

export const InstallAvailable: Story = {
  name: "Install available",
  render: () =>
    renderApp("/", null, {
      forceHostedHttpServiceApp: true,
      initialDevdTarget: "mock:devd-multi",
      nativeInstall: true,
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const installButton = await canvas.findByRole("button", { name: "Install app" });
    await expect(installButton).toBeInTheDocument();
    await userEvent.click(installButton);
  },
};

export const InstallAvailableVisual: Story = {
  name: "Install available visual",
  render: () =>
    renderApp("/", null, {
      forceHostedHttpServiceApp: true,
      initialDevdTarget: "mock:devd-multi",
      nativeInstall: true,
    }),
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
