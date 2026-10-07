# Channel scoped permissions + provenance — change notes

Branch: `feat/channel-scoped-permissions-provenance`
Base: `32f2d56`

Goal: scoped per-guild/channel/user tool+skill permissions, per-scope rate
limits, cron/SOP provenance (`where/when/who created`), minimal management UI.

Natural order (each = one PR-sized slice):
1. **Provenance IDs (1a)** — `ChannelMessage.guild_id/channel_id/user_id`,
   `zeroclaw.*` attribution, `scope!` propagation. [this slice]
2. **Cron origin (1b)** — `CronJob.origin`, DB migration, API exposure.
3. **Scoped policy (2)** — `channels.<type>.<alias>.rules[]`, enforcement seam.
4. **Scoped rate limits + approval (3)** — keyed buckets, `require_approval`.
5. **Management UI (4)** — `/channels/:type/:alias` dense page.

## Slice 1a — engine changes

- `zeroclaw-api/src/channel.rs`: `ChannelMessage` +=
  `guild_id/channel_id/user_id: Option<String>`. Additive, `Default=None`,
  all constructors use `..Default::default()` so no breakage.
  Canonical source for inbound identity stays `ChannelMessage`.
- `zeroclaw-log/src/event.rs`: `ATTRIBUTION_FIELDS` +=
  `guild_id, channel_id, user_id`. Single source — Layer/reader/gateway/UI
  read this list at runtime, no per-field plumbing.
- `zeroclaw-log/src/layer.rs`: `ScopeSpanCollector::put` skips empty-string
  attribution values (treats `""` as absent). Prevents `None→""` pollution
  from optional IDs. No behavior change for non-empty values.
- `zeroclaw-channels/src/discord/mod.rs`: `MESSAGE_CREATE` populates
  `guild_id (d.guild_id)`, `channel_id (d.channel_id)`, `user_id (author_id)`.
- `zeroclaw-channels/src/telegram.rs`: populates
  `channel_id (chat_id)`, `user_id (sender_id)` on text/voice/attachment paths.
- `zeroclaw-channels/src/orchestrator/mod.rs`: `process_channel_message`
  `scope!` += `guild_id/channel_id/user_id` so every descendant `record!`
  (tool invoke/complete, approvals, cron_add) inherits provenance.

Validation:
- `cargo test -p zeroclaw-log -p zeroclaw-api`
- `cargo clippy -p zeroclaw-log -p zeroclaw-api -p zeroclaw-channels`
- `cargo fmt --check`

## Slice 1b — cron origin (this commit, pending)

- `zeroclaw-runtime/src/cron/types.rs`: `CronOrigin{channel,guild_id,
  channel_id,user_id,message_id}` + `CronJob.origin: Option<CronOrigin>`
  (serde default/skip, immutable after creation). Task-local
  `CRON_ORIGIN` + `scope_cron_origin()` / `current_cron_origin()` following
  the `LIVE_SOP_ACTION_QUEUE` pattern — no `Tool` trait churn, no
  model-supplied forgery.
- `zeroclaw-runtime/src/cron/store.rs`: `origin TEXT` column (CREATE TABLE
  + `add_column_if_missing` migration), all 5 SELECTs extended, tolerant
  row decode (missing column → `None`), `add_*_with_origin` variants
  (`add_shell_job_with_format_and_origin`, `add_agent_job_with_origin`);
  old signatures delegate with `None`. `encode_origin` stores `None` for
  empty origins.
- `zeroclaw-runtime/src/cron/mod.rs`: re-exports + shell wrapper reads
  `current_cron_origin()` internally.
- `zeroclaw-runtime/src/tools/cron_add.rs`: agent path passes
  `current_cron_origin()`; shell path inherits via wrapper.
- `zeroclaw-runtime/src/tools/cron_common.rs`: `cron_add_output` includes
  `origin`; `cron_job_output`/gateway `list_jobs` expose it via serde.
- `zeroclaw-channels/src/orchestrator/mod.rs`: `process_channel_message`
  wraps body in `scope_cron_origin(Some(origin))`; scheduler/CLI paths
  have no scope → `None`.
- Declarative sync jobs: `origin = None` (config-owned, not chat-created).

## Slice 2 — scoped tool permissions (uncommitted)

- `zeroclaw-config/src/schema.rs`: `ChannelScopeRule{match_guilds,
  match_channels, match_users, allow_tools, deny_tools}` + `matches()` /
  `excluded_for_turn()` (deny-union, allow-intersection, deny wins).
  `scope_rules: Vec<ChannelScopeRule>` on `TelegramConfig` + `DiscordConfig`
  only. Rules narrow the risk-profile policy, never widen.
- `zeroclaw-runtime/src/tools/scoped.rs`: read-only `tool_names()`
  accessor on the sealed registry (engine change: exposes existing state,
  no behavior change, seal holds).
- `zeroclaw-channels/src/orchestrator/mod.rs`: `scope_rules_for_message()`
  (live config read, no snapshot) + `scoped_excluded_tools_for_turn()`,
  the single source for both the prompt path and the tool-loop path
  (incl. model-switch refresh). `cli`/`Full` bypass preserved.
- Skills surfacing inherits automatically (same `excluded_tools` predicate
  drives `refreshed_skills_system_prompt` + slash-spec filtering).
