# Docker Sandboxes Integration Plan

## Overview

Add Docker Sandboxes as a new executor type that runs agents in isolated microVMs instead of local processes. This requires changes across the stack: backend executor, frontend UI, and workspace creation flow.

---

## Phase 1: Backend Executor Implementation

### 1.1 Add Docker Sandbox Executor Type

**Files to modify:**
- `crates/executors/src/executors/mod.rs` - Add new enum variant
- New file: `crates/executors/src/executors/docker_sandbox.rs` - Implementation

**New executor config fields needed:**
```rust
// New config struct added to ExecutorConfig
pub struct DockerSandboxConfig {
    pub agent: DockerAgent,           // claude, codex, gemini, opencode, copilot, shell
    pub template: Option<String>,   // Custom template image (e.g., "docker.io/my-org/my-template:v1")
    pub branch_mode: bool,        // Use git worktrees instead of direct mode
    pub network_policy: NetworkPolicy, // Open, Balanced, LockedDown
    pub secrets: Vec<Secret>,    // API keys and credentials
    pub extra_workspaces: Vec<WorkspaceMount>, // Additional mounted directories
    pub docker_size: Option<String>, // Docker volume size (e.g., "10g")
}

pub enum DockerAgent {
    Claude,
    Codex,
    Gemini,
    Copilot,
    Opencode,
    Droid,
    Shell,  // No agent, just shell access
}

pub enum NetworkPolicy {
    Open,
    Balanced,
    LockedDown,
}

pub struct Secret {
    pub scope: SecretScope, // anthropic, openai, github, etc.
    pub value: String,
}

pub struct WorkspaceMount {
    pub host_path: String,
    pub read_only: bool,
}
```

### 1.2 Executor Implementation

**Communication pattern:**
- Spawn `sbx run` or `sbx create` as local process
- Communicate via stdin/stdout (same as existing executors)
- Use JSON stream protocol for Claude, Stream-JSON for others

**Key methods to implement:**
```rust
impl DockerSandboxExecutor {
    pub async fn spawn(&mut self, config: &DockerSandboxConfig, workspace: &Path) -> Result<SessionInfo>;

    pub async fn run_prompt(&mut self, session: &SessionInfo, prompt: &str) -> Result<Response>;

    pub async fn attach(&mut self, session: &SessionInfo) -> Result<()>;
}
```

**Lifecycle commands:**
| Action | Command |
|--------|--------|
| Create sandbox | `sbx create --name <name> --branch <branch> <agent> <workspace>` |
| Run agent | `sbx run <sandbox-name>` |
| Stop sandbox | `sbx stop <sandbox-name>` |
| Remove sandbox | `sbx rm <sandbox-name>` |
| List sandboxes | `sbx ls` |
| Port forwarding | `sbx ports <sandbox> --publish <host>:<guest>` |

### 1.3 Workspace Manager Integration

**Files to modify:**
- `crates/workspace-manager/src/workspace_manager.rs` - Handle docker workspace lifecycle
- `crates/workspace-manager/src/` - Add DockerWorkspaceState

**New workspace state:**
```rust
pub enum WorkspaceBackend {
    Local,       // Current: git worktrees on host
    DockerSandbox, // New: docker sandbox with worktrees inside VM
}

pub struct DockerWorkspaceState {
    pub sandbox_name: String,
    pub sandbox_id: String,
    pub worktree_path: Option<PathBuf>, // Only if branch_mode=true
    pub created_at: DateTime<Utc>,
}
```

---

## Phase 2: API Type Changes

### 2.1 Update Request/Response Types

**Files to modify:**
- `crates/db/src/models/requests.rs` - Add DockerSandboxConfig
- `crates/api-types/` - Regenerate TypeScript types
- `shared/types.ts` - Auto-generated

**New fields in CreateAndStartWorkspaceRequest:**
```typescript
interface CreateAndStartWorkspaceRequest {
  name?: string;
  repos: WorkspaceRepoInput[];
  linked_issue?: LinkedIssueInfo;
  executor_config: ExecutorConfig;
  prompt: string;
  attachment_ids?: string[];
  // New Docker-specific fields
  backend?: 'local' | 'docker_sandbox';
  docker_config?: DockerSandboxConfig;
}
```

---

## Phase 3: Frontend UI Changes

### 3.1 New Executor Option

**Files to modify:**
- `packages/web-core/src/shared/components/ModelSelectorContainer.tsx` - Add Docker executor option
- `packages/web-core/src/shared/components/CreateChatBoxContainer.tsx` - Show Docker config panel

**New executor dropdown options:**
- Current: Claude Code, Amp, Gemini, Codex, Opencode, Cursor Agent, Qwen Code, Copilot, Droid
- New: Docker: Claude, Docker: Codex, Docker: Gemini, Docker: Copilot, Docker: Opencode, Docker: Shell

### 3.2 Docker Configuration Panel

**New component:** `CreateDockerConfigPanel.tsx`

**Fields:**
| Field | Type | Description |
|-------|------|------------|
| Agent | Dropdown | claude, codex, gemini, copilot, opencode, shell |
| Template | Text input | Custom template image (optional) |
| Branch Mode | Toggle | Use git worktrees |
| Network Policy | Dropdown | Open, Balanced, LockedDown |
| Extra Workspaces | List | Additional mounted directories |
| Docker Size | Dropdown | 10g, 25g, 50g, 100g |
| Secrets | List | Add API keys for anthropic, openai, github, etc. |

### 3.3 Configuration UI Mockup

