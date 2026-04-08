## OpenCode Research (2026-04-07)

This document summarizes research into the opencode CLI/agent tool. All information was fetched from live sources on 2026-04-07. No training memory was used.

---

### What is OpenCode

OpenCode is an open-source AI coding agent built for the terminal. It is available at https://opencode.ai and on GitHub at https://github.com/anomalyco/opencode.

**Tagline:** "The open source AI coding agent"

**Key facts:**
- Written in Go (TypeScript/Bun backend for storage/sessions), giving it fast startup times
- ~120,000 GitHub stars, 800+ contributors, 5M+ monthly developers
- Current version: v1.3.17 (as of April 2026)
- Available as: terminal TUI, desktop app, IDE extension, web interface, GitHub Actions integration
- Does NOT store your code or context data (privacy-first)

**Interfaces:**
- TUI (Terminal User Interface) — the default, interactive mode
- CLI non-interactive mode (`opencode run "prompt"`) for scripting/automation
- Web interface (`opencode web`)
- IDE integration (VS Code, Cursor, etc.) — launch with `Ctrl+Esc` / `Cmd+Esc`
- GitHub/GitLab Actions — mention `/opencode` or `/oc` in PR comments

**Installation:**
```bash
curl -fsSL https://opencode.ai/install | bash
# or
npm i -g opencode-ai@latest
# or
brew install anomalyco/tap/opencode
```

**Sources:**
- https://opencode.ai/
- https://opencode.ai/docs/
- https://github.com/anomalyco/opencode

---

### Agent System

#### Overview

OpenCode has a two-tier agent architecture:

1. **Primary Agents** — Main assistants for direct user interaction. You cycle between them with Tab. Tool access managed via permissions.
2. **Subagents** — Specialized assistants invoked by primary agents (via `task` tool) or directly by users via `@mention`.

#### Built-in Agents

| Agent | Type | Description |
|-------|------|-------------|
| `build` | primary | Default. All tools enabled. For active development. |
| `plan` | primary | Restricted: `edit`/`bash` set to `ask`. Read-only analysis/planning. |
| `general` | subagent | Full access except todo tool. For multi-step tasks. |
| `explore` | subagent | Fast, read-only. For codebase navigation. |
| `compaction` | system (hidden) | Context management |
| `title` | system (hidden) | Auto-generates session titles |
| `summary` | system (hidden) | Session summarization |

#### Creating Custom Agents

**Interactive wizard:**
```bash
opencode agent create
```
Prompts for: storage location, description, system prompt, tool selection.

**Two configuration methods:**

**Method 1: Markdown file (recommended for readability)**

Place `.md` files in:
- Project: `.opencode/agents/<name>.md`
- Global: `~/.config/opencode/agents/<name>.md`

The filename (without `.md`) becomes the agent name.

```yaml
---
description: Agent purpose (required)
mode: subagent          # primary | subagent | all
model: anthropic/claude-sonnet-4-20250514
temperature: 0.1
steps: 5                # max agentic iterations before text-only
color: "#ff6b6b"        # hex or theme color name
hidden: false           # hide from @ autocomplete
disable: false          # disable agent entirely
permission:
  edit: deny
  bash: false
  webfetch: allow
  task: allow
---
System prompt text here.
```

**Method 2: JSON in opencode.json**

```json
{
  "agent": {
    "agent-name": {
      "description": "What this agent does",
      "mode": "primary|subagent|all",
      "model": "provider/model-id",
      "prompt": "{file:./prompts/file.txt}",
      "temperature": 0.3,
      "steps": 5,
      "permission": {
        "edit": "deny",
        "bash": "ask",
        "webfetch": "allow"
      }
    }
  }
}
```

#### Agent Configuration Options

| Option | Values | Description |
|--------|--------|-------------|
| `description` | string (required) | Brief explanation of agent function |
| `mode` | `primary`, `subagent`, `all` | Where agent appears; default is `all` |
| `model` | `provider/model-id` | Override default model for this agent |
| `prompt` | string or `{file:path}` | Custom system prompt |
| `temperature` | 0.0–1.0 | Randomness (0.0–0.2 = focused; 0.6–1.0 = creative) |
| `top_p` | 0.0–1.0 | Alternative randomness control |
| `steps` | integer | Max agentic iterations before text-only response |
| `permission` | object | Tool access control per tool |
| `color` | hex or theme name | UI appearance |
| `hidden` | bool | Hide from @ autocomplete |
| `disable` | bool | Disable agent entirely |

