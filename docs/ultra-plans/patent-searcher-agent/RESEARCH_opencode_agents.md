# OpenCode Agent Configuration — Research Notes

**Sources fetched:** 2026-04-07
**Purpose:** Exact schema for defining custom agents in OpenCode, for patent-searcher-agent design.

---

## Sources

- https://opencode.ai/docs/agents/ — agents documentation (fetched directly)
- https://opencode.ai/docs/config/ — config documentation (fetched directly)
- https://opencode.ai/docs/mcp-servers/ — MCP servers documentation (fetched directly; note: /docs/mcp/ 404s, correct URL is /docs/mcp-servers/)
- https://raw.githubusercontent.com/sst/opencode/refs/heads/dev/packages/opencode/src/config/config.ts — TypeScript Zod schema source
- https://deepwiki.com/sst/opencode/3.1-agent-configuration — DeepWiki config loading docs
- https://docs.bswen.com/blog/2026-03-30-opencode-custom-agents/ — BSWEN guide
- https://www.mintlify.com/opencode-ai/opencode/reference/config-schema — Mintlify config schema reference
- https://medium.com/@rosgluk/oh-my-opencode-specialised-agents-deep-dive-and-model-guide-d064d8f2a3fa — Oh My OpenCode deep dive

---

## 1. Agent Types

There are **two agent types** (controlled via the `mode` field):

| Mode | Description |
|------|-------------|
| `primary` | Main assistant users interact with directly. Cycle with Tab key. |
| `subagent` | Specialized assistant invoked automatically by primary agents or via `@mention`. |
| `all` | (default when `mode` omitted) — agent is available as both primary and subagent. |

### Built-in Primary Agents

- **build** — Default primary agent. All tools enabled. Full file ops and bash.
- **plan** — Read-only. All file edits and bash set to `ask` by default.

### Built-in Subagents

- **general** — Full tool access (except todo). For multi-step tasks and parallel work.
- **explore** — Read-only codebase explorer. Fast. Cannot modify files.
- **compaction** — Hidden system agent. Auto-compacts long context. Not selectable.
- **title** — Hidden system agent. Generates session titles automatically.
- **summary** — Hidden system agent. Creates session summaries automatically.

---

## 2. Exact JSON Schema for an Agent Entry

