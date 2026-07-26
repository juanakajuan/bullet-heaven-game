# Extending the template

## Add a regular enemy

1. Add an entry to `enemies` in `assets/config/game.ron`.
2. Give it a unique ID, stats, color, and geometric shape.
3. Reference the ID from one or more stage weight tables.
4. Run `cargo test config::tests` or press F9 from the main menu.

All regular enemies currently use pursuit behavior. A new movement behavior is
a rule change: add an authored behavior enum to `EnemyConfig`, then handle it
inside the enemy movement module rather than branching on an ID.

## Add a spawn stage

Append a stage with a strictly increasing `starts_at_seconds`, positive spawn
rate, cap, and nonzero weights. The active stage is the last stage whose start
time has passed.

## Add a stat upgrade

The schema currently maps six `UpgradeKind` variants to `ResolvedStats`.

1. Add the new enum variant and resolved-stat field.
2. Handle it in `resolve_stats`.
3. Add its five authored values to the RON catalog.
4. Add a focused stat-resolution test.

Keep resolved values derived from base configuration; do not mutate player
speed, damage, or health in multiple selection call sites.

## Add a weapon

For another instance of an existing behavior, add a weapon entry using `bolt`,
`nova`, or `orbit`.

For a genuinely different behavior:

1. Add a `WeaponKind` variant.
2. Implement one focused attack system or a focused branch in `tick_weapons`.
3. Reuse projectile, cooldown, targeting, damage, and collision primitives.
4. Put the system in `GameplaySet::Attacks`.
5. Add authored five-level values and a headless behavioral test.

Configuration controls values and progression; it is intentionally not a
general scripting language.

## Add a gameplay message

Use a typed Bevy `Message` when multiple systems or presentation adapters need
to react to an occurrence. Components or resources remain preferable for
durable state. Register the message inside the plugin that owns its meaning.

## Add a screen

Add a `GameState` variant, enter/exit systems that create and remove a marked
root entity, and menu actions that request state transitions. Do not let UI
systems directly advance gameplay time.

## Replace primitive art

Simulation entities do not own sprites. Replace or extend the `presentation`
systems that react to `Added<Player>`, `Added<Enemy>`, projectiles, and pickups.
Gameplay tests will continue to run without the new assets.

## Add audio later

Create an audio presentation plugin that reads durable state and typed gameplay
messages. It should not be required by simulation systems or headless tests.
