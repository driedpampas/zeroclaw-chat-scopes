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
