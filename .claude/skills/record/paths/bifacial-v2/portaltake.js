(async () => {
  const cx = 1779.5, cz = 20690.5;
  const sleep = (ms) => new Promise(r => setTimeout(r, ms));
  const A = globalThis.__args ?? {};
  const camB = A.camB ?? [cx + 16, -58, cz + 16];
  const log = [];
  // Recording is started by the caller once this returns from the 'setup' phase.
  if (A.phase === 'setup') {
    d.releaseCamera();
    d.theater('hud', { shown: true });
    d.setView({ clouds: 'off' });
    d.setTime(globalThis.__time ?? 0.26);
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
  let phase = 'walk', minY = 1e9, cutAt = 0;
  while (performance.now() - t0 < (A.maxMs ?? 20000)) {
    const f = d.state().feet;
    const dist = Math.hypot(f[0] - cx, f[2] - cz);
    if (phase === 'walk') {
      const p = dist > 7 ? -4 : -4 - (7 - dist) / 6 * 76;
      d.look(globalThis.__yaw, Math.max(p, -80));
      if (dist < 1.2) { d.press('KeyW', false); phase = 'fall'; log.push(T() + ' drop'); }
    } else if (phase === 'fall') {
      d.look(globalThis.__yaw, -88);
      if (f[1] < (A.cutY ?? -3)) {
        phase = 'b'; cutAt = T(); log.push(cutAt + ' cut y=' + f[1].toFixed(1));
        d.theater('play', { path: { formatVersion: 1, keyframes: [
          { t: 0, position: camB, target: 'player', fov: A.fov ?? 60 },
          { t: 30, position: camB } ], segments: [ { move: 'pan' } ] } }); d.theater('hud', { shown: false });
      }
    } else if (phase === 'b') {
      if (f[1] < minY) minY = f[1];
      else if (f[1] > minY + 0.3) {
        // at the top of the rise on face B: fly out of the hole toward the camera, land
        phase = 'out'; log.push(T() + ' apex y=' + minY.toFixed(1));
        d.fly(true);
        await sleep(300);
        d.look(A.outYaw ?? 45, 0); d.theater('hud', { shown: false });
        d.press('KeyW', true);
        await sleep(A.outMs ?? 700);
        d.press('KeyW', false);
        await sleep(200);
        d.fly(false);
        await sleep(A.tailMs ?? 3000);
        break;
      }
    }
    await sleep(16);
  }
  d.press('KeyW', false);
  log.push(T() + ' end feet=' + d.state().feet.map(v => v.toFixed(1)));
  return log.join(' | ');
})()
