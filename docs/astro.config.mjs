// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

// https://astro.build/config
export default defineConfig({
  site: "https://nrynss.github.io",
  base: "/mooshik",
  integrations: [
    starlight({
      title: "Mooshik",
      description: "Ambient, local-first AI cowork partner and workspace orchestrator.",
      social: {
        github: "https://github.com/nrynss/mooshik",
      },
      customCss: ["./src/styles/custom.css"],
      editLink: {
        baseUrl: "https://github.com/nrynss/mooshik/edit/main/docs",
      },
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "What is Mooshik", slug: "overview" },
            { label: "Why Lambo", slug: "why-lambo" },
            { label: "Quickstart", slug: "quickstart" },
          ],
        },
        {
          label: "First run",
          items: [
            { label: "Installation", slug: "installation" },
            { label: "Guided Setup", slug: "guided-setup" },
            { label: "Choosing a Posture", slug: "postures" },
            { label: "Configuration", slug: "configuration" },
          ],
        },
        {
          label: "Using Mooshik",
          items: [
            { label: "The Pane", slug: "tui-overview" },
            { label: "Ambient Workspace Awareness", slug: "workspace-watcher" },
            { label: "Chat and Recall", slug: "chat-and-recall" },
            { label: "Reflection", slug: "reflection" },
            { label: "Research and the Web", slug: "research" },
          ],
        },
        {
          label: "Memory",
          items: [
            { label: "How Lambo Memory Works", slug: "memory-and-lambo" },
            { label: "Earned Canonization", slug: "canonization" },
            { label: "WriteLane Concurrency", slug: "writelane-concurrency" },
            { label: "Memory Consolidation", slug: "reflection-consolidation" },
          ],
        },
        {
          label: "Tools and MCP",
          items: [
            { label: "MCP Host", slug: "mcp-host" },
            { label: "News Server", slug: "news-server" },
            { label: "Artifacts Server", slug: "artifacts-server" },
            { label: "Coder Server", slug: "coder-server" },
            { label: "Scratch Runner", slug: "scratch-runner" },
          ],
        },
        {
          label: "Security",
          items: [
            { label: "The Vault", slug: "security-and-vault" },
            { label: "Permissions and Grants", slug: "permissions" },
            { label: "Secret Scanning", slug: "secret-scanning" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "CLI Reference", slug: "cli" },
            { label: "Configuration Schema", slug: "config-schema" },
            { label: "Error Codes", slug: "error-codes" },
          ],
        },
        {
          label: "Contributing",
          items: [
            { label: "Development & Testing", slug: "development" },
            { label: "Release Pipeline", slug: "release-pipeline" },
            { label: "Adversarial Reviews", slug: "adversarial-reviews" },
          ],
        },
      ],
    }),
  ],
});
