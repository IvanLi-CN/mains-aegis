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

export default defineConfig({
  root: "docs",
  base: docsBase,
  title: "Mains Aegis 文档",
  description: "Mains Aegis 的设计手册与复刻/使用手册。",
  lang: "zh-CN",
  logo: docsLogo,
  logoText: "Mains Aegis",
  icon: docsFavicon,
  outDir: "doc_build",
  themeConfig: {
    search: true,
    nav: [
      { text: "首页", link: "/" },
      { text: "设计手册", link: "/design/" },
      { text: "复刻与使用", link: "/manual/" },
      { text: "GitHub", link: repoUrl },
    ],
    sidebar: {
      "/": [
        {
          text: "开始阅读",
          items: [
            { text: "文档首页", link: "/" },
            { text: "设计手册", link: "/design/" },
            { text: "复刻与使用", link: "/manual/" },
          ],
        },
      ],
      "/design/": [
        {
          text: "设计手册",
          items: [
            { text: "手册总览", link: "/design/" },
            { text: "系统概览", link: "/design/system-overview" },
            { text: "电源与 BMS", link: "/design/power-and-bms" },
            { text: "前面板与固件", link: "/design/front-panel-and-firmware" },
          ],
        },
      ],
      "/manual/": [
        {
          text: "复刻与使用",
          items: [
            { text: "手册总览", link: "/manual/" },
            { text: "准备与范围", link: "/manual/prepare-and-scope" },
            { text: "PCB 与连线检查", link: "/manual/pcb-and-wiring-checks" },
            { text: "固件烧录与首次自检", link: "/manual/firmware-flash-and-self-test" },
            { text: "基础使用与排障", link: "/manual/basic-use-and-troubleshooting" },
          ],
        },
      ],
    },
  },
});