The canonical schema key in `opencode.json` is **`agent`** (singular), containing a map of agent names to their config objects.

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "agent": {
    "<agent-name>": {
      // --- Identity & Docs ---
      "description": "string",           // REQUIRED for custom agents. Shown in UI and used by primary agents to decide when to invoke this subagent.
      "mode": "primary" | "subagent" | "all",  // Default: "all"
      "hidden": false,                   // boolean. If true, hides from @ autocomplete. Only applies to mode: subagent.
      "disable": false,                  // boolean. Set true to disable the agent entirely.
      "color": "#FF5733" | "primary" | "secondary" | "accent" | "success" | "warning" | "error" | "info",

      // --- Model ---
      "model": "provider/model-id",      // e.g. "anthropic/claude-sonnet-4-20250514". Optional. Falls back to global model.
      "variant": "string",               // Optional. Default model variant (applies only when using the agent's configured model).

      // --- Sampling ---
      "temperature": 0.0,                // number 0.0–1.0. Default: model-specific (usually 0; 0.55 for Qwen).
      "top_p": 0.9,                      // number 0.0–1.0. Alternative to temperature.

      // --- System Prompt ---
      "prompt": "string or {file:path}", // Custom system prompt. Can be inline string or file reference.

      // --- Iteration Limit ---
      "steps": 5,                        // int > 0. Max agentic iterations before forcing text-only response.
      // "maxSteps": 5,                  // DEPRECATED. Use "steps" instead.

      // --- Tool Access (DEPRECATED method) ---
      "tools": {                         // DEPRECATED. Use "permission" instead.
        "write": false,
        "edit": false,
        "bash": false,
        "mymcp_*": false                 // Glob patterns supported
      },

      // --- Permissions (preferred method) ---
      "permission": {
        "edit": "allow" | "ask" | "deny",
        "bash": "allow" | "ask" | "deny" | {
          "*": "ask",                    // wildcard — last matching rule wins
          "git status *": "allow",
          "git push": "ask"
        },
        "webfetch": "allow" | "ask" | "deny",
        "task": {                        // Controls which subagents this agent can invoke via Task tool
          "*": "deny",
          "orchestrator-*": "allow",
          "code-reviewer": "ask"
        }
      },

      // --- Pass-through provider options ---
      // Any unknown keys are passed through to the provider as model options.
      // Example for OpenAI reasoning models:
      "reasoningEffort": "high",         // provider-specific
      "textVerbosity": "low"             // provider-specific
    }
  }
}
```

### Authoritative TypeScript Zod Schema (from `packages/opencode/src/config/config.ts`)

```typescript
export const Agent = z
  .object({
    model: ModelId.optional(),
    variant: z
      .string()
      .optional()
      .describe("Default model variant for this agent (applies only when using the agent's configured model)."),
    temperature: z.number().optional(),
    top_p: z.number().optional(),
    prompt: z.string().optional(),
    tools: z.record(z.string(), z.boolean()).optional().describe("@deprecated Use 'permission' field instead"),
    disable: z.boolean().optional(),
    description: z.string().optional().describe("Description of when to use the agent"),
    mode: z.enum(["subagent", "primary", "all"]).optional(),
    hidden: z
      .boolean()
      .optional()
      .describe("Hide this subagent from the @ autocomplete menu (default: false, only applies to mode: subagent)"),
    options: z.record(z.string(), z.any()).optional(),
    color: z
      .union([
        z.string().regex(/^#[0-9a-fA-F]{6}$/, "Invalid hex color format"),
        z.enum(["primary", "secondary", "accent", "success", "warning", "error", "info"]),
      ])
      .optional()
      .describe("Hex color code (e.g., #FF5733) or theme color (e.g., primary)"),
    steps: z
      .number()
      .int()
      .positive()
      .optional()
      .describe("Maximum number of agentic iterations before forcing text-only response"),
    maxSteps: z.number().int().positive().optional().describe("@deprecated Use 'steps' field instead."),
    permission: Permission.optional(),
  })
  .catchall(z.any())
  // .transform(...) — unknown keys extracted into options{}; legacy tools→permission migration; maxSteps→steps migration
```

The `.catchall(z.any())` means **any additional unknown keys are accepted and passed through to the provider as model parameters** (stored in `options`).

Known keys (not passed through):
`name`, `model`, `variant`, `prompt`, `description`, `temperature`, `top_p`, `mode`, `hidden`, `color`, `steps`, `maxSteps`, `options`, `permission`, `disable`, `tools`

---

## 3. How to Configure a Custom Agent with a Specific System Prompt

### Method A: Inline JSON in `opencode.json`

```json
{
  "$schema": "https://opencode.ai/config.json",
  "agent": {
    "patent-searcher": {
      "description": "Searches and analyzes patents using USPTO and EPO MCP tools",
      "mode": "subagent",
      "model": "anthropic/claude-sonnet-4-20250514",
      "prompt": "You are a patent search specialist. Your role is to...",
      "permission": {
        "edit": "deny",
        "bash": "deny",
        "webfetch": "allow"
      }
    }
  }
}
```

### Method B: File reference for long prompts

```json
{
  "$schema": "https://opencode.ai/config.json",
  "agent": {
    "patent-searcher": {
      "description": "Searches and analyzes patents",
      "mode": "subagent",
      "prompt": "{file:./prompts/patent-searcher.txt}"
    }
  }
}
```

Path is relative to the config file location. Works for both global and project configs.

### Method C: Markdown file in `.opencode/agents/`

File: `.opencode/agents/patent-searcher.md`

The filename (without `.md`) becomes the agent name.

```markdown
---
description: Searches and analyzes patents using USPTO and EPO MCP tools
mode: subagent
model: anthropic/claude-sonnet-4-20250514
temperature: 0.1
permission:
  edit: deny
  bash: deny
  webfetch: allow
---

You are a patent search specialist. Your role is to search for patents
using the available patent database tools and provide detailed analysis.

Focus on:
- Prior art searches
- Claim analysis
- Classification (CPC/IPC codes)
- Citation analysis
```

Global location: `~/.config/opencode/agents/patent-searcher.md`
Project location: `.opencode/agents/patent-searcher.md`

---

## 4. How to Attach MCP Tools to an Agent

MCP servers are configured globally in `opencode.json` under the `mcp` key. By default, MCP tools are available to all agents. To restrict an MCP server to a specific agent:

### Pattern: Globally disable, then enable per-agent

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "patent-fetch": {
      "type": "local",
      "command": ["node", "/path/to/mcp-fetch-patents/dist/index.js"],
      "enabled": true
    }
  },
  "tools": {
    "patent-fetch_*": false
  },
  "agent": {
    "patent-searcher": {
      "description": "Searches patents using the patent fetch MCP",
      "mode": "subagent",
      "tools": {
        "patent-fetch_*": true
      }
    }
  }
}
```

**Note:** Tool glob pattern format for MCP servers is `<servername>_*` (server name + underscore + wildcard).

### MCP Server Configuration Reference

#### Local MCP (stdio)

```json
{
  "mcp": {
    "my-server": {
      "type": "local",
      "command": ["npx", "-y", "my-mcp-package"],
      "enabled": true,
      "environment": {
        "API_KEY": "{env:MY_API_KEY}"
      },
      "timeout": 5000
    }
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | Y | Must be `"local"` |
| `command` | string[] | Y | Command and arguments |
| `environment` | object | N | Environment variables |
| `enabled` | boolean | N | Enable/disable on startup |
| `timeout` | number | N | Timeout in ms (default: 5000) |

#### Remote MCP (HTTP/SSE)

```json
{
  "mcp": {
    "my-remote-server": {
      "type": "remote",
      "url": "https://my-mcp-server.com/mcp",
      "enabled": true,
      "headers": {
        "Authorization": "Bearer {env:MY_API_KEY}"
      },
      "timeout": 5000
    }
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | Y | Must be `"remote"` |
| `url` | string | Y | Server URL |
| `enabled` | boolean | N | Enable/disable on startup |
| `headers` | object | N | HTTP request headers |
| `oauth` | object/false | N | OAuth configuration |
| `timeout` | number | N | Timeout in ms (default: 5000) |

---

## 5. Complete Example Agent Configurations (Raw)

### From official docs: Full JSON example showing multiple agents

```json
{
  "$schema": "https://opencode.ai/config.json",
  "agent": {
    "build": {
      "mode": "primary",
      "model": "anthropic/claude-sonnet-4-20250514",
      "prompt": "{file:./prompts/build.txt}",
      "tools": {
        "write": true,
        "edit": true,
        "bash": true
      }
    },
    "plan": {
      "mode": "primary",
      "model": "anthropic/claude-haiku-4-20250514",
      "tools": {
        "write": false,
        "edit": false,
        "bash": false
      }
    },
    "code-reviewer": {
      "description": "Reviews code for best practices and potential issues",
      "mode": "subagent",
      "model": "anthropic/claude-sonnet-4-20250514",
      "prompt": "You are a code reviewer. Focus on security, performance, and maintainability.",
      "tools": {
        "write": false,
        "edit": false
      }
    }
  }
}
```

### From official docs: Permission-based example

```json
{
  "$schema": "https://opencode.ai/config.json",
  "permission": {
    "edit": "deny"
  },
  "agent": {
    "build": {
      "permission": {
        "edit": "ask"
      }
    }
  }
}
```

### From official docs: Bash permission glob patterns

```json
{
  "$schema": "https://opencode.ai/config.json",
  "agent": {
    "build": {
      "permission": {
        "bash": {
          "*": "ask",
          "git status *": "allow"
        }
      }
    }
  }
}
```

### From official docs: Task permissions (controlling subagent delegation)

```json
{
  "agent": {
    "orchestrator": {
      "mode": "primary",
      "permission": {
        "task": {
          "*": "deny",
          "orchestrator-*": "allow",
          "code-reviewer": "ask"
        }
      }
    }
  }
}
```

### From official docs: Steps limit

```json
{
  "agent": {
    "quick-thinker": {
      "description": "Fast reasoning with limited iterations",
      "prompt": "You are a quick thinker. Solve problems with minimal steps.",
      "steps": 5
    }
  }
}
```

### From official docs: Hidden subagent

```json
{
  "agent": {
    "internal-helper": {
      "mode": "subagent",
      "hidden": true
    }
  }
}
```

### From official docs: Provider pass-through options (OpenAI reasoning)

```json
{
  "agent": {
    "deep-thinker": {
      "description": "Agent that uses high reasoning effort for complex problems",
      "model": "openai/gpt-5",
      "reasoningEffort": "high",
      "textVerbosity": "low"
    }
  }
}
```

### From official docs: MCP per-agent tool scoping

```json
{
  "mcp": {
    "my-mcp": {
      "type": "local",
      "command": ["bun", "x", "my-mcp-command"],
      "enabled": true
    }
  },
  "tools": {
    "my-mcp*": false
  },
  "agent": {
    "my-agent": {
      "tools": {
        "my-mcp*": true
      }
    }
  }
}
```

### From official docs: Markdown agent — documentation writer

File: `~/.config/opencode/agents/docs-writer.md`

```markdown
---
description: Writes and maintains project documentation
mode: subagent
tools:
  bash: false
---

You are a technical writer. Create clear, comprehensive documentation.

Focus on:
- Clear explanations
- Proper structure
- Code examples
- User-friendly language
```

### From official docs: Markdown agent — security auditor

File: `~/.config/opencode/agents/security-auditor.md`

```markdown
---
description: Performs security audits and identifies vulnerabilities
mode: subagent
tools:
  write: false
  edit: false
---

You are a security expert. Focus on identifying potential security issues.

Look for:
- Input validation vulnerabilities
- Authentication and authorization flaws
- Data exposure risks
- Dependency vulnerabilities
- Configuration security issues
```

### From official docs: Markdown agent — code review with permission rules

File: `~/.config/opencode/agents/review.md`

```markdown
---
description: Reviews code for quality and best practices
mode: subagent
model: anthropic/claude-sonnet-4-20250514
temperature: 0.1
tools:
  write: false
  edit: false
  bash: false
---

You are in code review mode. Focus on:
- Code quality and best practices
- Potential bugs and edge cases
- Performance implications
- Security considerations

Provide constructive feedback without making direct changes.
```

### From official docs: Markdown agent with permission key (not tools)

File: `~/.config/opencode/agents/review.md`

```markdown
---
description: Code review without edits
mode: subagent
permission:
  edit: deny
  bash:
    "*": ask
    "git diff": allow
    "git log*": allow
    "grep *": allow
  webfetch: deny
---

Only analyze code and suggest changes.
```

---

## 6. Configuration File Locations and Precedence

Configuration files are merged (not replaced). Later sources override earlier ones for conflicting keys.

| Priority | Source | Path |
|----------|--------|------|
| 1 (lowest) | Remote org defaults | `.well-known/opencode` endpoint |
| 2 | Global user config | `~/.config/opencode/opencode.json` |
| 3 | Custom config path | `$OPENCODE_CONFIG` env var |
| 4 | Project config | `./opencode.json` |
| 5 | `.opencode` directories | `.opencode/agents/`, `.opencode/commands/` etc |
| 6 | Inline runtime config | `$OPENCODE_CONFIG_CONTENT` env var |
| 7 | Managed/admin config | `/etc/opencode/` (Linux) |
| 8 (highest) | macOS MDM | `.mobileconfig` via MDM |

Agent markdown files can be placed in:
- **Global:** `~/.config/opencode/agents/` (or `~/.config/opencode/agent/` for backwards compat)
- **Per-project:** `.opencode/agents/` (or `.opencode/agent/`)

**Arrays** (`instructions`, `plugin`) are **concatenated** across config layers, not replaced.

---

## 7. Variable Substitution in Config

```json
{
  "agent": {
    "my-agent": {
      "prompt": "{file:./prompts/my-agent-system-prompt.txt}",
      "model": "{env:OPENCODE_MODEL}"
    }
  }
}
```

- `{env:VAR_NAME}` — substitutes environment variable (empty string if not set)
- `{file:path/to/file}` — substitutes file contents (relative to config file, or absolute)

---

## 8. Default Agent Setting

```json
{
  "$schema": "https://opencode.ai/config.json",
  "default_agent": "plan"
}
```

- Must be a **primary** agent (not a subagent)
- Falls back to `"build"` with a warning if agent doesn't exist or is a subagent
- Applies to: TUI, CLI (`opencode run`), desktop app, GitHub Action

---

## 9. Creating Agents via CLI

```bash
opencode agent create           # project-scoped agent
opencode agent create --global  # global agent
```

Interactive wizard that:
1. Asks where to save (global or project)
2. Asks for description
3. Generates system prompt and identifier
4. Lets you select tool access
5. Creates a markdown file

---

## 10. Key Schema Notes

- Config key is **`agent`** (singular), not `agents` (plural). The `agents` key seen in some third-party docs appears to be inaccurate.
- The `agent` value is a **map/record** (object), not an array. Keys are agent identifiers.
- **`description` is required** for custom agents. It's used by the UI and by primary agents when deciding which subagent to invoke.
- The `tools` field is **deprecated** in favor of `permission`. New configs should use `permission`.
- Unknown fields in an agent config are **silently passed through** to the provider as model options (via the `.catchall(z.any())` in the Zod schema).
- `mode` defaults to `"all"` if not specified.
- Subagent model inheritance: if no model is specified on a subagent, it uses the model of the primary agent that invoked it.
- MCP tool glob pattern: `<servername>_*` (underscore separator, not hyphen).

---

## 11. MCP + Agent Integration Pattern for Patent Searcher

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "fetch-patents": {
      "type": "local",
      "command": ["node", "./dist/index.js"],
      "enabled": true,
      "environment": {
        "USPTO_API_KEY": "{env:USPTO_API_KEY}"
      }
    }
  },
  "tools": {
    "fetch-patents_*": false
  },
  "agent": {
    "patent-searcher": {
      "description": "Searches patents using USPTO and EPO databases via MCP tools",
      "mode": "subagent",
      "model": "anthropic/claude-sonnet-4-20250514",
      "temperature": 0.1,
      "prompt": "{file:.opencode/prompts/patent-searcher.md}",
      "permission": {
        "edit": "deny",
        "bash": "deny",
        "webfetch": "allow"
      },
      "tools": {
        "fetch-patents_*": true
      }
    }
  }
}
```

Note: The `tools` field is deprecated but is still the mechanism for MCP glob scoping in many docs. The `permission` field does not yet support MCP-level glob patterns directly — use `tools` for MCP scoping until `permission` fully replaces it.
