# RustyTunes — Code Structure & Conventions

This file defines the project's structural conventions. Apply them when
adding new code or modifying existing code. Reorganize new modules to fit
this layout rather than inventing new top-level folders.

## Module layout

We use the Rust 2018+ style: a parent module lives in `<name>.rs` next
to its `<name>/` subfolder of children — never inside it as `mod.rs`.
See <https://doc.rust-lang.org/stable/book/ch07-02-defining-modules-to-control-scope-and-privacy.html>.

```
src/
├── main.rs             — entry point, tracing setup, runs MusicBotClient.
├── bot.rs              — framework wiring & MusicBotData. No event/business logic.
├── checks.rs           — declares `channel_checks`, `player_checks`.
├── checks/             — command/button checks invoked via poise's `check =`.
├── commands.rs         — declares feature submodules + `help`.
├── commands/           — thin command handlers grouped by feature area.
│   ├── activity.rs     — declares `cmd_break`, `cmd_gather`.
│   ├── activity/       — coupled features (gather + break).
│   ├── help.rs
│   ├── music.rs        — declares music command files.
│   ├── music/          — playback (play, pause, queue, …).
│   ├── reputation.rs   — Rep struct + shared `process_rep`, declares subcommands.
│   ├── reputation/     — rep +/-/list.
│   ├── utility.rs      — declares utility command files.
│   └── utility/        — standalone utilities (uwu, wakeup, rename, notify).
├── embeds.rs           — declares feature embed submodules.
├── embeds/             — Discord embeds, grouped by feature area.
│   ├── activity.rs
│   ├── activity/       — break, gather.
│   ├── bot.rs
│   ├── bot/            — bot-level error/voice embeds.
│   ├── music.rs
│   ├── music/          — player, queue.
│   ├── reputation.rs
│   ├── reputation/     — rep.
│   ├── utility.rs
│   └── utility/        — notify.
├── handlers.rs         — declares error/queue/voice handlers.
├── handlers/           — every async/event handler (Serenity/Songbird/poise).
│   ├── error_handler.rs
│   ├── queue_handler.rs
│   └── voice_handler.rs
├── player.rs           — declares `player`, `track`.
├── player/             — Music bot player only.
│   ├── player.rs       — Player struct, state transitions, activity helpers.
│   └── track.rs        — Track / Playlist / TrackSource / PlaybackError types.
├── service.rs          — declares all service files.
├── service/            — business-logic services that back the commands.
│   ├── break_service.rs
│   ├── cache_service.rs
│   ├── channel_service.rs
│   ├── embed_service.rs
│   ├── gather_service.rs
│   ├── normalize_service.rs
│   ├── notifier_service.rs
│   └── picker_service.rs
├── sources.rs          — declares `local_player`, `spotify_player`, `youtube_player`.
├── sources/            — track sources used by commands and services.
│   ├── local_player.rs
│   ├── spotify_player.rs
│   └── youtube_player.rs
├── utils.rs            — declares `string_utils`, `time_utils`.
└── utils/              — pure, shared helpers reusable across services/commands.
    ├── string_utils.rs — number_to_emoji, sanitize_name, MAX_NAME_LEN.
    └── time_utils.rs   — get_current_time, humanize_duration, parse_text, …
```

## Conventions

1. **Embeds**: All Discord embeds live under `embeds/`. Group them in
   subfolders that mirror the feature area (`embeds/music/queue_embed.rs`,
   `embeds/activity/break_embed.rs`). Do not define embeds inline inside
   commands or services — define a variant on the feature's embed enum and
   call `.to_embed()` from there.

2. **Utils**: When a helper is used by more than one service or command,
   move it to `utils/` and group by topic (`utils/time_utils.rs`,
   `utils/string_utils.rs`). Helpers used by exactly one module stay local
   to that module. Each topic file should expose stateless, pure
   functions — no Discord types, no I/O.

3. **Handlers**: Every event handler — Serenity `FullEvent`, Songbird
   `EventHandler`, poise error/lifecycle hooks — lives in `handlers/`.
   `bot.rs` should not contain inline event-handling logic; delegate to a
   `handlers/<name>_handler.rs` entry point.

4. **Services vs commands**: Commands are thin orchestrators. They:
   - Parse arguments and validate via `checks/`.
   - Call into `service/`, `player/`, or `sources/` to do the work.
   - Send embeds via `service::embed_service::SendEmbed`.

   Substantive logic (loops, retry handling, multi-step flows, state
   mutation) belongs in a service. If a command grows beyond ~50 lines or
   acquires nested helpers, push the body into a service.

5. **Sources**: Track origins (`spotify`, `youtube`, `local`) live in
   `sources/` as flat files named `<name>_player.rs`. Both commands and
   services may call into them. A new source goes in as a sibling file —
   no per-source subfolder unless it grows enough to need one.

6. **Coupled commands stay together**: When commands hand off to each
   other (`break` ends and auto-starts `gather`), put them in a shared
   `commands/<feature>/` folder. Don't split them across `utility/` and
   another folder just because the entry points have different names.

7. **Checks**: The `checks/` folder contains *only* check functions used
   by `#[poise::command(... check = "…")]` and by interactive button
   handlers. Determinations of "is this action doable right now" go here.
   Lower-level state predicates that the player uses internally to guard
   its own methods stay on `Player`.

8. **Module names**: Files keep their `_service`, `_handler`, `_embed`,
   `_player`, `_checks`, `_utils` suffix in the filename so the role is
   obvious in `use` statements. Picking the right suffix is part of
   choosing the right folder.

   No `mod.rs`. A module that has children lives in `<name>.rs` next to
   the `<name>/` folder it owns. Never add a `mod.rs` inside the folder.

9. **Player folder**: `player/` is only for the music bot's playback
   engine — the `Player` state machine and the value types (`Track`,
   `Playlist`, `TrackSource`, `PlaybackError`) it owns. Anything that
   isn't part of playing audio (notifications, reminders, gather/break
   state) belongs in `service/`. Split long files in `player/` by topic
   (state machine vs. data types) rather than letting them grow.

## Formatting

`rustfmt.toml` pins `max_width = 200`. A GitHub Action
(`.github/workflows/fmt.yml`) runs `cargo fmt --all` on every push to
`master` and auto-commits any changes as `style: cargo fmt`. Feature
branches don't need to be perfectly formatted — that gets ironed out on
merge — but running `cargo fmt` locally keeps diffs small.

## Adding a feature: checklist

- Embeds go in `embeds/<area>/<feature>_embed.rs`.
- Business logic goes in `service/<feature>_service.rs` (or extends an
  existing service when it fits).
- Source-specific code goes in `sources/<source>_player.rs`.
- Event/listener code goes in `handlers/<event>_handler.rs`.
- Command file in `commands/<area>/cmd_<name>.rs` that calls the service.
- Shared helpers (time, string, parsing) extract into `utils/`.
- Register the command in `bot.rs`.

If a refactor would violate any of the above, fix the layout first and
then make the change.
