import type { Preview } from "@storybook/react-vite";
import "../src/styles/tokens.css";
import "../src/styles/globals.css";

const preview: Preview = {
  parameters: {
    layout: "fullscreen",
    controls: {
      expanded: true,
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
