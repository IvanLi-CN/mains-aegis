import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { DeviceRegistryProvider } from "../device-registry/DeviceRegistry";
import { App, ButtonLabel, ConnectionCallout } from "./App";
import { Usb } from "lucide-react";

const STORAGE_KEY = "mains-aegis-web.devices.v1";

const meta = {
  title: "UPS Management/Connect",
  component: App,
  parameters: {
    layout: "fullscreen",
  },
} satisfies Meta<typeof App>;

export default meta;
type Story = StoryObj<typeof meta>;

function renderApp(seed = "default") {
  window.localStorage.removeItem(STORAGE_KEY);
  const params = new URLSearchParams(window.location.search);
  params.set("seed", seed);
  window.history.replaceState(null, "", `${window.location.pathname}?${params.toString()}${window.location.hash}`);
  return (
    <DeviceRegistryProvider>
      <App initialPath="/connect" />
    </DeviceRegistryProvider>
  );
}

export const AddLanTarget: Story = {
  name: "Add LAN target",
  render: () => renderApp(),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByRole("heading", { name: "Connect devices" })).toBeInTheDocument();
    await userEvent.clear(canvas.getByLabelText("Target"));
    await userEvent.type(canvas.getByLabelText("Target"), "mock:backup");
    await userEvent.click(canvas.getByRole("button", { name: "Add LAN" }));
    await expect(await canvas.findByText("Connected Router backup")).toBeInTheDocument();
  },
};

export const FirmwareMismatchWarning: Story = {
  name: "Firmware mismatch warning",
  render: () => (
    <main className="storybook-feedback-surface">
      <header className="storybook-feedback-header">
        <span className="eyebrow">USB connection gate</span>
        <h1>Firmware mismatch must stop writable USB setup</h1>
        <p>Raw log decode issues may be ignored in the console, but a mismatched firmware artifact blocks connect until the user explicitly continues.</p>
      </header>
      <section className="connect-grid">
        <section className="connect-panel usb-panel">
          <header className="connect-panel-header">
            <div>
              <h3><Usb size={18} /> USB CDC</h3>
              <p>Chromium Web Serial available</p>
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
                <ButtonLabel icon={Usb} busy={false} busyText="Connecting" text="Connect USB" />
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
    await expect(canvas.getByText("firmware_artifact_mismatch")).toBeInTheDocument();
    await expect(canvas.getByRole("button", { name: "Ignore warning and connect" })).toBeInTheDocument();
  },
};
