import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const baseUrl = process.env.DOCUSAURUS_BASE_URL ?? '/cortexfs/';

const config: Config = {
  title: 'CortexFS',
  tagline: 'Models, agents, tools, and durable sessions under /ctx',
  url: 'https://lightjunction.github.io',
  baseUrl,
  favicon: 'img/cortexfs-favicon.svg',
  organizationName: 'LIghtJUNction',
  projectName: 'cortexfs',
  onBrokenLinks: 'throw',
  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'throw',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en', 'zh-Hans'],
    localeConfigs: {
      en: {
        label: 'English',
        htmlLang: 'en-US',
      },
      'zh-Hans': {
        label: '简体中文',
        htmlLang: 'zh-CN',
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
          editUrl: 'https://github.com/LIghtJUNction/cortexfs/edit/main/docs/',
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
    metadata: [
      {
        name: 'description',
        content:
          'CortexFS mounts models, agents, tools, and durable sessions at /ctx — a small Unix ABI you can ls, cat, execute, secure, and audit.',
      },
    ],
    colorMode: {
      respectPrefersColorScheme: true,
    },
    navbar: {
      logo: {
        alt: '/ctx',
        src: 'img/cortexfs-logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docs',
          position: 'left',
          label: 'Docs',
        },
        {
          to: '/docs/getting-started',
          label: 'Install',
          position: 'left',
        },
        {
          to: '/docs/using-cortexfs',
          label: 'Usage',
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
          title: 'Core',
          items: [
            {label: 'Install', to: '/docs/getting-started'},
            {label: 'Use CortexFS', to: '/docs/using-cortexfs'},
            {label: 'Develop', to: '/docs/developing-cortexfs'},
            {label: 'Design', to: '/docs/DESIGN'},
            {label: 'Root ABI', to: '/docs/spec/root-abi'},
            {label: 'Object ABI', to: '/docs/spec/object-abi'},
          ],
        },
        {
          title: 'Clients',
          items: [
            {label: 'ctx coreutils', to: '/docs/CTX'},
            {label: 'agent.sh', to: '/docs/agent-sh'},
            {label: 'Chat UI ABI', to: '/docs/chatui'},
            {label: 'Session ABI', to: '/docs/spec/session-abi'},
          ],
        },
        {
          title: 'Project',
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
      additionalLanguages: ['bash', 'toml', 'diff', 'ini', 'json'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
