import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

/**
 * Creating a sidebar enables you to:
 - create an ordered group of docs
 - render a sidebar for each doc of that group
 - provide next/previous navigation

 The sidebars can be generated from the filesystem, or explicitly defined here.

 Create as many sidebars as you want.
 */
const sidebars: SidebarsConfig = {
  docs: [
    'intro',
    {
      type: 'category',
      label: '快速开始',
      items: [
        'getting-started/install-deploy',
        'getting-started/quick-start',
        'getting-started/mounting',
        'getting-started/first-request',
      ],
    },
    {
      type: 'category',
      label: '核心概念',
      items: [
        'concepts/filesystem-abi',
        'concepts/formats-providers-models',
        'concepts/spaces-and-security',
      ],
    },
    {
      type: 'category',
      label: 'API 表面',
      items: [
        'api/file-api',
        'api/local-api',
        'api/structured-jobs',
        'api/uds-control-plane',
        'api/threads-and-batch',
      ],
    },
    {
      type: 'category',
      label: 'Provider 与路由',
      items: [
        'providers/new-api-replacement',
        'providers/provider-instances',
        'providers/routing-fallback',
        'providers/secrets',
      ],
    },
    {
      type: 'category',
      label: '集成',
      items: [
        'bun-template',
        'integrations/external-orchestrators',
        'integrations/agents-tools-mcp',
      ],
    },
    {
      type: 'category',
      label: '运维',
      items: [
        'operations/audit-export',
        'operations/live-tests',
        'operations/development-constraints',
      ],
    },
    {
      type: 'category',
      label: '参考',
      items: ['reference/top-level-tree', 'reference/file-types', 'design'],
    },
  ],
};

export default sidebars;
