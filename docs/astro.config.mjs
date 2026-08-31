// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
  site: 'https://mooshik.github.io',
  integrations: [
    starlight({
      title: 'Mooshik Documentation',
      description: 'Ambient, local-first AI cowork partner and workspace orchestrator',
      social: {
        github: 'https://github.com/nrynss/mooshik',
      },
      sidebar: [
        {
          label: 'Getting Started',
          autogenerate: { directory: 'getting-started' },
        },
        {
          label: 'Architecture',
          autogenerate: { directory: 'architecture' },
        },
        {
          label: 'User Interface',
          autogenerate: { directory: 'user-interface' },
        },
        {
          label: 'MCP & Tool Hub',
          autogenerate: { directory: 'mcp-and-tools' },
        },
        {
          label: 'Reference',
          autogenerate: { directory: 'reference' },
        },
        {
          label: 'Contributor Guide',
          autogenerate: { directory: 'contributor-guide' },
        },
      ],
    }),
  ],
});
