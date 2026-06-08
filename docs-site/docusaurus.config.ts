import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: 'CortexFS',
  tagline: 'Provider-neutral AI API 文件系统 ABI',
  favicon: 'img/favicon.ico',

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
  },

  // Set the production url of your site here
  url: 'https://lightjunction.github.io',
  // Set the /<baseUrl>/ pathname under which your site is served
  // For GitHub pages deployment, it is often '/<projectName>/'
  baseUrl: '/cortexfs/',

  // GitHub pages deployment config.
  // If you aren't using GitHub pages, you don't need these.
  organizationName: 'LIghtJUNction',
  projectName: 'cortexfs',

  onBrokenLinks: 'throw',

  // Even if you don't use internationalization, you can use this field to set
  // useful metadata like html lang. For example, if your site is Chinese, you
  // may want to replace "en" with "zh-Hans".
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
    colorMode: {
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'CortexFS',
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docs',
          position: 'left',
          label: '文档',
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
          title: '文档',
          items: [
            {
              label: '概览',
              to: '/docs/intro',
            },
            {
              label: '快速开始',
              to: '/docs/getting-started/quick-start',
            },
            {
              label: '核心概念',
              to: '/docs/concepts/filesystem-abi',
            },
          ],
        },
        {
          title: '指南',
          items: [
            {
              label: 'API 表面',
              to: '/docs/api/file-api',
            },
            {
              label: 'Provider 与路由',
              to: '/docs/providers/provider-instances',
            },
            {
              label: 'Bun Template',
              to: '/docs/bun-template',
            },
          ],
        },
        {
          title: '更多',
          items: [
            {
              label: '运维',
              to: '/docs/operations/audit-export',
            },
            {
              label: '设计规范',
              to: '/docs/design',
            },
            {
              label: 'GitHub',
              href: 'https://github.com/LIghtJUNction/cortexfs',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} CortexFS。由 Docusaurus 构建。`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
