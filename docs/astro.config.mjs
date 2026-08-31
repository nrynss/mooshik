// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
  site: 'https://nrynss.github.io',
  base: '/mooshik',
  integrations: [
    starlight({
      title: 'Mooshik',
      description: 'Ambient, local-first AI cowork partner and workspace orchestrator.',
      social: {
        github: 'https://github.com/nrynss/mooshik',
      },
      editLink: {
        baseUrl: 'https://github.com/nrynss/mooshik/edit/main/docs',
      },
      sidebar: [
        {
          label: 'Start here',
          items: [
            { label: 'Product Overview', slug: 'overview' },
            { label: 'Quickstart', slug: 'quickstart' },
          ],
        },
        {
          label: 'Install and configure',
          items: [
            { label: 'Installation', slug: 'installation' },
            { label: 'Configuration', slug: 'configuration' },
          ],
        },
        {
          label: 'Architecture',
          items: [
            { label: 'System Overview', slug: 'system-overview' },
            { label: 'Memory & Lambo Substrate', slug: 'memory-and-lambo' },
            { label: 'WriteLane Concurrency', slug: 'writelane-concurrency' },
            { label: 'Security & Secret Vault', slug: 'security-and-vault' },
          ],
        },
        {
          label: 'User Interface',
          items: [
            { label: 'Terminal UI (TUI)', slug: 'tui-overview' },
            { label: 'Workspace Watcher', slug: 'workspace-watcher' },
            { label: 'Reflection & Synthesis', slug: 'reflection' },
          ],
        },
        {
          label: 'MCP & Tool Hub',
          items: [
            { label: 'MCP Client Host', slug: 'mcp-host' },
            { label: 'News & Search Server', slug: 'news-server' },
            { label: 'Artifacts Server', slug: 'artifacts-server' },
            { label: 'Scratch Script Runner', slug: 'scratch-runner' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'CLI Reference', slug: 'cli' },
            { label: 'Configuration Schema', slug: 'config-schema' },
            { label: 'Error Codes', slug: 'error-codes' },
          ],
        },
        {
          label: 'Contributor Guide',
          items: [
            { label: 'Development & Testing', slug: 'development' },
            { label: 'Release Pipeline', slug: 'release-pipeline' },
            { label: 'Adversarial Reviews', slug: 'adversarial-reviews' },
          ],
        },
      ],
    }),
  ],
});
