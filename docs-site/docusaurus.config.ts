import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const baseUrl = process.env.DOCUSAURUS_BASE_URL ?? '/cortexfs/';

const config: Config = {
  title: 'CortexFS',
  tagline: 'A small Linux filesystem ABI for agent runtimes',
  url: 'https://lightjunction.github.io',
  baseUrl,
  favicon: 'img/cortexfs-logo.svg',
  organizationName: 'LIghtJUNction',
  projectName: 'cortexfs',
  onBrokenLinks: 'throw',
  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
  },

  i18n: {
    defaultLocale: 'zh-Hans',
    locales: ['zh-Hans', 'en'],
    localeConfigs: {
      'zh-Hans': {
        label: '简体中文',
        htmlLang: 'zh-CN',
      },
      en: {
        label: 'English',
        htmlLang: 'en-US',
      },
    },
  },

  presets: [
    [
      'classic',
      {
        docs: {
          path: '../docs',
          routeBasePath: 'docs',
          sidebarPath: './sidebars.ts',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/cortexfs-social-card.png',
    colorMode: {
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'CortexFS',
      logo: {
        alt: 'CortexFS',
        src: 'img/cortexfs-logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docs',
          position: 'left',
          label: '文档',
        },
        {
          to: '/docs/getting-started',
          label: '安装',
          position: 'left',
        },
        {
          to: '/docs/using-cortexfs',
          label: '使用',
          position: 'left',
        },
        {
          to: '/docs/agent-sh',
          label: 'agent.sh',
          position: 'left',
        },
        {
          type: 'localeDropdown',
          position: 'right',
        },
        {
          href: 'https://github.com/LIghtJUNction/cortexfs',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: '核心',
          items: [
            {label: '安装', to: '/docs/getting-started'},
            {label: '使用 CortexFS', to: '/docs/using-cortexfs'},
            {label: '二次开发', to: '/docs/developing-cortexfs'},
            {label: '设计', to: '/docs/DESIGN'},
            {label: 'Root ABI', to: '/docs/spec/root-abi'},
            {label: 'Object ABI', to: '/docs/spec/object-abi'},
          ],
        },
        {
          title: 'Clients',
          items: [
            {label: 'ctx coreutils', to: '/docs/CTX'},
            {label: 'agent.sh', to: '/docs/agent-sh'},
            {label: 'Session ABI', to: '/docs/spec/session-abi'},
          ],
        },
        {
          title: '项目',
          items: [
            {label: 'GitHub', href: 'https://github.com/LIghtJUNction/cortexfs'},
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} CortexFS.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