#### Permissions System

Three levels:
- `"allow"` — Execute without approval
- `"ask"` — Require user confirmation
- `"deny"` — Disable tool (removes from agent's available tools)

Configurable permission targets: `edit`, `bash`, `webfetch`, `task`, `skill`, and any MCP tool.

Bash supports glob patterns:
```json
{
  "bash": {
    "*": "ask",
    "git status *": "allow",
    "grep *": "allow"
  }
}
```

Per-agent overrides in `opencode.json`:
```json
{
  "agent": {
    "plan": {
      "permission": {
        "skill": { "internal-*": "allow" }
      }
    }
  }
}
```

**Sources:**
- https://opencode.ai/docs/agents/
- https://opencode.ai/docs/config/

---

### Agent Types

#### Built-in Primary Agents
- **build** — Full tool access; default agent
- **plan** — Restricted to read-only; asks before bash/edit

#### Built-in Subagents
- **general** — Full access except todo; for complex multi-step tasks
- **explore** — Fast, read-only; for codebase navigation

#### Agent Modes
- `primary` — Appears in Tab cycle; user-facing
- `subagent` — Invocable via `@mention` or `task` tool; may be hidden
- `all` — Both primary and subagent (default if `mode` omitted)

#### Orchestrator Pattern (Custom Agent Type)

An orchestrator is a primary agent that routes work to subagents instead of executing directly.

Required configuration for an orchestrator:
```yaml
---
description: Routes user requests to specialized subagents
mode: primary
model: anthropic/claude-haiku-4.5   # fast model preferred
temperature: 0.1                      # deterministic routing
permission:
  edit: deny
  write: deny
  bash: deny
  webfetch: deny
  task: allow                         # MUST have task tool
---
You are a routing agent. Your role is to analyze user requests and delegate
to the appropriate specialized subagent via the task tool.
Never execute tasks directly. Always delegate.
...
```

Example from community guide — 14 specialized subagents:
`oracle`, `explorer`, `code-review`, `dev`, `writer`, `ux`, `librarian`, `commits`, `fixup`, `tailwind-theme`, `code-pattern-analyst`, `mutation-testing`, `test-drop`, `prompt-safety-review`

Example from community repo (19 agents):
`planner`, `architect`, `security-auditor`, `orchestrator`, `new-feature`, `refactor`, `typescript-reviewer`, `python-reviewer`, `flutter-reviewer`, `java-reviewer`, `code-review`, `tdd-guide`, `pr-review`, `explore`, `doc-writer`, `write-commit`, `build-resolver`, `fix`, `init-project`

**Sources:**
- https://opencode.ai/docs/agents/
- https://gist.github.com/gc-victor/1d3eeb46ddfda5257c08744972e0fc4c
- https://github.com/RogerioSobrinho/codeme-opencode
- https://dev.to/uenyioha/porting-claude-codes-agent-teams-to-opencode-4hol

---

### Tools Available to Agents

All tools are available by default, subject to permissions. Tools are referenced by key for permission configuration.

#### File Operations
| Tool | Permission Key | Description |
|------|---------------|-------------|
| `read` | `read` | Read file contents with optional offset/limit (default 2000 lines) |
| `write` | `edit` | Create/overwrite files |
| `edit` | `edit` | Modify files via exact string replacement |
| `apply_patch` | `edit` | Apply patches to files |
| `glob` | `glob` | Find files by pattern (uses ripgrep, respects .gitignore) |
| `grep` | `grep` | Search file contents with regex (caps at 100 matches) |
| `list` | `list` | List directory contents recursively |
| `lsp` | `lsp` | Code intelligence: definitions, references, hover info (experimental: `OPENCODE_EXPERIMENTAL_LSP_TOOL=true`) |

#### Execution
| Tool | Permission Key | Description |
|------|---------------|-------------|
| `bash` | `bash` | Execute shell commands in persistent shell; 2-min timeout; supports glob-pattern permissions |

#### Web/Network
| Tool | Permission Key | Description |
|------|---------------|-------------|
| `webfetch` | `webfetch` | Fetch URLs, converts HTML to Markdown; 5MB limit; Cloudflare bypass |
| `websearch` | `websearch` | Web search via Exa AI (requires `OPENCODE_ENABLE_EXA=1`) |

#### Agent Coordination
| Tool | Permission Key | Description |
|------|---------------|-------------|
| `task` | `task` | Spawn subagents; creates isolated session with own context window |
| `skill` | `skill` | Load SKILL.md files on-demand for conversation context |

#### Task Management
| Tool | Permission Key | Description |
|------|---------------|-------------|
| `todowrite` | `todowrite` | Manage session-scoped todo lists |
| `todoread` | `todowrite` | Read todo lists |

#### Interaction
| Tool | Permission Key | Description |
|------|---------------|-------------|
| `question` | `question` | Ask user questions during execution (header, text, options list) |

**Sources:**
- https://opencode.ai/docs/tools/
- https://deepwiki.com/sst/opencode/5.3-built-in-tools-reference

---

### Config File Format

#### File Locations (merged in order, later overrides earlier)
1. Remote config (`.well-known/opencode`)
2. Global: `~/.config/opencode/opencode.json`
3. Custom: `OPENCODE_CONFIG` env var
4. Project: `opencode.json` in project root
5. `.opencode` directories
6. Inline: `OPENCODE_CONFIG_CONTENT` env var
7. Managed/system directories (require admin access)
8. macOS MDM managed preferences

#### Project Directory Structure
```
.opencode/
├── opencode.json       # Project config
├── agents/             # Custom agent .md files
├── commands/           # Custom slash command .md files
├── skills/             # Skill packs (SKILL.md files in subdirs)
├── modes/              # (deprecated, now use agents)
├── plugins/            # NPM plugin configs
├── tools/              # Custom tool definitions
└── themes/             # Custom themes
```

Note: Both singular (`agent/`) and plural (`agents/`) directory names are supported.

#### Full opencode.json Schema

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  
  // Core model settings
  "model": "anthropic/claude-sonnet-4-5",
  "small_model": "anthropic/claude-haiku-4",
  "default_agent": "build",
  
  // Provider configuration
  "provider": {
    "myprovider": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Display Name",
      "options": {
        "baseURL": "https://api.endpoint.com/v1",
        "headers": { "X-Custom": "value" }
      },
      "models": {
        "model-id": { "name": "Model Name" }
      }
    }
  },
  "disabled_providers": ["groq"],
  "enabled_providers": ["anthropic", "openai"],
  
  // Agent definitions
  "agent": {
    "code-reviewer": {
      "description": "Reviews code for quality and security",
      "mode": "subagent",
      "model": "anthropic/claude-sonnet-4-5",
      "prompt": "You are a code reviewer...",
      "temperature": 0.2,
      "steps": 10,
      "hidden": false,
      "permission": {
        "edit": "deny",
        "bash": "deny",
        "webfetch": "allow"
      }
    }
  },
  
  // Instructions / AGENTS.md
  "instructions": [
    "CONTRIBUTING.md",
    "docs/guidelines.md",
    ".cursor/rules/*.md",
    "https://example.com/remote-rules.md"
  ],
  
  // Custom commands
  "command": {
    "test": {
      "template": "Run the full test suite and report results",
      "description": "Run tests",
      "agent": "build"
    }
  },
  
  // Global permissions
  "permission": {
    "edit": "ask",
    "bash": "ask",
    "webfetch": "allow"
  },
  
  // Tool toggles
  "tools": {
    "write": true,
    "bash": true,
    "websearch": false
  },
  
  // MCP servers
  "mcp": {
    "my-server": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "my-mcp-package"],
      "env": { "API_KEY": "{env:MY_API_KEY}" }
    }
  },
  
  // Server settings
  "server": {
    "port": 4096,
    "hostname": "0.0.0.0",
    "mdns": true,
    "cors": ["http://localhost:5173"]
  },
  
  // Snapshot/undo tracking (default: true)
  "snapshot": true,
  
  // Auto-update behavior
  "autoupdate": true,
  
  // Context compaction
  "compaction": "auto",  // "auto" | "prune" | "reserved"
  
  // Session sharing
  "share": "manual",  // "manual" | "auto" | "disabled"
  
  // Code formatter
  "formatter": { ... },
  
  // NPM plugins
  "plugin": ["@opencode/helicone"],
  
  // Experimental features
  "experimental": { ... }
}
```

#### Variable Substitution
- Environment variables: `{env:VARIABLE_NAME}`
- File contents: `{file:path/to/file}`

**Sources:**
- https://opencode.ai/docs/config/
- https://opencode.ai/config.json (schema)

---

### Agent Skills

Skills are reusable instruction packs that agents load on-demand via the `skill` tool.

#### File Structure
```
.opencode/skills/
└── git-release/
    └── SKILL.md
