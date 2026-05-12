# Conduit — Phase 5: Limit Monitoring + Account Failover Design

**Date:** 2026-05-12
**Scope:** `SpendTracker`, `FallbackResolver`, `--account` flag, `auto_switch` config, `conduit providers add` extended flow, `conduit status` spend display
**Status:** Approved
**Depends on:** Phase 4 (ParallelRunner, ProviderResolver + Send + Sync)

---

## Overview

Phase 5 adds per-account spend tracking and automatic failover to `conduit run`. Each `AIAccount` can opt into auto-switch with a configurable trigger (`error`, `limit`, or `both`). When triggered, `FallbackResolver` tries the next available account — same provider type first, then any other — without changing `PipelineRunner` or `ParallelRunner`. A local spend log (`~/.conduit/spend.toml`) tracks daily invocation counts and estimates spend against `daily_limit_usd`. A new `--account` flag lets users override account selection at runtime.

---

## Config Changes

### New fields on `AIAccount`

```toml
[[ai_account]]
name = "claude-work"
provider = "claude"
daily_limit_usd = 10.0       # existing field
auto_switch = true            # NEW: enable failover from this account
switch_on = "both"            # NEW: "error" | "limit" | "both"
cost_per_run = 0.05           # NEW: estimated USD per stage invocation
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `auto_switch` | `bool` | `false` | If false, errors go directly to the user — no failover |
| `switch_on` | `"error" \| "limit" \| "both"` | `"error"` | What triggers the switch |
| `cost_per_run` | `Option<f64>` | none | Estimated USD per stage invocation; required for limit checks |

**`switch_on` semantics:**
- `"error"` — failover when the provider CLI returns a non-zero exit code (`AgentInvocationFailed`)
- `"limit"` — skip this account before invocation when `invocations × cost_per_run >= daily_limit_usd`; no failover on runtime errors
- `"both"` — apply both checks

If `cost_per_run` is not set, `"limit"` and `"both"` triggers behave as `"error"` only (limit check is skipped).

### Rust struct

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AIAccount {
    pub name: String,
    pub provider: String,
    pub daily_limit_usd: Option<f64>,
    pub auto_switch: Option<bool>,
    pub switch_on: Option<String>,   // "error" | "limit" | "both"
    pub cost_per_run: Option<f64>,
}

impl AIAccount {
    pub fn auto_switch_enabled(&self) -> bool {
        self.auto_switch.unwrap_or(false)
    }

    pub fn switch_on_error(&self) -> bool {
        matches!(self.switch_on.as_deref(), Some("error") | Some("both") | None)
            && self.auto_switch_enabled()
    }

    pub fn switch_on_limit(&self) -> bool {
        matches!(self.switch_on.as_deref(), Some("limit") | Some("both"))
            && self.auto_switch_enabled()
            && self.cost_per_run.is_some()
            && self.daily_limit_usd.is_some()
    }
}
```

---

## Spend Tracking

### File: `~/.conduit/spend.toml`

```toml
[2026-05-12]
claude-work = 14
openai-personal = 3

[2026-05-11]
claude-work = 22
```

One section per calendar date (ISO 8601). Each key is an account name; value is total stage invocations that day. Entries for previous days are retained but not counted toward current limits. Old entries can be pruned after 30 days (but this is not required in Phase 5).

### New file: `conduit-core/src/spend.rs`

```rust
pub struct SpendTracker {
    // internal: BTreeMap<NaiveDate, HashMap<String, u64>>
}

impl SpendTracker {
    pub fn load() -> Result<Self, ConduitError>;
    pub fn record(&mut self, account: &str);
    pub fn today_invocations(&self, account: &str) -> u64;
    pub fn estimated_spend(&self, account: &AIAccount) -> f64;
    pub fn is_over_limit(&self, account: &AIAccount) -> bool;
    pub fn save(&self) -> Result<(), ConduitError>;
}
```

- `load()` reads `~/.conduit/spend.toml`; if the file doesn't exist, returns an empty tracker (not an error)
- `record()` increments today's count for the named account then saves immediately (write-through)
- `estimated_spend()` returns `today_invocations(account) as f64 * cost_per_run` (0.0 if either is unset)
- `is_over_limit()` returns `estimated_spend() >= daily_limit_usd` when both values are set; false otherwise
- `save()` writes the full map back to `~/.conduit/spend.toml`

