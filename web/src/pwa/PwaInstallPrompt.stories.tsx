import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, userEvent, within } from "storybook/test";
import { useEffect, type ReactNode } from "react";
import { PwaInstallPrompt } from "./PwaInstallPrompt";

const meta = {
  title: "UPS Management/PWA Install",
  component: PwaInstallPrompt,
  tags: ["autodocs"],
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Non-blocking PWA installation recommendation for browsers with native install support and iPhone/iPad manual installation guidance.",
      },
    },
  },
  args: {
    availability: "native",
    visible: true,
    onInstall: fn(),
    onDismiss: fn(),
    onIosGuideOpenChange: fn(),
    placement: "inline",
  },
  decorators: [
    (Story, context) => {
      const className = context.name.includes("Mobile")
        ? "storybook-pwa-install-surface is-mobile-story"
        : "storybook-pwa-install-surface";
      return (
        <div className={className}>
          <Story />
        </div>
      );
    },
  ],
} satisfies Meta<typeof PwaInstallPrompt>;

export default meta;
type Story = StoryObj<typeof meta>;

function IosGuideBackdrop({ children }: { children: ReactNode }) {
  useEffect(() => {
    document.body.classList.add("storybook-pwa-ios-backdrop");
    return () => document.body.classList.remove("storybook-pwa-ios-backdrop");
  }, []);
  return children;
}

export const Native: Story = {
  name: "Native install available",
  play: async ({ args, canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(await canvas.findByText("Install Mains Aegis Web")).toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Install" }));
    await expect(args.onInstall).toHaveBeenCalled();
  },
};

export const IosGuide: Story = {
  name: "iPhone and iPad guide",
  decorators: [
    (Story) => (
      <IosGuideBackdrop>
        <Story />
      </IosGuideBackdrop>
    ),
  ],
  args: {
    availability: "ios-guide",
    iosGuideOpen: true,
    visible: false,
  },
  play: async ({ args }) => {
    const dialog = within(document.body).getByRole("dialog", {
      name: "Install Mains Aegis Web",
    });
    await expect(dialog).toBeVisible();
    await expect(
      within(dialog).getByText(
        "Open the browser Share menu, choose Add to Home Screen, then select Add.",
      ),
    ).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Done" }));
    await expect(args.onIosGuideOpenChange).toHaveBeenCalledWith(false);
  },
};

export const Mobile: Story = {
  name: "Mobile install prompt",
  parameters: {
    viewport: {
      defaultViewport: "mobile",
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const prompt = await canvas.findByRole("complementary", {
      name: "Mains Aegis Web installation",
    });
    await expect(prompt).toBeVisible();
    await expect(prompt.getBoundingClientRect().width).toBeGreaterThan(0);
  },
};

export const Dismissed: Story = {
  name: "Dismiss recommendation",
  play: async ({ args, canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      canvas.getByRole("button", { name: "Dismiss install recommendation" }),
    );
    await expect(args.onDismiss).toHaveBeenCalled();
  },
};

export const Hidden: Story = {
  name: "Hidden",
  args: {
    visible: false,
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.queryByRole("complementary", { name: "Mains Aegis Web installation" }),
    ).not.toBeInTheDocument();
  },
};

export const StateGallery: Story = {
  name: "State gallery",
  render: (args) => (
    <main className="storybook-pwa-gallery">
      <section className="storybook-pwa-gallery-item">
        <PwaInstallPrompt {...args} availability="native" visible />
      </section>
      <section className="storybook-pwa-gallery-item">
        <PwaInstallPrompt {...args} availability="ios-guide" visible />
      </section>
      <section className="storybook-pwa-gallery-item">
        <PwaInstallPrompt {...args} availability="unavailable" visible={false} />
      </section>
    </main>
  ),
};
