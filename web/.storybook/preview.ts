import type { Preview } from "@storybook/react-vite";
import "../src/styles/tokens.css";
import "../src/styles/globals.css";

const preview: Preview = {
  parameters: {
    controls: {
      expanded: true,
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/i,
      },
    },
    backgrounds: {
      default: "canvas",
      values: [
        { name: "canvas", value: "oklch(98.8% 0.006 105)" },
        { name: "panel", value: "oklch(99.2% 0.005 105)" },
      ],
    },
    viewport: {
      options: {
        desktop: {
          name: "Desktop",
          styles: { width: "1280px", height: "900px" },
        },
        mobile: {
          name: "Mobile",
          styles: { width: "390px", height: "844px" },
        },
      },
    },
  },
};

export default preview;
