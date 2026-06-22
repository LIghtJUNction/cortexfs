import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'CortexFS',
  tagline: 'A small Linux filesystem ABI for agent runtimes',
  url: 'https://lightjunction.github.io',
  baseUrl: '/cortexfs/',
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
    locales: ['zh-Hans'],
    localeConfigs: {
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
          label: 'Spec',
        },
        {
          to: '/docs/agent-sh',
          label: 'agent.sh',
          position: 'left',
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
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