```

#### SKILL.md Format
```yaml
---
name: git-release              # required; 1-64 chars; ^[a-z0-9]+(-[a-z0-9]+)*$
description: Create consistent releases and changelogs   # required; 1-1024 chars
license: MIT                   # optional
compatibility: opencode        # optional
metadata:                      # optional
  audience: maintainers
  workflow: github
---

## What I do
- Draft release notes from merged PRs
- Propose a version bump
- Provide a copy-pasteable `gh release create` command

## When to use me
Use this when you are preparing a tagged release.
```

#### Skill Search Paths
- `.opencode/skills/<name>/SKILL.md`
- `.claude/skills/<name>/SKILL.md`
- `.agents/skills/<name>/SKILL.md`
- `~/.config/opencode/skills/<name>/SKILL.md`
- `~/.claude/skills/`
- `~/.agents/skills/`

Opencode walks up from current directory to git worktree root.

#### Skill Permissions
```json
{
  "permission": {
    "skill": {
      "*": "allow",
      "internal-*": "deny",
      "experimental-*": "ask"
    }
  }
}
```

**Sources:**
- https://opencode.ai/docs/skills/

---

### Rules / AGENTS.md

#### What it is
`AGENTS.md` provides persistent project context and instructions to agents — equivalent to Cursor rules or CLAUDE.md. Run `/init` to auto-generate from your codebase.

#### File Locations (precedence order)
1. Local `AGENTS.md` files (walking up from cwd to git root)
2. Global `~/.config/opencode/AGENTS.md`
3. Fallback: `CLAUDE.md` (project), `~/.claude/CLAUDE.md` (global)

Note: If both `AGENTS.md` and `CLAUDE.md` exist, only `AGENTS.md` is used.

#### Custom instructions via opencode.json
```json
{
  "instructions": [
    "CONTRIBUTING.md",
    "docs/guidelines.md",
    ".cursor/rules/*.md",
    "https://remote.example.com/rules.md"
  ]
}
```
All sources are combined with AGENTS.md content. Remote URLs fetched with 5-second timeout.

**Sources:**
- https://opencode.ai/docs/rules/

---

### State/Session Management

#### Session Architecture
- Sessions stored in SQLite via Drizzle ORM at `~/.local/share/opencode/opencode.db` (or `Global.Path.data/opencode.db`)
- Each session has a unique ULID identifier
- Scoped to projects (`ProjectID`)
- Contains ordered message sequences with tool call parts
- Supports parent-child relationships for subagent delegation

#### Session Hierarchy
- **Root sessions**: Top-level conversations (no parent)
- **Child sessions**: Created when primary agent uses `task` tool to spawn subagent; linked via `parentID`
- Recursive deletion: deleting parent removes children first (CASCADE)

#### Session Lifecycle
1. Creation via `Session.createNext()` — generates ULID, sets default title
2. Active processing — agent loop with tool execution; events published to event bus
3. State persistence — `Session.touch()` updates timestamps; `Session.setSummary()` records diff stats
4. Archival — `Session.setArchived()` marks inactive but keeps queryable

#### Fork Operations
`Session.fork()` creates a branch of a session at a specific message point (for trying alternate approaches).

#### Storage Architecture (Dual-Layer)
- **SQLite (Drizzle ORM)**: Entities, relationships, metadata
- **File-based storage**: Large content (tool outputs, diffs) at `Global.Path.data/storage/`
- **VCS Snapshot Layer**: Git-based file change tracking at `Global.Path.data/snapshot/[projectID]/[hash]`
- SQLite uses WAL journal mode and NORMAL synchronous for concurrent access

#### Context Management
When conversation history approaches token limits, OpenCode automatically summarizes previous exchanges (compaction). Modes: `auto`, `prune`, `reserved`.

#### Undo/Redo
- `/undo` — Reverts last change (git snapshot-based)
- `/redo` — Re-applies reverted change
- Git-based snapshots captured at step boundaries enable granular rollback

#### Session CLI Commands
```bash
opencode session list            # List all sessions
opencode export [sessionID]      # Export session as JSON
opencode import <file>           # Import session
opencode run --session <id>      # Continue specific session
opencode run --continue          # Resume last session
opencode run --fork              # Branch when continuing
opencode stats                   # Token usage and costs
```

#### Agent State Restoration
When resuming a session:
1. Load `Session.Info` by ID
2. Retrieve ordered message history via `Session.messages()`
3. Agent loop uses history to inform subsequent LLM calls
4. Permission rulesets reapplied for tool execution

**Sources:**
- https://deepwiki.com/sst/opencode/2.1-session-lifecycle-and-state
- https://deepwiki.com/sst/opencode/2.9-storage-and-database
- https://opencode.ai/docs/cli/

---

### Multi-Agent Orchestration

#### Core Pattern
Primary agents spawn subagents via the `task` tool. Each subagent runs in an isolated context window (its own session), potentially with a different model.

#### Subagent Invocation
- **Automatic**: Primary agent calls `task` tool with subagent name and prompt
- **Manual**: User types `@agent-name` in TUI
- **CLI**: `opencode run "@agent-name do this task"`

#### Isolation
- Each subagent gets its own context window (separate from orchestrator)
- Keeps orchestrator context clean
- Subagents can use different models than the invoking agent

#### Model Assignment for Subagents
- If no model specified: subagent uses the model of the invoking primary agent
- Can be overridden in agent config with `model: provider/model-id`

#### Navigation in TUI
- `Leader+Down` — Enter child session
- `Right` — Next child session
- `Left` — Previous child session
- `Up` — Return to parent session

#### Agent Teams (Advanced, 2026 Feature)
OpenCode supports agent teams — lead AI spawns teammates, each with own context window, coordinating via JSONL message passing.

**Message system:**
- Messages appended to `team_inbox/<projectId>/<teamName>/<agentName>.jsonl` (O(1) writes)
- Injected as synthetic user messages into recipient sessions
- Auto-wake: sending to idle agent restarts its prompt loop

**Topology:** Full peer-to-peer (any agent can message any other); not just hub-and-spoke.

**Cross-provider teams:** Mix models from different providers (e.g., GPT-5.3, Gemini 2.5, Claude Sonnet in same team).

**Sub-agent isolation in teams:** Disposable subagents spawned by teammates are blocked from team channels via permission deny rules.

**Recovery:** On server crash, busy members force-transition to ready; lead notified; no automatic restart (to prevent runaway costs).

#### Parallel vs Sequential Delegation
- **Sequential**: Chain tasks where later steps depend on earlier outputs
- **Parallel**: Multiple `task` calls in single response for independent tasks

#### Community: "Oh My OpenCode" (omo)
Full multi-agent engineering system plugin. Adds:
- Sisyphus orchestration system with parallel background execution
- 11 specialized agents with distinct roles
- LSP + AST-Grep for IDE-quality refactoring
- Built-in MCPs (Exa, Context7, Grep.app)

**Sources:**
- https://opencode.ai/docs/agents/
- https://dev.to/uenyioha/porting-claude-codes-agent-teams-to-opencode-4hol
- https://deepwiki.com/code-yeongyu/oh-my-opencode/4.1-agent-orchestration-overview

---

### Models

#### Model Format
`provider_id/model_id` — e.g., `anthropic/claude-sonnet-4-5`, `openai/gpt-5`

#### Setting Default Model
```json
{ "model": "anthropic/claude-sonnet-4-5" }
```

Via CLI: `opencode --model anthropic/claude-sonnet-4-5`

#### Model Loading Priority
1. `--model` / `-m` CLI flag
2. `model` field in config file
3. Last used model
4. First model by internal priority

#### Variants System
Models support configuration variants:

- **Anthropic**: `high`, `max` (thinking budgets)
- **OpenAI**: `none`, `minimal`, `low`, `medium`, `high`, `xhigh` (reasoning effort)
- **Google**: `low`, `high` (effort/token budget)

Custom variants override built-in options via `variants` config key.

Model-specific options (passed through to provider):
```json
{
  "model": "anthropic/claude-opus-4-5",
  "thinking": { "type": "enabled", "budgetTokens": 10000 }
}
```

```json
{
  "model": "openai/gpt-5",
  "reasoningEffort": "high",
  "reasoningSummary": "detailed"
}
```

#### Supported Providers (as of April 2026)

**Major cloud:**
- Anthropic (Claude Opus 4.5, Sonnet 4.5, Haiku)
- OpenAI (GPT-5.4, GPT-5.1 Codex, o3, o4-mini, GPT-4.1 family)
- Google (Gemini 3.1 Pro, Gemini 2.5 Flash)
- AWS Bedrock (IAM auth, profiles, VPC endpoints)
- Azure OpenAI (Azure AI Foundry)
- Google Vertex AI

**Specialized/subscription:**
- GitHub Copilot (device code auth)
- GitLab Duo (Premium/Ultimate)
- OpenCode Zen (curated models, pay-as-you-go, 30+ models)
- OpenCode Go (low-cost subscription)

**Routing/gateways:**
- OpenRouter (75+ models)
- Vercel AI Gateway
- Cloudflare AI Gateway
- Helicone (observability + proxy)

**Local:**
- Ollama (`baseURL: "http://localhost:11434/v1"`)
- LM Studio (`baseURL: "http://127.0.0.1:1234/v1"`)
- llama.cpp (OpenAI-compatible endpoint)

**Additional providers (75+ total):**
302.AI, Baseten, Cerebras, Cloudflare Workers AI, Cortecs, DeepSeek, Deep Infra, Fireworks AI, Groq, Hugging Face, IO.NET, Moonshot AI, MiniMax, Nebius, Ollama Cloud, OVHcloud, SAP AI Core, Scaleway, Together AI, Venice AI, xAI (Grok), Z.AI, and more.

#### Custom/self-hosted provider setup
```json
{
  "provider": {
    "myprovider": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "My Provider",
      "options": {
        "baseURL": "https://api.myprovider.com/v1",
        "headers": { "Authorization": "Bearer {env:MY_API_KEY}" }
      },
      "models": {
        "my-model-id": { "name": "My Model", "limit": { "context": 128000, "output": 4096 } }
      }
    }
  }
}
```

Or via `LOCAL_ENDPOINT` environment variable for simple self-hosted setup.

**Sources:**
- https://opencode.ai/docs/models/
- https://opencode.ai/docs/providers/
- https://opencode.ai/docs/zen/

---

### Examples

#### Example 1: Security Auditor Subagent (Markdown)

`.opencode/agents/security-auditor.md`:
```yaml
---
description: Performs security audits on code, looking for vulnerabilities
mode: subagent
model: anthropic/claude-haiku-4.5
temperature: 0.1
permission:
  edit: deny
  bash: deny
  webfetch: allow
  task: deny
