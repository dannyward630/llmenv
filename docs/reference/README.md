# Reference

Per-tool reference notes on the configuration surfaces llmenv targets, each
analyzed against what llmenv currently materializes. These are the basis for
future design docs and err on the side of too much detail.

- [claude-code/](./claude-code/) — Claude Code config surfaces (settings, hooks,
  permissions, skills, subagents, MCP, memory, statusline, plugins, env vars) and
  a consolidated gap analysis vs llmenv.
- [codex/](./codex/) — OpenAI Codex CLI config surfaces (`config.toml`, model
  providers, sandbox/approvals, MCP, AGENTS.md, hooks, profiles, skills, auth,
  env vars, enterprise) and a consolidated gap analysis framed as what a
  greenfield `CodexAdapter` would need to build.
- [opencode-v2-parity.md](./opencode-v2-parity.md) — gaps between the Claude Code
  engine and the opencode engine under opencode v2.0.31: the plugin API rewrite,
  config schema changes, hook-run tool-name mismatches, and the work to reach
  parity.