```
┌─────────────────────────────────────────────────────────────┐
│  Docker Sandbox Configuration                                │
├─────────────────────────────────────────────────────────────┤
│  Agent:        [Claude          ▼]                          │
│  Template:     [docker.io/...                 ] (optional)  │
│                                                             │
│  ☑ Branch Mode (use git worktrees)                         │
│                                                             │
│  Network Policy:  ○ Open  ● Balanced  ○ LockedDown       │
│                                                             │
│  Extra Workspaces:                                         │
│    [+ Add Workspace]                                       │
│                                                             │
│  Docker Size:    [50GB ▼]                                 │
│                                                             │
│  Secrets:                                            │
│    [+ Add Secret]                                        │
│    • anthropic (configured)                              │
│    • github (configured)                                │
├─────────────────────────────────────────────────────────────┤
│  [Create Workspace]                                      │
└─────────────────────────────────────────────────────────────┘
```

### 3.4 Secrets Management

**New component:** `DockerSecretsManager.tsx`

**Features:**
- List configured secrets (from OS keychain via `sbx secret ls`)
- Add new secret: prompts for key type and value, stores via `sbx secret set`
- Remove secret: `sbx secret rm`
- Secret types: anthropic, openai, google, github, azure-openai, etc.

---

## Phase 4: Validation and Error Handling

### 4.1 Prerequisites Check

**On workspace creation, validate:**
- [ ] `sbx` CLI is installed (`sbx --version`)
- [ ] User is logged in (`sbx whoami` works)
- [ ] Docker/sandbox permissions (KVM on Linux, Hyper-V on Windows)
- [ ] Sufficient disk space for docker_size

**Error messages:**
| Error | Message | Resolution |
|-------|--------|-----------|
| sbx not found | "Docker Sandboxes CLI not found" | Show install instructions |
| Not logged in | "Run `sbx login` first" | Prompt user to authenticate |
| No permissions | "Sandboxing not available" | Show platform requirements |
| Disk full | "Insufficient disk space" | Offer smaller docker_size |

### 4.2 Runtime Error Handling

**Detect sandbox state:**
```rust
async fn get_sandbox_status(name: &str) -> Result<SandboxStatus> {
    let output = Command::new("sbx").args(["ls", "--format", "json"]).output()?;
    // Parse JSON output for status
}
```

**Statuses:**
- running - Agent is active
- stopped - Sandbox exists but agent exited
- created - Sandbox exists, no agent started
- unknown - Not found

---

## Phase 5: Testing Strategy

### 5.1 Unit Tests

**New test file:** `crates/executors/src/executors/docker_sandbox_test.rs`

- Test config validation
- Test command construction
- Test status parsing

### 5.2 Integration Tests

- [ ] Create docker workspace (requires sbx installed)
- [ ] Run prompt and get response
- [ ] Test branch mode worktrees
- [ ] Test port forwarding
- [ ] Test cleanup

### 5.3 E2E Tests

- [ ] Full UI flow: select Docker executor → configure → create → use
- [ ] Error recovery: handle sbx not installed
- [ ] Multiple concurrent Docker workspaces

---

## File Summary

### New Files

| File | Purpose |
|------|---------|
| `crates/executors/src/executors/docker_sandbox.rs` | Main executor implementation |
| `crates/executors/src/executors/docker_sandbox/test.rs` | Unit tests |
| `packages/web-core/src/shared/components/CreateDockerConfigPanel.tsx` | Config UI |
| `packages/web-core/src/shared/components/DockerSecretsManager.tsx` | Secrets management UI |
| `packages/web-core/src/shared/hooks/useDockerSandbox.ts` | API hooks |

### Modified Files

| File | Changes |
|------|---------|
| `crates/executors/src/executors/mod.rs` | Add DockerSandbox variant |
| `crates/db/src/models/requests.rs` | Add DockerSandboxConfig |
| `crates/workspace-manager/src/workspace_manager.rs` | Handle docker backend |
| `packages/web-core/src/shared/components/CreateChatBoxContainer.tsx` | Show docker config |
| `packages/web-core/src/shared/components/ModelSelectorContainer.tsx` | Add docker options |
| `packages/web-core/src/shared/hooks/useCreateWorkspace.ts` | Handle docker config |

---

## Open Questions

1. **Single repo vs multiple:** Docker sandboxes can mount multiple workspaces. Should vibe-kanban support multiple host repos mounted into one sandbox?

2. **Port forwarding:** Dev servers run inside sandbox. Should vibe-kanban auto-forward ports back to host? How to discover which port?

3. **Branch naming:** vibe-kanban generates branch names. How to integrate with `--branch auto` from sbx?

4. **Template registry:** Should users be able to specify custom template images? How to validate?

5. **Existing sandbox reuse:** Docker sandboxes persist. Should vibe-kanban try to reconnect to existing sandboxes for the same workspace?

6. **Credentials injection:** Currently vibe-kanban doesn't handle API keys. Docker sandbox has `sbx secret` for this. Should vibe-kanban integrate?

---

## Dependencies

- **sbx CLI** must be installed on system running vibe-kanban
- **Docker account** for authentication
- **Platform support:** macOS (Apple silicon), Windows 11 (Hyper-V), Linux (KVM)
- **Minimum workspace:** Docker Sandboxes requires significant disk space and memory

---

## Timeline Estimate

| Phase | Effort |
|-------|--------|
| Phase 1: Backend | 2-3 days |
| Phase 2: API Types | 0.5 day |
| Phase 3: UI | 2-3 days |
| Phase 4: Error Handling | 1 day |
| Phase 5: Testing | 1-2 days |
| **Total** | **6-9 days** |