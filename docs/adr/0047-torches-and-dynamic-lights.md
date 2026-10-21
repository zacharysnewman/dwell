# 0047. Torches and dynamic lights: light sources as block data, held lights per fragment, placed ones through the flood light

- Status: Accepted (the owner's answers, 2026-10-21)
- Date: 2026-10-21
- Design: [`LIGHTS.md`](../LIGHTS.md); plan: Phase 47

## Context

The owner asked for a torch that lights its surroundings when held and when placed, casting
shadows (a setting of its own), and updating the block light when placed. The flood light (ADR 0045)
already spreads block light from blocks that `emits`, but nothing gave off light yet, and nothing
lit the world from a moving source.

## Decision

- **Light sources are block data**: a `lights` table in the block registry's namespaces (colour,
  strength at the flame, radius, flicker, whether it casts shadows), referenced by name from a block's
  `light`. The torch is a block with `emits` (its flood light) and `light: dwell:torch`; its held
  model is a character item named by the block's `held`. Lanterns and the like are data alone.
  Registry sync carries `light` and `held` as look tags, by name.
- **Held lights are per-fragment point lights, never in the flood light** (owner). The renderer
  gathers the lights in hands this frame (yours and other players', from the held items in the
  snapshots: no protocol change), keeps the nearest few within a distance (`shared/look/lights.json`)
  and every lit surface adds their irradiance, falling off on the flood light's block-light curve
  (so held and placed torches light alike). The flood light would lag
  and step behind a moving torch and re-mesh chunks as it went.
- **Placed lights go through the flood light** whatever the settings; with the Torch shadows
  setting on (T2) a placed torch's light is also a shadowed dynamic light on top.
- **Two settings** (owner): Sun & moon shadows (the Shadows setting) unchanged, and a new **Torch
  shadows** (Off / Low / High: how many lights cast shadows, and at what resolution; T2).
- **Glow is a texture property**: a material's `emissive` (shared/textures), per atlas tile for
  blocks and per material for characters.

## Rejected

- **Writing a held torch into the flood light** (Minecraft's dynamic-lights mods): whole-cell steps,
  a lag of frames, and a re-mesh of every chunk its light crosses as the holder walks.
- **One combined shadows setting with modes** (directional / block lights / both): the owner chose
  two settings.
- **Clustered (tiled) lighting** for T1: with at most a handful of lights near the eye, a small fixed
  list is as fast and simpler; a cluster grid can replace the list if many lights are wanted later.

## Consequences

- Unshadowed held light reaches through thin walls until Torch shadows (T2) are on.
- Every lit fragment loops over the lights drawn (none most of the time): a few ALU ops a light.
- The registry hash changes (a block added; not breaking, ADR 0038).
