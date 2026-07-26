# Extension task list

## Add a spawn stage

- [ ] Append a stage with a strictly increasing `starts_at_seconds`.
- [ ] Set a positive spawn rate and enemy cap.
- [ ] Add nonzero enemy weights.
- [ ] Confirm the stage becomes active after its start time.

## Add a stat upgrade

- [ ] Add an `UpgradeKind` variant and corresponding `ResolvedStats` field.
- [ ] Handle the variant in the level-up module's derived-stat resolution.
- [ ] Add five authored values to the RON catalog.
- [ ] Keep the resolved value derived from base configuration.
- [ ] Add a focused headless level-up flow test.

## Add a weapon

- [ ] Decide whether to reuse `bolt`, `nova`, or `orbit` behavior.
- [ ] Add a weapon entry to the RON catalog.
- [ ] For a new behavior, add a `WeaponKind` variant.
- [ ] Implement one focused attack system or branch in `tick_weapons`.
- [ ] Reuse projectile, cooldown, targeting, damage, and collision primitives.
- [ ] Put new attack logic in `GameplaySet::Attacks`.
- [ ] Add five authored levels.
- [ ] Add a headless behavioral test.

## Add a gameplay message

- [ ] Confirm multiple systems or presentation adapters need the occurrence.
- [ ] Define a typed Bevy `Message`.
- [ ] Register it inside the plugin that owns its meaning.
- [ ] Keep durable state in components or resources.

## Add a screen

- [ ] Add a `GameState` variant.
- [ ] Add enter and exit systems for a marked root entity.
- [ ] Add menu actions that request the required state transitions.
- [ ] Keep gameplay-time advancement out of UI systems.

## Replace primitive art

- [ ] Replace or extend the `presentation` systems for players and enemies.
- [ ] Replace or extend projectile and pickup presentation.
- [ ] Keep sprites out of simulation entities.
- [ ] Confirm headless gameplay tests still pass without assets.

## Add audio

- [ ] Create an audio presentation plugin.
- [ ] Drive audio from durable state and typed gameplay messages.
- [ ] Keep audio optional for simulation systems and headless tests.