---
You are a security auditor. Your role is to analyze code for security vulnerabilities,
OWASP top 10 issues, injection risks, authentication flaws, and secrets in code.

Report findings in structured format:
- Severity: CRITICAL/HIGH/MEDIUM/LOW
- Location: file:line
- Issue description
- Recommended fix

Never modify code. Only report findings.
```

#### Example 2: Orchestrator Agent

`.opencode/agents/orchestrator.md`:
```yaml
---
description: Routes user requests to specialized subagents
mode: primary
model: anthropic/claude-haiku-4.5
temperature: 0.1
permission:
  edit: deny
  write: deny
  bash: deny
  webfetch: deny
  task: allow
  read: allow
  grep: allow
  glob: allow
---
You are an orchestration agent. Analyze user requests and delegate to the appropriate
specialized subagent via the task tool. Never execute tasks directly.

Available agents:
- @explorer: codebase search and exploration
- @security-auditor: security review
- @dev: feature implementation
- @code-review: code quality review
- @doc-writer: documentation

Always gather context first before delegating implementation tasks.
Chain: explorer -> dev for "fix this bug" type requests.
```

#### Example 3: opencode.json with Multiple Agents

```json
{
  "$schema": "https://opencode.ai/config.json",
  "model": "anthropic/claude-sonnet-4-5",
  "default_agent": "build",
  "instructions": ["AGENTS.md", "CONTRIBUTING.md"],
  "permission": {
    "bash": "ask",
    "edit": "ask"
  },
  "agent": {
    "patent-searcher": {
      "description": "Searches and analyzes patents using MCP tools",
      "mode": "subagent",
      "model": "anthropic/claude-sonnet-4-5",
      "temperature": 0.2,
      "steps": 20,
      "permission": {
        "bash": "deny",
        "edit": "deny",
        "webfetch": "allow",
        "task": "allow"
      }
    },
    "patent-analyst": {
      "description": "Analyzes patent claims and prior art",
      "mode": "subagent",
      "model": "anthropic/claude-opus-4-5",
      "temperature": 0.1,
      "permission": {
        "bash": "deny",
        "edit": "allow",
        "webfetch": "allow"
      }
    }
  },
  "mcp": {
    "fetch-patents": {
      "type": "stdio",
      "command": "node",
      "args": ["./build/index.js"]
    }
  }
}
```

#### Example 4: Custom Command

`.opencode/commands/patent-search.md`:
```yaml
---
description: Search for patents related to a technology area
agent: patent-searcher
subtask: true
---
Search for patents related to: $TOPIC

