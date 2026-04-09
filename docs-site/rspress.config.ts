import { defineConfig } from "rspress/config";

function normalizeBase(base: string | undefined): string {
  const raw = (base ?? "/").trim();
  if (!raw || raw === "/") return "/";
  const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

const repoUrl = "https://github.com/IvanLi-CN/mains-aegis";
const docsBase = normalizeBase(process.env.DOCS_BASE);
const docsLogo = `${docsBase}brand/mark.svg`;
const docsFavicon = new URL("./docs/public/favicon.svg", import.meta.url);

const handbookSidebar = [
  {
    text: "项目手册",
    items: [{ text: "总览", link: "/handbook/index" }],
  },
  {
    text: "系统设计",
    items: [
      { text: "导读", link: "/design/index" },
      { text: "系统概览", link: "/design/system-overview" },
      { text: "电源与 BMS", link: "/design/power-and-bms" },
      { text: "前面板与固件", link: "/design/front-panel-and-firmware" },
      { text: "前面板屏幕页面总览", link: "/design/front-panel-screen-pages" },
      { text: "前面板 UI 交互与设计", link: "/design/front-panel-ui-design" },
    ],
  },
  {
    text: "样机复刻与 Bring-up",
    items: [
      { text: "导读", link: "/manual/index" },
      { text: "准备与范围", link: "/manual/prepare-and-scope" },
      { text: "PCB 与连线检查", link: "/manual/pcb-and-wiring-checks" },
      { text: "固件烧录与首次自检", link: "/manual/firmware-flash-and-self-test" },
      { text: "基础使用与排障", link: "/manual/basic-use-and-troubleshooting" },
    ],
  },
];

export default defineConfig({
  root: "docs",
  base: docsBase,
  title: "Mains Aegis 文档",
  description: "Mains Aegis 项目手册：系统设计、样机复刻与 bring-up。",
  lang: "zh-CN",
  logo: docsLogo,
  logoText: "Mains Aegis",
  icon: docsFavicon,
  outDir: "doc_build",
  themeConfig: {
    search: true,
    nav: [
      { text: "首页", link: "/" },
      { text: "项目手册", link: "/handbook/index" },
      { text: "GitHub", link: repoUrl },
    ],
    sidebar: {
      "/": handbookSidebar,
      "/handbook/": handbookSidebar,
      "/design/": handbookSidebar,
      "/manual/": handbookSidebar,
    },
  },
});
