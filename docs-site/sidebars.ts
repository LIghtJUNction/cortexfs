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
      label: 'Getting Started',
      items: [
        'getting-started/quick-start',
        'getting-started/mounting',
        'getting-started/first-request',
      ],
    },
    {
      type: 'category',
      label: 'Concepts',
      items: [
        'concepts/filesystem-abi',
        'concepts/formats-providers-models',
        'concepts/spaces-and-security',
      ],
    },
    {
      type: 'category',
      label: 'API Surface',
      items: ['api/file-api', 'api/local-api', 'api/threads-and-batch'],
    },
    {
      type: 'category',
      label: 'Providers and Routing',
      items: [
        'providers/provider-instances',
        'providers/routing-fallback',
        'providers/secrets',
      ],
    },
    {
      type: 'category',
      label: 'Integrations',
      items: [
        'bun-template',
        'integrations/external-orchestrators',
        'integrations/agents-tools-mcp',
      ],
    },
    {
      type: 'category',
      label: 'Operations',
      items: [
        'operations/audit-export',
        'operations/live-tests',
        'operations/development-constraints',
      ],
    },
    {
      type: 'category',
      label: 'Reference',
      items: ['reference/top-level-tree', 'reference/file-types', 'design'],
    },
  ],
};

export default sidebars;
