# Architecture

## Primary interface

The library presents one high-leverage interface:

```rust
App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(bullet_heaven_game::GamePlugin);
```

`GamePlugin` composes the internal feature plugins and fixed-step ordering.
Callers do not need to know individual systems. The binary remains a thin
desktop adapter responsible for the window and Bevy's platform plugins.

The other intentional public seam is the validated content schema:
`GameConfig`, `ContentCatalog`, and the stable ID types. Configuration loading
hides parsing, duplicate detection, cross-reference checks, and lookup maps.

## State flow

```text
MainMenu ──new run──> Playing ──XP threshold──> LevelUp
   ▲                    │  ▲                         │
   │                    │  └────choice──────────────┘
   │                    ├────────pause────────> Paused
   │                    │                         │
   │                    │<────────resume──────────┘
   │                    ├────────death────────> GameOver
   │                    └────────boss death───> Victory
   │
   └──────────── main-menu actions from terminal screens

MainMenu or Paused ──> Settings ──back──> previous state
```

Only `Playing` runs gameplay systems. UI and real-time presentation continue
while paused or choosing an upgrade. A `RunRequest` distinguishes entering
`Playing` to start/retry a run from returning to it after pause or level-up.

## Fixed simulation

Gameplay runs at 60 Hz in explicitly ordered sets:

```text
Input
  → Movement
  → Spawning
  → Attacks
  → Collision
  → Damage
  → Progression
  → Cleanup
```

This ordering is part of the gameplay module's interface. New systems should
join the narrowest applicable set and declare additional ordering only where
one system truly depends on another.

Rendering and UI use the variable-rate `Update` schedule. Gameplay entities
carry rule data (`Health`, `Collider`, `Enemy`, projectiles); presentation
systems attach sprites and react to `DamageApplied`. This keeps headless tests
on the same simulation seam used by the game.

Regular enemy definitions select a pursuer, dasher, or shooter behavior.
Authored values live in the content catalog while private `EnemyBrain` state
tracks cooldowns and phases. Presentation reads only the telegraph state.

## Run ownership and cleanup

Every run-owned entity carries `RunEntity`. Starting a requested run or
entering the main menu removes those entities and replaces run-scoped
resources. Pause and level-up transitions preserve them.

There is no global game-manager object. State is represented by:

- Components for per-entity data
- Resources for unique run/build/catalog state
- Typed messages for occurrences such as damage, death, and XP collection
- Bevy states for application modes

## Determinism

A displayed `u64` seed creates independent ChaCha streams for spawning, loot,
and upgrade drafts. Adding a loot roll therefore does not scramble enemy
spawns. The project promises repeatable debugging inputs, not bit-identical
replays across engine/platform changes.

## Collision

Gameplay uses circle colliders and rebuilds a uniform 128-unit spatial grid
from enemies every fixed tick. Player attacks and contact checks query nearby
cells. This avoids a physics-engine dependency and keeps the spatial interface
replaceable if profiling later justifies a different implementation.

## Derived player stats

Player stats are recalculated from immutable base configuration plus the
current authored upgrade value. Systems consume `ResolvedStats`; they do not
incrementally mutate derived values. Reordering or removing upgrades therefore
cannot leave stale bonuses behind.

## Configuration and persistence

`assets/config/game.ron` is required content with an embedded fallback for
testability and basic executable resilience. Startup parsing or validation
errors stop the application with a precise message. F9 reloads a valid catalog
in development builds.

Preferences use the OS-appropriate configuration directory via `directories`.
Writes go to a temporary file and are renamed atomically. Missing or invalid
preferences fall back to defaults and never prevent play.

## Verification strategy

- Pure tests cover XP curves, deterministic drafts, stat resolution, parsing,
  validation, and preference defaults.
- Headless Bevy tests cross the actual gameplay interface: requested run
  creation, fixed simulation/spawning, and boss transition.
- The native smoke mode runs the real renderer, window, UI, and simulation.
- CI formats, lints, tests, and compile-checks all desktop targets.
