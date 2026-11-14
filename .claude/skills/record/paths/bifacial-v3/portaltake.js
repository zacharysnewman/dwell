// v3: walk to seed 7's portal, drop in (no ladder, no flight), cut past the motes to a wide camera on face B that
// keeps face A's up ("up": "a": face B and the player hang under the disc), and let the shaft's exit push (#824)
// carry the player up out of face B's hole to land beside it. Post with `globalThis.__args = {...}` prepended.
(async () => {
  const A = globalThis.__args ?? {};
  // seed 7's shaft axis (dwell_worldgen::portal_shafts_for_sized): 1779.5, 20690.5; v3's takes used axisX 1780.5
  const cx = A.axisX ?? 1779.5, cz = 20690.5;
  const sleep = (ms) => new Promise(r => setTimeout(r, ms));
  const camB = A.camB ?? [cx + 26, -76, cz + 26];
  const log = [];
  if (A.phase === 'setup') {
    d.releaseCamera();
    d.theater('hud', { shown: true });
    d.setView({ clouds: 'off' });
    d.setTime(globalThis.__time ?? 0.29);
    globalThis.__hideHud && clearInterval(globalThis.__hideHud);
    const hide = () => document.querySelectorAll('body > *:not(canvas)').forEach(e => e.style.visibility = 'hidden');
    hide(); globalThis.__hideHud = setInterval(hide, 200);
    d.showArms(true);
    const st = A.start ?? [-9, -9]; d.teleport([cx + st[0], 53, cz + st[1]]); globalThis.__yaw = A.yaw ?? 45;
    await sleep(1500);
    d.look(globalThis.__yaw, -4);
    await sleep(2500);
    return JSON.stringify(d.state().feet);
  }
  const t0 = performance.now();
  const T = () => Math.round(performance.now() - t0);
  await sleep(600);
  d.press('KeyW', true);
  let phase = 'walk', minY = 1e9, landed = 0;
  while (performance.now() - t0 < (A.maxMs ?? 20000)) {
    const s = d.state();
    const f = s.feet;
    const dist = Math.hypot(f[0] - cx, f[2] - cz);
    if (phase === 'walk') {
      const p = dist > 7 ? -4 : -4 - (7 - dist) / 6 * 76;
      d.look(globalThis.__yaw, Math.max(p, -80));
      if (dist < 1.2) { d.press('KeyW', false); phase = 'fall'; log.push(T() + ' drop'); }
    } else if (phase === 'fall') {
      if (f[1] > 0) d.look(globalThis.__yaw, -88);
      if (f[1] < (A.cutY ?? -3)) {
        phase = 'b'; log.push(T() + ' cut y=' + f[1].toFixed(1));
        if (!A.noCam) d.theater('play', { path: { formatVersion: 1, keyframes: [
          { t: 0, position: camB, target: 'player', fov: A.fov ?? 60, up: 'a' },
          { t: 30, position: camB } ], segments: [ { move: 'pan' } ] } }); d.theater('hud', { shown: false });
      }
    } else if (phase === 'b') {
      if (f[1] < minY) minY = f[1];
      else if (f[1] > minY + 0.3) { phase = 'out'; log.push(T() + ' apex y=' + minY.toFixed(1)); }
    } else if (phase === 'out') {
      // face B: the ground is below in face-local terms; a landing stops the fall back toward the midplane
      if (s.grounded && !landed) {
        landed = T(); log.push(landed + ' landed at ' + Math.hypot(f[0] - cx, f[2] - cz).toFixed(1) + ' m from the axis');
      }
      if (landed && T() - landed > (A.tailMs ?? 3000)) break;
    }
    await sleep(16);
  }
  d.press('KeyW', false);
  log.push(T() + ' end feet=' + d.state().feet.map(v => v.toFixed(1)));
  return log.join(' | ');
})()
