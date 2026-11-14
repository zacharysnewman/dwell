// The trailer's gameplay takes (video.json "take" shots). Args: mode, start (feet), yaw, pitch, timeOfDay, and per mode:
//   walk     the player walks toward a low camera (cam, camTo: offsets from the start; seconds; fov): the hero shot
//   dynamite first person: place a stick ahead, light it with the torch, then cut to a camera at camOff (from the
//            stick) while the player is moved away; blastMs of the fuse and blast
//   chain    a row of sticks (region fill from..to, offsets from the feet), lit at the near end, seen from camOff
//   chop     first person with the axe, a hit (Break) every hitMs for seconds on what the crosshair holds
//   npcs     villagers in a ring round the player, an orbit camera (radius, height, degrees, seconds)
//   herd     a deer herd and a wolf pack brought in beside the player, a high camera on them (cam offset), seconds
//   moon     the moon's ground (moonLand), first person looking at yaw/pitch, seconds
//   sun      the sun's crust beside its lava (sunLand river), first person, seconds
//   die      the player dies and respawns at the nearest shrine; first person after the respawn, seconds
// Posted with `globalThis.__args = {...}` prepended (take.mjs); the setup phase runs before the recording starts.
(async () => {
  const A = globalThis.__args ?? {};
  const sleep = (ms) => new Promise(r => setTimeout(r, ms));
  const yaw = A.yaw ?? 0, pitch = A.pitch ?? -5;
  const hideHud = () => {
    globalThis.__hideHud && clearInterval(globalThis.__hideHud);
    const hide = () => document.querySelectorAll('body > *:not(canvas)').forEach(e => e.style.visibility = 'hidden');
    hide(); globalThis.__hideHud = setInterval(hide, 200);
  };
  const add = (p, o) => [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
  const play = (keyframes, segments) => d.theater('play', { path: { formatVersion: 1, keyframes, segments } });
  const waitPath = async () => { await sleep(300); while (!d.theater('state', {}).finished) await sleep(100); };
  const hold = (pos, look, fov = 50, target) => play([{ t: 0, position: pos, ...(target ? { target } : { look }), fov, timeOfDay: A.timeOfDay ?? 0.3 }, { t: 600 }], [{ move: 'cut' }]);
  if (A.phase === 'setup') {
    d.theater('stop', {});
    d.releaseCamera();
    d.theater('hud', { shown: true });
    d.setTime(A.timeOfDay ?? 0.3);
    hideHud();
    d.showArms(true);
    if (A.mode === 'moon') { await d.moonLand({ face: 'a' }); await sleep(2000); d.look(yaw, pitch); return 'moon ' + JSON.stringify(d.state().feet); }
    if (A.mode === 'sun') { await d.sunLand({ face: 'a', river: true }); await sleep(2000); d.look(yaw, pitch); return 'sun ' + JSON.stringify(d.state().feet); }
    d.fly(!!A.fly);
    if (A.start) d.teleport(A.start);
    await sleep(3000);
    d.look(yaw, pitch);
    if (A.mode === 'chop') { d.pick(A.item ?? 'dwell:metal_axe'); }
    if (A.mode === 'npcs') { await d.spawnNpcs(A.count ?? 8, A.radius ?? 4, A.variant ?? 0); await sleep(2500); }
    if (A.mode === 'herd') {
      d.animalsIgnorePlayers?.(true);
      await d.spawnHerd('deer', A.deer ?? 6, A.deerOff ?? 14);
      await d.spawnHerd('wolf', A.wolves ?? 3, A.wolfOff ?? 40);
      await sleep(A.herdWarmMs ?? 4000);
    }
    if (A.mode === 'walk' || A.mode === 'npcs' || A.mode === 'herd') {
      const f = d.state().feet;
      hold(add(f, A.cam ?? [0, 0.4, -20]), null, A.fov ?? 40, add(f, A.aim ?? [0, 1.2, 0]));
      d.theater('hud', { shown: false });
      await sleep(A.warmMs ?? 6000);
    }
    await sleep(1000);
    return JSON.stringify(d.state().feet.map(v => +v.toFixed(1)));
  }
  const t0 = performance.now();
  const T = () => Math.round(performance.now() - t0);
  const log = [];
  const f0 = d.state().feet;
  if (A.mode === 'walk') {
    // the camera holds low and pushes in slowly while the player walks toward it (the hero shot)
    play([{ t: 0, position: add(f0, A.cam ?? [0, 0.4, -20]), target: 'player', fov: A.fov ?? 40, timeOfDay: A.timeOfDay ?? 0.3 },
      { t: A.seconds ?? 6, position: add(f0, A.camTo ?? [0, 0.5, -17]), target: 'player', fov: A.fovTo ?? A.fov ?? 40 }],
      [{ move: 'ease', easing: 'inOutSine' }]);
    d.look(yaw, -2);
    d.press('KeyW', true);
    await sleep((A.walkS ?? A.seconds ?? 6) * 1000);
    d.press('KeyW', false);
    await waitPath();
    await sleep(A.tailMs ?? 600);
  } else if (A.mode === 'dynamite' || A.mode === 'chain') {
    d.pick('dwell:dynamite');
    await sleep(300);
    if (A.mode === 'chain') {
      // a row of sticks: the region tool fills the box with the selected block (offsets from the feet)
      // corners as cells: x, z in metres, y in ½ m rows (the bench's region step)
      const cell = (o) => [Math.floor(f0[0] + o[0]), Math.floor((f0[1] + o[1]) / 0.5), Math.floor(f0[2] + o[2])];
      d.tool('cornerA@' + cell(A.from ?? [0, 0, 2]).join(',')); d.tool('cornerB@' + cell(A.to ?? [0, 0, 14]).join(',')); d.tool('fill');
      await sleep(600);
    } else {
      d.edit('place');
      await sleep(A.placeMs ?? 700);
    }
    log.push(T() + ' placed');
    d.pick('dwell:torch');
    await sleep(A.torchMs ?? 700);
    d.edit('place');
    log.push(T() + ' lit');
    await sleep(A.litMs ?? 1300);
    // cut to the wide camera, move the player clear, and let the fuse and the blast play
    const t = d.state().target ?? null;
    const at = add(f0, A.blastAt ?? [0, 0, 2]);
    hold(add(at, A.camOff ?? [12, 6, -10]), null, A.fov ?? 45, add(at, A.aimOff ?? [0, 1, 0]));
    d.theater('hud', { shown: false });
    log.push(T() + ' cut ' + JSON.stringify(t));
    d.teleport(add(f0, A.away ?? [-30, 2, -30]));
    await sleep(A.blastMs ?? 6000);
  } else if (A.mode === 'chop') {
    const end = (A.seconds ?? 6) * 1000;
    while (T() < end) { d.edit('break'); await sleep(A.hitMs ?? 450); }
    log.push('target ' + JSON.stringify(d.state().target));
  } else if (A.mode === 'npcs') {
    const c = add(f0, [0, A.aimY ?? 1.0, 0]);
    play([{ t: 0, position: add(f0, A.cam ?? [0, 2, -9]), target: c, fov: A.fov ?? 45, timeOfDay: A.timeOfDay ?? 0.3 },
      { t: A.seconds ?? 6, target: c }], [{ move: 'orbit', around: c, degrees: A.degrees ?? 50 }]);
    await waitPath();
  } else if (A.mode === 'herd') {
    const c = d.creatures?.()?.[0];
    log.push('creatures ' + (d.creatures?.() ?? []).length);
    play([{ t: 0, position: add(f0, A.cam ?? [0, 25, -30]), target: add(f0, A.aim ?? [20, 0, 0]), fov: A.fov ?? 50, timeOfDay: A.timeOfDay ?? 0.3 },
      { t: A.seconds ?? 8, position: add(f0, A.camTo ?? [10, 25, -30]), target: add(f0, A.aimTo ?? A.aim ?? [20, 0, 0]) }], [{ move: 'ease', easing: 'inOutSine' }]);
    await waitPath();
    log.push(c ? 'first ' + JSON.stringify(c) : 'no creature');
  } else {
    // moon, sun, die: first person, a slow turn
    if (A.mode === 'die') { d.die(); log.push(T() + ' died'); await sleep(A.respawnMs ?? 5000); hideHud(); }
    const k0 = performance.now(), s = (A.seconds ?? 4) * 1000;
    while (performance.now() - k0 < s) {
      const k = (performance.now() - k0) / s;
      d.look(yaw + (A.turn ?? 20) * k, pitch + (A.tilt ?? 0) * k);
      await sleep(16);
    }
  }
  log.push(T() + ' end feet=' + d.state().feet.map(v => v.toFixed(1)));
  return log.join(' | ');
})()
