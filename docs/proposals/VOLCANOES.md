# Proposal: active volcanoes, lava and eruptions

**Status:** proposal (owner's direction, 2026-11-11). Not architecture until an ADR adopts it. Builds on
the tectonics layer (ADR 0075 T5: dormant stratovolcanoes on a volcanic arc, hotspot islands, the
Rainier shaping, `coneHardness`), the water surface grid (ADR 0060) and explosives (docs/EXPLOSIVES.md).

## 1. The owner's direction

- "We'll add lava for active volcanoes as well. Some volcanoes are 'always on active' spewing lava (like
  in Hawaii) and some are like Mount St. Helens, active but they're just waiting to go off. Once we model
  eruptions, we could have volcanoes that did go off in the past, and are partially destroyed."
- Earlier: dormant landforms first, eruptions later (SYSTEMIC_TERRAIN §6).

## 2. Volcano states, as data per cone

| State | Real example | In game |
|---|---|---|
| Dormant | Rainier today | The cone as built (0.52.25–0.52.45): no lava. |
| Effusive, always on | Kīlauea, Hawaii | A vent (summit crater or a flank rift) feeding a lava lake and slow flows down the slope that crust over and cool to new rock, building the land. Steady, local danger. |
| Explosive, waiting | Mount St. Helens before 1980 | Quiet on the surface; signs as it builds (steam vents, tremors, a bulging flank); an eruption when a trigger fires (time, a pressure value, player action). |
| Blown | Mount St. Helens after 1980 | Generated already erupted: a summit gone, a horseshoe crater opening to one side (a lateral blast and flank collapse), a debris field and ash-grey slopes, a lava dome growing in the crater. |

Hotspot islands map to effusive; arc cones to dormant, waiting or blown.

## 3. Pieces it needs

- **Lava as a fluid:** on the water surface grid as a second fluid (thick: slow, high viscosity), or its
  own simple flow model; hot (light, damage, sets fire to wood), cools from its edges into rock
  (basalt, obsidian at the quench edge with water), steam where it meets water.
- **Vents and sources:** a volcano's vent cells as world data, fed at a rate (effusive) or released at
  once (explosive).
- **Eruptions:** an explosive eruption as an event: ash column (particles, sky darkening), pyroclastic
  flow (a fast cloud down the slope), a lateral blast that flattens trees (fells them as bodies), and
  the cone reshaped (the blown state's shape applied live). Reuses the explosives' blast for the
  mechanics where it fits.
- **Generated history:** the blown state as worldgen data on the cone (crater bearing, collapse depth,
  debris spread, ash cover), so a world can start with a volcano that went off long ago.

## 4. Phases (≤ 45 min each, sketch)

- V1: lava as a fluid on a test patch (flow, cooling to rock, light, damage).
- V2: effusive vents on hotspot cones (data), lava lakes and flows in the starter world's islands.
- V3: the blown state at worldgen (crater, collapse, debris, ash) as cone data.
- V4: explosive-waiting signs (steam vents, tremor) and a triggered eruption event.
- V5: the eruption reshaping the cone live; ash fall; pyroclastic flow.

## 5. Questions for the owner

1. **Lava's danger:** instant death on contact, heavy damage over time, or damage you can survive briefly?
2. **Effusive scale:** a lava lake in the crater with occasional flows, or continuous rivers of lava
   reaching the sea (building new land over time)?
3. **Explosive trigger:** on a timer (days), random within a window, or caused by players (dynamite,
   digging into the vent)?
4. **Eruption reach:** how destructive to players' builds near the cone (a blast that flattens
   everything within a radius, or mostly cosmetic ash and a crater change)?
5. **Server cost:** lava simulated everywhere a volcano is active, or only near players (like water
   fast-forwarded elsewhere)?
