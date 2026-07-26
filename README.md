# Bullet Heaven Game

A deliberately small, extensible bullet-heaven MVP built with Rust and
[Bevy](https://bevy.org/). It provides a complete Vampire Survivors-style run
without imposing a theme, art direction, or metagame.

## Play

Requirements:

- Rust 1.95 or newer
- Linux, Windows, or macOS
- On Linux, the system packages listed in Bevy's
  [setup guide](https://bevy.org/learn/quick-start/getting-started/setup/)

```sh
cargo run
```

The first build compiles Bevy and can take a few minutes. Later builds are
incremental.

## Controls

| Action | Keyboard / Mouse |
| --- | --- |
| Move | WASD or arrows |
| Navigate | Arrows or mouse |
| Confirm | Enter, Space, or click |
| Back / Pause | Escape |
| Fullscreen | F11 or Settings menu |

Development builds also include:

| Key | Action |
| --- | --- |
| F1 | Toggle diagnostics |
| F2 | Grant 100 XP |
| F3 | Toggle invulnerability |
| F4 | Advance the run by 60 seconds |
| F5 | Spawn a mixed test wave |
| F6 | Toggle collider/spatial-cell outlines |
| F9 | Reload `assets/config/game.ron` |

## Included game loop

- Ten-minute, seeded survival runs
- Automatic Bolt, Nova, and Orbit weapons with five levels each
- Six five-level stat upgrades and three-choice level-up drafts
- Pursuer, telegraphed dasher, and ranged shooter enemies plus a charge/burst boss
- XP gems, rare healing drops, contact damage, and invulnerability frames
- Time-based spawn stages, arena boundaries, victory/defeat, and seed retry
- Keyboard movement and keyboard/mouse menus
- Persistent fullscreen and VSync preferences

All graphics are code-generated primitives. Text uses Bevy's embedded Fira
Mono subset, so the repository requires no external art or font files.

## Shape of the project

`src/main.rs` is a desktop adapter. The reusable interface is
`bullet_heaven_game::GamePlugin`.

| Module | Owns |
| --- | --- |
| `config` | RON parsing, validation, typed content catalogs |
| `gameplay` | Fixed-step rules, run state, combat, spawning, progression |
| `input` | Keyboard input normalization |
| `presentation` | Sprites, camera, hit feedback, arena visuals |
| `ui` | Menus, HUD, settings, upgrade and results screens |
| `persistence` | Atomic, platform-native preference storage |
| `developer` | Debug-only overlay, shortcuts, smoke runner |

Read [ARCHITECTURE.md](docs/ARCHITECTURE.md) for state and system flow, then
[EXTENDING.md](docs/EXTENDING.md) for concrete extension recipes.

## Content and balance

All shipped balance lives in [game.ron](assets/config/game.ron). The file
defines the arena, player, XP curve, weapons, upgrades, enemies, spawn stages,
and boss. It is validated before the main menu opens. Invalid IDs or values
produce actionable startup errors.

Press F9 in a development build to reload it. Reload from the main menu when
changing or removing IDs that an active run may own.

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
```

A deterministic native smoke run is also available in development builds:

```sh
BULLET_HEAVEN_SMOKE_SECONDS=8 cargo run
```

It starts seed `0xB011E7`, runs the real renderer and simulation, then exits
successfully.

Set `BULLET_HEAVEN_SMOKE_START_SECONDS` to exercise a later spawn stage without
waiting through the full run:

```sh
BULLET_HEAVEN_SMOKE_SECONDS=8 BULLET_HEAVEN_SMOKE_START_SECONDS=180 cargo run
```

Accelerated smoke runs enable invulnerability and the diagnostics overlay.

## Packaging

The executable expects the `assets` directory beside the working directory:

```text
bullet-heaven-game
assets/
  config/game.ron
```

Tagged GitHub releases build a Linux archive containing that layout. CI checks
the source on Linux, Windows, and macOS.

## Intentionally deferred

Audio, sprite animation, permanent progression, run saves, achievements,
localization, rebinding UI, weapon evolutions, chests, shops, procedural maps,
multiplayer, and stable mod compatibility are intentionally outside this MVP.

## License

MIT. See [LICENSE](LICENSE).