Provide:
1. Top 10 most relevant patents with numbers and titles
2. Key inventors and assignees
3. Filing date range
4. Main technical claims summary
5. Prior art landscape overview
```

#### Example 5: Skill Pack

`.opencode/skills/patent-analysis/SKILL.md`:
```yaml
---
name: patent-analysis
description: Analyze patent claims, identify prior art, assess novelty
license: MIT
compatibility: opencode
---

## What I do
- Parse patent claim structures (independent vs dependent claims)
- Identify claim elements and their relationships
- Compare claims against prior art
- Assess novelty and non-obviousness
- Generate freedom-to-operate summaries

## When to use me
Use when analyzing a specific patent document or comparing a set of patents.

## Claim Analysis Format
For each independent claim:
- Claim number and text
- Preamble (technical field)
- Body elements (enumerated)
- Claim type (method/apparatus/composition)
```

**Sources:**
- https://opencode.ai/docs/agents/
- https://gist.github.com/gc-victor/1d3eeb46ddfda5257c08744972e0fc4c
- https://github.com/RogerioSobrinho/codeme-opencode

---

### Internal Architecture

#### Agent Loop
1. System prompts + conversation history + tool definitions + user input assembled
2. `streamText` called via AI SDK
3. Results stream: text deltas, tool calls, tool results
4. Tool results stored as message parts; events published to event bus
5. Loop continues until `stopWhen` (e.g., max steps reached)

The AI SDK provides a provider-agnostic interface, so the same tool definitions work across all models.

#### LSP Integration
- Spawns language-specific LSP servers (pyright, gopls, ruby-lsp, etc.)
- Communicates via JSON-RPC over stdio
- After file edits, queries diagnostics; feeds back to LLM
- File watching notifies LSP of changes
- Requires `OPENCODE_EXPERIMENTAL_LSP_TOOL=true`

#### Real-Time Communication
- HTTP server exposes Server-Sent Events (SSE) for real-time updates
- Both TUI and external clients receive updates via shared event bus

**Sources:**
- https://cefboud.com/posts/coding-agents-internals-opencode-deepdive/

---

### CLI Reference

```bash
# Start TUI
opencode