`SpendTracker` must be `Send + Sync` so it can be shared across rayon threads via `Arc<Mutex<SpendTracker>>`.

---

## FallbackResolver

### Location: `conduit-core/src/provider.rs`

```rust
pub struct FallbackResolver<'a> {
    profile: &'a Profile,
    config: &'a Config,
    spend: Arc<Mutex<SpendTracker>>,
    account_override: Option<&'a str>,
}
```

### `resolve()` algorithm

```
1. Determine primary account name:
   - If account_override is set → use it
   - Else → profile.account_for_stage(stage)

2. Build candidate list:
   - Primary account (if it exists in config)
   - Other accounts with the same provider type (in config order)
   - Remaining accounts (any provider, in config order)
   - Dedup — each account appears at most once

3. For each candidate account in order:
   a. If auto_switch_enabled() == false:
      → try the provider; return Ok or Err immediately (no further candidates)
   b. If switch_on_limit() == true AND spend.is_over_limit(account):
      → log "[account] skipped: over daily limit"
      → continue to next candidate
   c. Try the provider:
      - On success: spend.record(account); return Ok(provider)
      - On AgentInvocationFailed:
          - spend.record(account)
          - If switch_on_error() == true: continue to next candidate
          - Else: return Err immediately

4. If all candidates exhausted:
   → return Err(ConduitError::AllAccountsExhausted { stage })
```

### New error variant (`conduit-core/src/error.rs`)

```rust
#[error("All accounts exhausted for stage `{stage}`: every configured account is over its limit or failed")]
AllAccountsExhausted { stage: String },
```

---

## CLI Changes

### `conduit run` — new `--account` flag

```
conduit run [--account <name>] [--profile <name>] [--concurrency <n>] [--task <id>]
```

- If `--account <name>` is given and the account doesn't exist in config → fail immediately before loading tasks: `"Account 'X' not found. Run \`conduit providers list\` to see available accounts."`
- Passed to `FallbackResolver` as `account_override`
- `--account` and `--profile` can be combined: profile defines per-stage assignments, `--account` overrides the starting account for all stages

**`conduit-cli/src/main.rs` change:**
```rust
Run {
    #[arg(long)] task: Option<String>,
    #[arg(long)] profile: Option<String>,
    #[arg(long)] concurrency: Option<usize>,
    #[arg(long)] account: Option<String>,   // NEW
},
```

**`conduit-cli/src/commands/run.rs` change:**
- Signature: `pub fn run(..., account: Option<&str>) -> Result<()>`
- Validate account exists before loading tasks
- Pass `account` to `FallbackResolver::new()`
- Replace `ProfileResolver` with `FallbackResolver` throughout

### `conduit providers add` — extended interactive flow

After successful login and name entry, two new prompts:

```
Enable auto-switch for this account? [y/n] (default: n)
```

If yes:
```
Switch when?
  [1] On error (provider CLI fails)
  [2] On daily limit (estimated spend reaches daily_limit_usd)
  [3] Both
```

If choice is 2 or 3, and `daily_limit_usd` is not already set:
```
Daily limit (USD, e.g. 10.0):
Estimated cost per stage run (USD, e.g. 0.05):
```

If choice is 2 or 3, and `daily_limit_usd` IS already set:
```
Estimated cost per stage run (USD, e.g. 0.05):
```

### `conduit status` — spend display

```
AI Accounts:
  claude-work  (claude)  $10.00/day  ~$0.70 used today  auto-switch: both
  openai-personal  (openai)  no limit  auto-switch: off
  gemini-free  (gemini)  $5.00/day  cost tracking not configured  auto-switch: error
```

- `~$X.XX used today` — shown when `cost_per_run` is set; computed as `invocations × cost_per_run`
- `cost tracking not configured` — shown when `daily_limit_usd` set but `cost_per_run` not set
- `auto-switch: off` — shown when `auto_switch = false` (or not set)

---

## Data Flow

