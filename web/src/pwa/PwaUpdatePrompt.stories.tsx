import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, userEvent, within } from "storybook/test";
import {
  PwaUpdatePrompt,
  type PwaUpdateSnapshot,
} from "./PwaUpdatePrompt";

function snapshot(status: PwaUpdateSnapshot["status"], error: string | null = null): PwaUpdateSnapshot {
  return { status, error };
}

const meta = {
  title: "UPS Management/PWA Update",
  component: PwaUpdatePrompt,
  tags: ["autodocs"],
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Non-blocking PWA lifecycle feedback for downloaded app-shell updates and first-load offline readiness.",
      },
    },
  },
  args: {
    snapshot: snapshot("ready"),
    onUpdate: fn(),
    onDismiss: fn(),
    placement: "inline",
  },
  decorators: [
    (Story, context) => {
      const className = context.name.includes("Mobile")
        ? "storybook-pwa-surface is-mobile-story"
        : "storybook-pwa-surface";
      return (
        <div className={className}>
          <Story />
        </div>
      );
    },
  ],
} satisfies Meta<typeof PwaUpdatePrompt>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Ready: Story = {
  name: "Ready to update",
  play: async ({ args, canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("New version available")).toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Update" }));
    const dialog = within(document.body).getByRole("dialog", {
      name: "Update Mains Aegis Web",
    });
    await expect(dialog).toBeVisible();
    await userEvent.click(within(dialog).getByRole("button", { name: "Update" }));
    await expect(args.onUpdate).toHaveBeenCalled();
  },
};

export const Activating: Story = {
  name: "Activating update",
  args: {
    snapshot: snapshot("activating"),
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("Updating Mains Aegis Web")).toBeInTheDocument();
    await expect(canvas.getByRole("button", { name: "Updating" })).toBeDisabled();
  },
};

export const OfflineReady: Story = {
  name: "Offline ready",
  args: {
    snapshot: snapshot("offlineReady"),
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("Ready for offline use")).toBeInTheDocument();
    await expect(canvas.queryByRole("button", { name: "Update" })).not.toBeInTheDocument();
  },
};

export const ErrorState: Story = {
  name: "Registration error",
  args: {
    snapshot: snapshot("error", "The browser rejected service worker registration."),
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("Update check failed")).toBeInTheDocument();
    await expect(await canvas.findByText("The browser rejected service worker registration.")).toBeInTheDocument();
  },
};

export const Mobile: Story = {
  name: "Mobile ready prompt",
  parameters: {
    viewport: {
      defaultViewport: "mobile",
    },
  },
  render: (args) => (
    <div className="storybook-pwa-mobile-frame">
      <PwaUpdatePrompt {...args} placement="inline" />
    </div>
  ),
};

export const StateGallery: Story = {
  name: "State gallery",
  render: (args) => (
    <main className="storybook-pwa-gallery">
      {[
        snapshot("ready"),
        snapshot("activating"),
        snapshot("offlineReady"),
        snapshot("error", "Service worker registration failed."),
      ].map((item) => (
        <section key={item.status} className="storybook-pwa-gallery-item">
          <PwaUpdatePrompt {...args} snapshot={item} placement="inline" />
        </section>
      ))}
    </main>
  ),
};