# Non-interactive / scripting
opencode run "write a function that..."
opencode run --model anthropic/claude-opus-4-5 "complex task"
opencode run --continue "follow up"
opencode run --session <id> "continue this session"
opencode run --fork "try alternate approach"

# Server modes
opencode serve              # Headless API server
opencode web                # Headless + open browser
opencode attach             # Attach to running backend

# Agent management
opencode agent create       # Interactive agent wizard
opencode agent list         # List available agents

# Auth
opencode auth login         # Configure API keys
opencode auth list          # Show authenticated providers
opencode auth logout        # Remove credentials

# Sessions
opencode session list       # List sessions
opencode export [id]        # Export session as JSON
opencode import <file>      # Import session
opencode stats              # Token usage and costs

# Models
opencode models [provider]  # List available models

# MCP
opencode mcp add            # Add MCP server
opencode mcp list           # Show configured servers
opencode mcp auth [name]    # OAuth authentication
opencode mcp debug          # Troubleshoot

# GitHub
opencode github install     # Setup GitHub Actions
opencode github run         # Execute agent in CI/CD

# Maintenance
opencode upgrade [version]  # Update to latest/specific version
opencode acp                # Start Agent Client Protocol server (stdin/stdout nd-JSON)
opencode uninstall          # Remove OpenCode
```

**Key flags:**
- `--model` / `-m`: `provider/model-id`
- `--session` / `-s`: Session ID to continue
- `--continue` / `-c`: Resume last session
- `--fork`: Branch session when continuing
- `--format`: `default` or `json`

**Sources:**
- https://opencode.ai/docs/cli/

---

### Raw URLs Fetched

All URLs fetched on 2026-04-07:

1. https://opencode.ai/docs/ — Main documentation intro
2. https://opencode.ai/docs/agents/ — Agent system documentation
3. https://opencode.ai/docs/config/ — Configuration reference
4. https://opencode.ai/docs/cli/ — CLI commands reference
5. https://opencode.ai/docs/skills/ — Agent skills documentation
6. https://opencode.ai/docs/tools/ — Built-in tools reference
7. https://opencode.ai/docs/models/ — Models configuration
8. https://opencode.ai/docs/providers/ — Provider setup guides
9. https://opencode.ai/docs/rules/ — AGENTS.md rules documentation
10. https://opencode.ai/docs/modes/ — Modes documentation (deprecated in favor of agents)
11. https://opencode.ai/docs/zen/ — OpenCode Zen service documentation
12. https://opencode.ai/ — Homepage
13. https://deepwiki.com/sst/opencode/2.1-session-lifecycle-and-state — Session lifecycle deep dive
14. https://deepwiki.com/sst/opencode/2.9-storage-and-database — Storage/database architecture
15. https://deepwiki.com/sst/opencode/5.3-built-in-tools-reference — Built-in tools reference
16. https://dev.to/uenyioha/porting-claude-codes-agent-teams-to-opencode-4hol — Multi-agent orchestration
17. https://gist.github.com/gc-victor/1d3eeb46ddfda5257c08744972e0fc4c — Orchestrator agent guide
18. https://github.com/RogerioSobrinho/codeme-opencode — Example config repository
19. https://cefboud.com/posts/coding-agents-internals-opencode-deepdive/ — Internal architecture deep dive
20. https://github.com/rothnic/opencode-agents/blob/main/docs/custom-coding-agents.md — Custom agent creation guide

**Search queries used:**
- "opencode CLI agent tool 2025 2026 official documentation"
- "opencode.ai CLI tool what is it features"
- "opencode multi-agent orchestration subagents how they work 2025 2026"
- "opencode session state persistence storage SQLite 2025 2026"
- "opencode supported models providers list anthropic openai gemini 2026"
- "opencode tools available agents bash edit webfetch task tool list 2025 2026"
- "opencode \".opencode\" directory structure agents skills commands config files examples"
- "opencode AGENTS.md rules file project context persistent instructions format"
