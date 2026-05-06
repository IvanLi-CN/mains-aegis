import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";
import { Server, Trash2, Wifi } from "lucide-react";
import { ButtonLabel, ConnectionCallout, WifiProvisioningCallout, type UiFeedback } from "./App";

type FeedbackScenario = {
  title: string;
  description: string;
  body: React.ReactNode;
};

function WifiFormPreview({ busy, callout }: { busy: "save" | "clear" | null; callout?: React.ReactNode }) {
  return (
    <section className="info-panel settings-panel storybook-feedback-panel">
      <header>
        <Wifi size={18} />
        <h2>WiFi provisioning</h2>
      </header>
      <form className="settings-form">
        <label>
          SSID
          <input name="wifi-ssid" value="LabNet" readOnly />
        </label>
        <label>
          PSK
          <input name="wifi-psk" value="correct-horse" type="password" readOnly />
        </label>
        <div className="secret-note">PSK is written over USB and cleared from the form after submit.</div>
        <div className="form-actions wifi-actions">
          <span className="wifi-save-anchor">
            <button className="primary-button" type="button" disabled={busy !== null}>
              <ButtonLabel busy={busy === "save"} busyText="Saving" text="Save WiFi" />
            </button>
            {callout}
          </span>
          <button className="secondary-button" type="button" disabled={busy !== null}>
            <ButtonLabel icon={Trash2} busy={busy === "clear"} busyText="Clearing" text="Clear" />
          </button>
        </div>
      </form>
    </section>
  );
}

function FeedbackGallery({ scenarios }: { scenarios: FeedbackScenario[] }) {
  return (
    <main className="storybook-feedback-surface">
      <header className="storybook-feedback-header">
        <span className="eyebrow">USB WiFi feedback states</span>
        <h1>Provisioning must report real hardware progress</h1>
        <p>These states cover the visible contract for save, clear, device connection, loading, and failure feedback.</p>
      </header>
      <div className="storybook-feedback-grid">
        {scenarios.map((scenario) => (
          <section className="storybook-feedback-card" key={scenario.title}>
            <div className="storybook-feedback-copy">
              <h2>{scenario.title}</h2>
              <p>{scenario.description}</p>
            </div>
            <div className="storybook-feedback-demo">{scenario.body}</div>
          </section>
        ))}
      </div>
    </main>
  );
}

const success: UiFeedback = {
  tone: "success",
  message: "WiFi connected to LabNet at 192.168.31.42",
};

const saveFailure: UiFeedback = {
  tone: "error",
  message: "wifi_connect_failed: connect_failed",
};

const clearFailure: UiFeedback = {
  tone: "error",
  message: "wifi_disconnect_timeout: timed out waiting for WiFi state disabled",
};

const scenarios: FeedbackScenario[] = [
  {
    title: "Connect failure bubble",
    description: "Hardware connection failures appear as a callout anchored to the command area.",
    body: <ConnectionCallout id="story-connect-failure" message="transport_error: Failed to fetch" />,
  },
  {
    title: "Save failure bubble",
    description: "Save WiFi failures are anchored to the Save WiFi command and preserve the firmware error code.",
    body: (
      <WifiFormPreview
        busy={null}
        callout={<WifiProvisioningCallout id="story-save-failure" feedback={saveFailure} />}
      />
    ),
  },
  {
    title: "Clear failure bubble",
    description: "Clear WiFi failures remain prominent because the EEPROM and runtime state may not match.",
    body: (
      <WifiFormPreview
        busy={null}
        callout={<WifiProvisioningCallout id="story-clear-failure" feedback={clearFailure} />}
      />
    ),
  },
  {
    title: "Save pending",
    description: "Save reports hardware progress while credentials are written, WiFi starts, and DHCP returns an address.",
    body: (
      <div className="storybook-feedback-inline">
        <WifiFormPreview
          busy="save"
          callout={
            <WifiProvisioningCallout
              id="story-save-progress"
              progress={{ phase: "connecting", message: "Connecting to LabNet and waiting for an IP address", network: { state: "connecting", ipv4: null, last_error: null } }}
            />
          }
        />
      </div>
    ),
  },
  {
    title: "Waiting for IP",
    description: "The link can be up before DHCP has produced an address, so that state remains visible.",
    body: (
      <WifiProvisioningCallout
        id="story-ip-progress"
        progress={{ phase: "ip", message: "WiFi link is up. Waiting for an IP address", network: { state: "connected", ipv4: null, last_error: null } }}
      />
    ),
  },
  {
    title: "Clear pending",
    description: "Clear stays loading until EEPROM clear ack and disabled network state are observed.",
    body: (
      <div className="storybook-feedback-inline">
        <WifiFormPreview
          busy="clear"
          callout={
            <WifiProvisioningCallout
              id="story-clear-progress"
              progress={{ phase: "clearing", message: "Disconnecting WiFi and clearing runtime credentials", network: { state: "connected", ipv4: "192.168.31.42", last_error: null } }}
            />
          }
        />
      </div>
    ),
  },
  {
    title: "Success feedback",
    description: "Success remains low noise after the hardware result is confirmed.",
    body: <WifiFormPreview busy={null} callout={<WifiProvisioningCallout id="story-save-success" feedback={success} />} />,
  },
];

const meta = {
  title: "UPS Management/Settings/WiFi Provisioning Feedback",
  component: FeedbackGallery,
  tags: ["autodocs"],
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Grouped feedback states for USB WiFi provisioning. Failures use bubbles; pending operations show spinning command buttons until firmware state is confirmed.",
      },
    },
  },
} satisfies Meta<typeof FeedbackGallery>;

export default meta;

type Story = StoryObj<typeof meta>;

export const StateGallery: Story = {
  args: { scenarios },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByText("Connect failure bubble")).toBeInTheDocument();
    await expect(canvas.getByText("wifi_disconnect_timeout")).toBeInTheDocument();
    await expect(canvas.getByText("wifi_connect_failed")).toBeInTheDocument();
    await expect(canvas.getByRole("button", { name: "Saving" })).toBeDisabled();
    await expect(canvas.getByRole("button", { name: "Clearing" })).toBeDisabled();
  },
};