```
conduit run --account claude-work --concurrency 2
  └─ validate --account exists in config
  └─ load_tasks() → Vec<Task>
  └─ load_global_config() → Config
  └─ select profile → Profile
  └─ SpendTracker::load() → SpendTracker
  └─ FallbackResolver::new(profile, config, spend, Some("claude-work"))
  └─ ParallelRunner::run()
       ├─ task-1: PipelineRunner
       │    └─ stage Orchestrator:
       │         FallbackResolver.resolve("orchestrator")
       │           1. primary = "claude-work"
       │           2. candidates = [claude-work, claude-personal, codex-main]
       │           3. claude-work: over limit → skip
       │           4. claude-personal: try → success → record + return
       └─ task-2: PipelineRunner
            └─ stage Orchestrator:
                 FallbackResolver.resolve("orchestrator")
                   1. primary = "claude-work"
                   2. claude-work: over limit → skip
                   3. claude-personal: try → AgentInvocationFailed → record → try next
                   4. codex-main: try → success → record + return
```

---

## Error Handling

| Scenario | Behaviour |
|---|---|
| `--account` name not in config | Fail immediately, before loading tasks |
| Account over limit, `switch_on = "limit"` | Skip account; try next |
| Provider CLI fails, `switch_on = "error"` | Try next account |
| All accounts over limit or failed | `AllAccountsExhausted` error for that stage → task fails |
| `auto_switch = false` | No failover; return the error directly |
| `cost_per_run` not set but `switch_on = "limit"` | Limit check silently skipped; behaves as `"error"` |
| `SpendTracker` file unreadable | Load returns empty tracker (non-fatal); spend not tracked |

---

## Testing

### Unit tests (`conduit-core`)

**`SpendTracker`:**
- Load from missing file → empty tracker, no error
- `record()` increments today's count
- `is_over_limit()` false when `cost_per_run` not set
- `is_over_limit()` true when `invocations × cost_per_run >= daily_limit_usd`
- Yesterday's invocations don't count toward today's limit
- Save + reload round-trip preserves all entries

**`FallbackResolver`:**
- `auto_switch = false` → returns provider error, no fallback attempted
- `switch_on = "error"` → skips to next on `AgentInvocationFailed`, same-provider first
- `switch_on = "limit"` → skips over-limit account; does NOT fallover on runtime error
- `switch_on = "both"` → skips over-limit AND falls over on error
- All candidates exhausted → `AllAccountsExhausted`
- `account_override` → starts candidate list from named account
- Same-provider candidates are tried before cross-provider candidates

### Integration tests (`conduit-cli`)

- `conduit run --account nonexistent` → fails with "Account not found" before loading tasks
- `conduit run --account fake` with no fallback → fails with `AllAccountsExhausted` (or `NoProviderAvailable`)
- `conduit status` with spend data present → shows estimated usage per account

---

## Dependencies Added

None — no new crates required. `chrono` is not needed; use `std::time::SystemTime` with `time` formatting, or the simple `YYYY-MM-DD` string from the system date via `std::time`.

Actually, date handling without `chrono` is verbose. Add:

| Crate | Where | Used for |
|---|---|---|
| `chrono = { version = "0.4", features = ["serde"] }` | `conduit-core/Cargo.toml` | Current date for spend log keys |

---

## Out of Scope for Phase 5

- Actual API cost data from providers (Phase 7)
- Per-account concurrency limiting (max N parallel tasks on one account simultaneously)
- Spend log pruning / rotation
- Notifications when approaching limit
- TUI dashboard (Phase 8)

---

## Success Criteria

Phase 5 is complete when:

1. `auto_switch`, `switch_on`, `cost_per_run` fields are configurable in `~/.conduit/config.toml`
2. `conduit providers add` prompts for auto-switch settings
3. `SpendTracker` records invocations per account per day in `~/.conduit/spend.toml`
4. `FallbackResolver` respects `switch_on = "error"`, `"limit"`, `"both"`
5. Failover tries same-provider accounts first, then any provider
6. `conduit run --account <name>` overrides starting account
7. `conduit status` shows estimated daily spend per account
8. `AllAccountsExhausted` error when no candidate succeeds
9. All Phase 1–4 tests still pass
10. New unit and integration tests for Phase 5 pass
