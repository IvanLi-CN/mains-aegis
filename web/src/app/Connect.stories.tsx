import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { DeviceRegistryProvider } from "../device-registry/DeviceRegistry";
import { App, ButtonLabel, ConnectionCallout } from "./App";
import { Usb } from "lucide-react";
import type { DeviceTarget } from "../api/types";

const STORAGE_KEY = "mains-aegis-web.devices.v1";

const meta = {
  title: "UPS Management/Add device",
  component: App,
  tags: ["autodocs"],
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Add-device states for standalone Web usage and the hosted mains-aegis-devd app. Use this surface to add hardware, bind a new USB path, or add a LAN endpoint from current devd device records.",
      },
    },
  },
} satisfies Meta<typeof App>;

export default meta;
type Story = StoryObj<typeof meta>;

function renderApp(
  seed: string | null = "default",
  options: {
    initialDevdTarget?: string;
    forceHostedHttpServiceApp?: boolean;
    storedTargets?: DeviceTarget[];
    runtimeMode?: "public_static" | "hosted" | "unknown";
    extraQuery?: Record<string, string>;
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
  for (const [key, value] of Object.entries(options.extraQuery ?? {})) {
    params.set(key, value);
  }
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}?${params.toString()}${window.location.hash}`,
  );
  setMeta(
    "mains-aegis-app-runtime-mode",
    options.runtimeMode ?? "unknown",
  );
  return (
    <DeviceRegistryProvider>
      <App
        initialPath="/connect"
        initialDevdTarget={options.initialDevdTarget}
        forceHostedHttpServiceApp={options.forceHostedHttpServiceApp}
      />
    </DeviceRegistryProvider>
  );
}

function setMeta(name: string, content: string) {
  let node = document.head.querySelector<HTMLMetaElement>(`meta[name="${name}"]`);
  if (!node) {
    node = document.createElement("meta");
    node.name = name;
    document.head.appendChild(node);
  }
  node.content = content;
}

export const HostedDevdDiscovery: Story = {
  name: "Hosted devd records",
  render: () => renderApp("empty", { forceHostedHttpServiceApp: true }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      await canvas.findByRole("heading", { name: "Add device" }),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByRole("heading", {
        name: /mains-aegis-devd device records/,
      }),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByText("mains-aegis-devd-service"),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByText("mock:lab-standby"),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByRole("button", { name: "Add WiFi" }),
    ).toBeInTheDocument();
    await expect(await canvas.findByText("Mock")).toBeInTheDocument();
    await expect(
      canvas.queryByText("Discovery source"),
    ).not.toBeInTheDocument();
    await expect(canvas.queryByLabelText("devd URL")).not.toBeInTheDocument();
    await expect(canvas.queryByLabelText("Target")).not.toBeInTheDocument();
    await expect(
      canvas.queryByRole("heading", { name: "Web Serial" }),
    ).not.toBeInTheDocument();
    await expect(
      canvas.queryByRole("heading", { name: "LAN device API" }),
    ).not.toBeInTheDocument();
  },
};

export const PagesDirectLanSupported: Story = {
  name: "Pages direct LAN supported",
  render: () =>
    renderApp("empty", {
      runtimeMode: "public_static",
      extraQuery: {
        stored_target_preset: "lan-companion-confirmed",
        mock_browser_capability: "supported",
      },
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      await canvas.findByRole("heading", { name: "LAN device API" }),
    ).toBeInTheDocument();
    await expect(await canvas.findByRole("heading", { name: "CIDR scan" })).toBeInTheDocument();
    await expect(await canvas.findByLabelText("Target")).toBeInTheDocument();
    await userEvent.clear(canvas.getByLabelText("Target"));
    await userEvent.type(
      canvas.getByLabelText("Target"),
      "mains-aegis-c7d8e9.local",
    );
    await userEvent.click(canvas.getByRole("button", { name: "Add LAN" }));
    await expect(await canvas.findByText(/^Connected /)).toBeInTheDocument();
  },
};

export const PagesDirectLanUnsupported: Story = {
  name: "Pages direct LAN unsupported browser",
  render: () =>
    renderApp("empty", {
      runtimeMode: "public_static",
      extraQuery: {
        mock_browser_capability: "unsupported",
      },
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText(/Use Chrome or local devd UI/)).toBeInTheDocument();
    await expect(canvas.getByRole("button", { name: "Add LAN" })).toBeDisabled();
    await expect(canvas.getByRole("button", { name: "Scan LAN" })).toBeDisabled();
  },
};

export const PagesCidrScanCandidates: Story = {
  name: "Pages CIDR scan candidates",
  render: () =>
    renderApp("empty", {
      runtimeMode: "public_static",
      extraQuery: {
        mock_browser_capability: "supported",
      },
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByLabelText("IPv4 CIDR"), "192.168.31.40/29");
    await userEvent.click(canvas.getByRole("button", { name: "Scan LAN" }));
    await expect(await canvas.findByText(/Found 2 devices in 192.168.31.40\/29/)).toBeInTheDocument();
    await expect(await canvas.findByText("mains-aegis-a1b2c3.local")).toBeInTheDocument();
    await expect(await canvas.findByText("mains-aegis-c7d8e9.local")).toBeInTheDocument();
  },
};

export const MergedMultiChannelDevice: Story = {
  name: "Merged multi-channel device",
  render: () =>
    renderApp(null, {
      forceHostedHttpServiceApp: true,
      initialDevdTarget: "mock:devd-multi",
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      await canvas.findByText("mains-aegis-a1b2c3"),
    ).toBeInTheDocument();
    await expect(
      canvas.getByText("USB connected / WiFi connected"),
    ).toBeInTheDocument();
    await expect(
      canvas.getByRole("button", { name: "Bind USB" }),
    ).toBeInTheDocument();
    await expect(
      canvas.getByRole("button", { name: "Add WiFi" }),
    ).toBeInTheDocument();
  },
};

export const RememberedChannelSwitch: Story = {
  name: "Remembered channel switch",
  render: () =>
    renderApp(null, {
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
          rememberedChannels: {
            http: {
              baseUrl: "mock:lab-standby",
              seenAt: "2026-06-07T00:00:00.000Z",
              source: "devd_discovery",
            },
          },
        },
      ],
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      await canvas.findByRole("button", { name: "Open" }),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByRole("button", { name: "Use WiFi" }),
    ).toBeInTheDocument();
    await expect(
      await canvas.findByRole("button", { name: "Use USB" }),
    ).toBeInTheDocument();
  },
};

export const PendingUsbBindTargetSelection: Story = {
  name: "Pending USB bind target selection",
  render: () =>
    renderApp(null, {
      forceHostedHttpServiceApp: true,
      initialDevdTarget: "mock:devd-bind-target",
      storedTargets: [
        {
          deviceId: "mains-aegis-a1b2c3",
          baseUrl: "mock:lab-standby",
          alias: "Lab rack A",
          location: "Bench 1",
          addedAt: "2026-06-07T00:00:00.000Z",
          transport: "http",
          preferredTransport: "http",
          rememberedChannels: {
            http: {
              baseUrl: "mock:lab-standby",
              seenAt: "2026-06-07T00:00:00.000Z",
              source: "devd_discovery",
            },
          },
        },
      ],
    }),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const bindButton = await canvas.findByRole("button", { name: "Bind USB" });
    const bindTarget = await canvas.findByRole("combobox", {
      name: /Bind USB target/i,
    });
    await expect(bindButton).toBeDisabled();
    await userEvent.selectOptions(bindTarget, "mains-aegis-a1b2c3");
    await expect(bindButton).toBeEnabled();
  },
};

export const FirmwareMismatchWarning: Story = {
  name: "Firmware mismatch warning",
  render: () => (
    <main className="storybook-feedback-surface">
      <header className="storybook-feedback-header">
        <span className="eyebrow">USB connection gate</span>
        <h1>Firmware mismatch must stop writable USB setup</h1>
        <p>
          Raw log decode issues may be ignored in the console, but a mismatched
          firmware artifact blocks connect until the user explicitly continues.
        </p>
      </header>
      <section className="connect-grid">
        <section className="connect-panel usb-panel">
          <header className="connect-panel-header">
            <div>
              <h3>
                <Usb size={18} /> Web Serial
              </h3>
              <p>Chromium Web Serial available for USB CDC devices</p>
            </div>
            <span className="transport-badge serial">ready</span>
          </header>
          <div className="connect-form compact">
            <label>
              Alias
              <input name="usb-alias" value="Lab bench USB" readOnly />
            </label>
            <label>
              Location
              <input name="usb-location" value="Bench 1" readOnly />
            </label>
            <div className="form-actions with-callout">
              <button className="primary-button" type="button">
                <ButtonLabel
                  icon={Usb}
                  busy={false}
                  busyText="Connecting"
                  text="Connect Web Serial"
                />
              </button>
              <ConnectionCallout
                id="story-firmware-mismatch"
                message="firmware_artifact_mismatch: Connected firmware build e5f9e4a-dirty does not match any available firmware artifact. defmt decode and safe diagnostics may be wrong."
              />
              <button className="secondary-button danger-action" type="button">
                Ignore warning and connect
              </button>
            </div>
          </div>
        </section>
      </section>
    </main>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByText("Firmware mismatch")).toBeInTheDocument();
    await expect(
      canvas.getByText("firmware_artifact_mismatch"),
    ).toBeInTheDocument();
    await expect(
      canvas.getByRole("button", { name: "Ignore warning and connect" }),
    ).toBeInTheDocument();
  },
};
