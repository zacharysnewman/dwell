// The water video's player takes (video.json shots s3, s5 and s6). Args: mode ("wade": first person walking in off
// the beach, the surface rising to the camera, until the head goes under; "swim": from under the water, swimming
// along the bottom, then looking up at the surface;
// "surface": the player treading water at the surface, a third-person pull-back at sunset), start (feet), yaw,
// timeOfDay, diveMs, swimMs, upMs (wade), cam/camTo/seconds (surface). Posted with `globalThis.__args = {...}`.
(async () => {
  const A = globalThis.__args ?? {};
  const sleep = (ms) => new Promise(r => setTimeout(r, ms));
  const start = A.start ?? [11660, 33, 19440];
  const yaw = A.yaw ?? 0;
  if (A.phase === 'setup') {
    d.theater('stop', {});
    d.releaseCamera();
    d.theater('hud', { shown: true });
    d.setView({ clouds: 'off' });
    d.setTime(A.timeOfDay ?? 0.55);
    globalThis.__hideHud && clearInterval(globalThis.__hideHud);
    const hide = () => document.querySelectorAll('body > *:not(canvas)').forEach(e => e.style.visibility = 'hidden');
    hide(); globalThis.__hideHud = setInterval(hide, 200);
    d.showArms(true);
    d.fly(false);
    d.teleport(start);
    await sleep(2500);
    d.look(yaw, A.pitch0 ?? -8);
    await sleep(2500);
    return JSON.stringify(d.state().feet);
  }
  const t0 = performance.now();
  const T = () => Math.round(performance.now() - t0);
  const log = [];
  if (A.mode === 'surface') {
    // the player treads water (no input); the camera pulls back and rises from behind the player toward the sun
    const f = d.state().feet;
    const cam = A.cam ?? [0, 1.2, -3], to = A.camTo ?? [0, 9, -22];
    d.theater('play', { path: { formatVersion: 1, keyframes: [
      { t: 0, position: [f[0] + cam[0], f[1] + cam[1], f[2] + cam[2]], target: 'player', fov: A.fov ?? 55, timeOfDay: A.timeOfDay ?? 0.735 },
      { t: A.seconds ?? 12, position: [f[0] + to[0], f[1] + to[1], f[2] + to[2]], target: 'player' } ],
      segments: [ { move: 'ease', easing: 'inOutSine' } ] } });
    d.theater('hud', { shown: false });
    await sleep(500);
    while (!d.theater('state', {}).finished) await sleep(100);
    await sleep(300);
    return 'surface ' + JSON.stringify(f.map(v => +v.toFixed(1)));
  }
  // wade: walk in, the pitch level; once the head is near the water's level dive (pitch down), swim along the
  // bottom, then pitch up and look at the surface while gliding
  await sleep(500);
  d.press('KeyW', true);
  // swim: C (crouch) swims down; held through the dive and the swim so buoyancy doesn't lift the player to the top
  let phase = A.mode === 'swim' ? 'dive' : 'wade', tDive = 0;
  if (A.mode === 'swim') d.press('KeyC', true);
  const level = A.level ?? 32;
  while (performance.now() - t0 < (A.maxMs ?? 30000)) {
    const f = d.state().feet;
    if (phase === 'wade') {
      d.look(yaw, -6);
      // a swimmer floats (the feet stop ~1.3 m under the level): once swimming, C takes the head under
      if (f[1] < level - (A.underM ?? 1.0)) { phase = 'dive'; tDive = T(); d.press('KeyC', true); log.push(tDive + ' dive y=' + f[1].toFixed(1)); }
    } else if (phase === 'dive' && A.mode === 'wade') {
      // the wade take ends as the head goes under (the transition and the swim are their own shots)
      d.look(yaw, -6 - Math.min(1, (T() - tDive) / 800) * 20);
      if (T() - tDive > (A.tailMs ?? 1500)) break;
    } else if (phase === 'dive') {
      const k = Math.min(1, (T() - tDive) / 800);
      d.look(yaw, -6 - k * 34);
      if (T() - tDive > (A.diveMs ?? 2200)) { phase = 'swim'; log.push(T() + ' swim y=' + f[1].toFixed(1)); }
    } else if (phase === 'swim') {
      d.look(yaw, -8);
      if (T() - tDive > (A.diveMs ?? 2200) + (A.swimMs ?? 4000)) { phase = 'up'; d.press('KeyW', false); log.push(T() + ' up y=' + f[1].toFixed(1)); }
    } else if (phase === 'up') {
      const k = Math.min(1, (T() - tDive - (A.diveMs ?? 2200) - (A.swimMs ?? 4000)) / 2000);
      d.look(yaw, -8 + k * 68);
      if (k >= 1 && T() - tDive > (A.diveMs ?? 2200) + (A.swimMs ?? 4000) + (A.upMs ?? 4500)) break;
    }
    await sleep(16);
  }
  d.press('KeyW', false); d.press('KeyC', false);
  log.push(T() + ' end feet=' + d.state().feet.map(v => v.toFixed(1)));
  return log.join(' | ');
})()
