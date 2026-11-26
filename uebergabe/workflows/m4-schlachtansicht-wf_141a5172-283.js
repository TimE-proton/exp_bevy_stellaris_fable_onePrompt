export const meta = {
  name: 'm4-schlachtansicht',
  description: 'M4: 3D-Schlachtansicht mit prozeduralen Schiffen, Effekten, HUD und Gefechtsuebung; danach gegenpruefen und nachbessern',
  phases: [
    { title: 'Umsetzen', detail: 'Feed, Szene, Meshes, Kamera, Effekte, HUD, Uebungsmodus, Selbsttest' },
    { title: 'Pruefen', detail: 'drei Blickwinkel, jeder Befund wird gegengeprueft' },
    { title: 'Nachbessern', detail: 'bestaetigte Befunde beheben und committen' },
  ],
}

const DIR = '/home/user/exp_bevy_stellaris_fable_onePrompt'
const SCRATCH = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game'
const BASE = 'd883061'

const COMMON = `
You are a senior Rust/Bevy engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1, bevy_egui 0.43.1 with egui 0.36, egui_plot 0.37).
Workspace: crates/sim = deterministic simulation core without rendering; crates/game = the Bevy app (galaxy map, egui user interface, input) that only displays the core and sends it commands; crates/headless = command line. Game data in assets/data/*.ron.
Milestones 1-3 (galaxy map, economy, stock market) are finished and playable. Milestone 4 (space combat) is in progress.

YOUR WORKING DIRECTORY: ${DIR} (branch claude/keen-franklin-aqi4h7). Read README.md first.
Other agents work in parallel in the hidden directory ${DIR}/.worktrees/ - never read, search or edit anything in there. They are changing the simulation core on other branches; in YOUR tree crates/sim is frozen. You must NOT edit crates/sim, crates/headless or assets/data. If you believe the core needs a change, describe it in your final report instead.
Scratch directory for logs, screenshots and temporary files: ${SCRATCH}

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, every UI string, error messages, command line help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code.
2. The game crate only displays the simulation and sends commands; game rules never live in crates/game. Purely cosmetic randomness (debris tumbling, star field) may use any simple local generator in the game crate.
3. Use the engine API of Bevy 0.20.1 exactly as it is - do not rely on memory of older versions. docs.rs is not reachable from this machine; the authoritative sources are local: ~/.cargo/registry/src/index.crates.io-*/bevy-0.20.1/ (umbrella crate WITH the official examples in examples/), the bevy_* crates next to it (for example bevy_camera_controller-0.20.1 with the free camera, bevy_pbr, bevy_mesh, bevy_gizmos, bevy_post_process or wherever bloom lives in this version), bevy_egui-0.43.1 and egui-0.36.2. Read the relevant example or source before using an API you have not seen working in this repository. Things already established in this code base: buffered events are Message / MessageWriter / MessageReader / app.add_message; Query::single() returns a Result; egui systems run in the schedule EguiPrimaryContextPass; egui 0.36 panels need a root Ui (see crates/game/src/ui/mod.rs for the working pattern); there is a dedicated egui camera (see crates/game/src/map.rs); screenshots via Screenshot::primary_window() with save_to_disk (see crates/game/src/selftest.rs); simulation speed via the SimSpeed resource and Time<Virtual> (crates/game/src/time_control.rs).
4. Enabled engine features are listed in the root Cargo.toml (no audio, no gamepad). Avoid adding features: every change there recompiles the engine (10+ minutes). If you are certain you need one (for example for bloom or tonemapping), decide at the very beginning and do it once.
5. Tests must be honest and meaningful.
6. Wording: abstract strategy game, neutral game-mechanics language (Verluste, Gefecht, ausgefallen). No graphic descriptions.
7. Git: at the end you may commit your own work on the current branch with 'git add crates/game' (plus Cargo.toml and Cargo.lock only if you changed them) and 'git commit'. Never 'git add -A' (the tree contains other things), never push, never create pull requests, never run git stash / git reset / git clean / git checkout. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
8. Cargo: never judge success from output piped through tail/grep - redirect to a log file in the scratch directory and check the exit code ($?). A game build takes 1-3 minutes; use timeouts up to 10 minutes. Format with: cargo fmt -p game
9. Running the game on this machine: there is no display and no GPU, but Xvfb and a software Vulkan driver are installed. Run for example:
     cd ${DIR} && xvfb-run -a -s "-screen 0 1600x900x24" cargo run -q -p game -- --selftest 200 --seed 3 --screenshot ${SCRATCH}/karte.png > ${SCRATCH}/run.log 2>&1; echo exit=$?
   Software rendering is slow (a few frames per second) - that is expected; keep selftests at a few hundred frames. Run only one instance at a time. LOOK at every screenshot you produce with the Read tool and judge it critically.
10. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository.

REQUIRED FINAL STATE (all must succeed):
  cargo build -p game
  cargo test -p game
  cargo clippy -p game --all-targets -- -D warnings
  the existing campaign selftest still prints 'SELBSTTEST OK' (command in point 9)
  the new practice-battle selftests described below run, print their OK line, and their screenshots show what they should
`

const IMPLEMENT = COMMON + `
YOUR TASK: build the 3D battle view and a practice-battle mode ('Gefechtsübung').

What the game specification demands:
- Battles in 3D with a freely movable camera; the galaxy map stays a clear top-down view. Combat logic is computed in a plane, the view is 3D.
- Ships from simple procedural meshes, no external assets. Own world, own names, own ship designs - nothing that resembles existing brands or franchises.
- Visible staging: broadsides, beam and projectile weapons, capital ships breaking apart, debris fields.
- Performance for hundreds of ships and thousands of projectiles.
- Ship classes: fighter (Jäger), corvette, frigate, cruiser, battleship, carrier; ships consist of modules that can fail individually - the player must be able to see that.
- Data-heavy displays as tables and bars in egui.

THE BATTLE STATE YOU DISPLAY (read these files in crates/sim completely before designing):
  crates/sim/src/battle/mod.rs - Battle { combatants, ships, shots, beams, impacts, wrecks, tick, time, finished, timed_out, .. }, BattleShip (pos, vel, heading, class, status, side, hull/hull_max, shield/shield_max, armor[4]/armor_max, modules, ammo, fuel, morale, target, design, radius, ..), Combatant (formation, doctrine, anchor, facing, retreating, initial/lost/escaped per class, ..), Shot, Beam (BeamKind Light/Heavy/PointDefense, ttl), Impact, Wreck, ShipStatus, Formation, Doctrine, and the methods add_combatant, set_orders, order_retreat, active_by_class, fighting_value.
  crates/sim/src/battle/step.rs - Battle::step(&data) advances 0.1 s of battle time; it is called once per simulation tick (20 ticks per second at normal speed).
  crates/sim/src/battle/matchup.rs - Side::parse(&data, "speer:4,waechter:6"), matchup::build(&data, seed, &[side_a, side_b]) -> Battle, outcome_of.
  crates/sim/src/data/ships.rs - ShipClass, DesignDef/DesignStats, ModuleDef/ModuleKind, hull data; assets/data/hulls.ron, designs.ron, modules.ron for sizes and names.
  Geometry: the battle plane uses x/y with angles counter-clockwise (heading 0 points along +x, +y is to port). Field radius is about 2400 units, ship radii range from 3 (fighter) to 36 (carrier). ships only ever grows (carrier-borne fighters are appended) and indices are stable for the whole battle; wrecks only grows; shots, beams and impacts are compacted every tick (their indices are NOT stable).
  The same resolver runs every battle of the campaign, watched or not. A parallel agent is still tuning the resolver (pacing, balance); field names you see are stable, numbers are not.

ARCHITECTURE TO IMPLEMENT (it keeps the campaign hookup, which another agent does later, trivial)
New module directory crates/game/src/battle_view/ with a BattleViewPlugin, split into several files (for example feed.rs, scene.rs, meshes.rs, camera.rs, effects.rs, hud.rs, practice.rs).
1. BattleFeed (Resource): the single source of everything the view shows. It holds the battle state after the latest simulation tick (a clone of sim::battle::Battle is fine), the previous pose (position, heading) of every ship for interpolation between ticks, an epoch counter that changes whenever a different battle is shown (so the scene can rebuild), per side a display name and colour, and which side (if any) the player commands. The view systems read ONLY the feed.
2. BattleOrder (Message): orders the player gives in the view - set formation and doctrine for a side, order retreat. Whoever owns the battle consumes it.
3. Practice mode: a new top-level AppState::Practice. It owns a Battle built with matchup::build, steps it in FixedUpdate (so pause and the speed levels of time_control work exactly as in the campaign), refreshes the feed after each tick and applies BattleOrder directly (battle.set_orders / battle.order_retreat). Reachable from the main menu through a button 'Gefechtsübung' with a small setup dialog (two fleet lists with sensible presets such as a small skirmish, a line battle, carrier against battleships, a large battle with about 200 ships per side; formation and doctrine per side; seed) and from the command line: --practice, --fleet-a LISTE, --fleet-b LISTE (lists in the format of Side::parse), together with --seed. Esc returns to the main menu. After the battle ends a summary panel shows the outcome and losses per side and class and offers 'Neu starten' and 'Zurück'.
4. Campaign: ViewState::Battle (sub-state of AppState::InGame, already declared in crates/game/src/state.rs) must use the very same view. Gate the view systems on 'the feed holds a battle' rather than on one specific state, and provide enter/exit handling for both states (3D scene and battle camera on, galaxy map camera and map drawing off, and back). Nothing enters ViewState::Battle yet; make sure it will work as soon as another system fills the feed and consumes BattleOrder, and describe in your report exactly what that bridge has to do.
5. Scene. Map the plane to the 3D world with y up (x stays x, plane y goes to -z, so a heading angle is a rotation about the y axis). One render entity per ship index, created lazily, with mesh and material handles shared per class and side so the engine can batch them. Interpolate ship poses between ticks (Time<Fixed> overstep fraction). Shots, beams and impacts come from pools of reusable entities that are assigned each frame (or are drawn with another batched technique) - never spawn and despawn thousands of entities per frame and never create meshes or materials per frame.
6. Procedural ship meshes (meshes.rs): one distinctive, original silhouette per class, built in code from simple solids merged into a single mesh per class with correct normals - a small dart for the fighter, a slim fast hull with engine nacelles for the corvette, a compact escort with point-defence blisters for the frigate, a long hull with broadside sponsons for the cruiser, a massive armoured hull with heavy turrets for the battleship, a wide flat hull with hangar openings for the carrier. Bow along +x, scaled so the hull length matches the ship's radius from the data. The classes must be distinguishable at a glance. Give capital ships a fore and an aft half (two extra meshes) for the break-up effect.
7. Effects: beams as glowing stretched solids or another batched technique (thin and short-lived for light lasers, thick for heavy lances, tracer-like for point defence; a missed beam visibly passes the target - Beam.to already contains the end point); kinetic rounds as small bright elongated tracers oriented along their velocity; guided weapons larger with a short trail; impacts as expanding fading flashes sized by Impact.size; shields as a brief shimmer at the hit when the target still has shield; engine glow at the stern that scales with speed. A destroyed ship becomes a wreck: darkened hull that drifts and tumbles slowly (Wreck gives position, velocity, heading, class), capital ships break into their two halves that drift apart, with a burst of debris pieces; wrecks and debris stay as a debris field. Disabled ships (status Disabled) drift without engine glow; routing ships are recognisable (for example dimmed side colour or a marker).
8. Readability at scale: from far away ships are only a few pixels. Draw a tactical marker per ship (class-specific shape or size, side colour, drawn in screen space or as gizmos) that fades in when the ship is small on screen, so a battle of 400 ships can be read from an overview. Provide a toggleable tactical overlay (fleet anchors and facing, range rings of the field, target line and weapon range of the selected ship). A subtle reference grid or ring on the battle plane, a star field and a light source so hulls read as 3D shapes.
9. Camera: free flight in 3D (use the engine's free camera from bevy_camera_controller if it fits, otherwise write a small controller): move, look around, change speed, plus comfort functions - jump to an overview above the field, focus on the selected ship or on the centre of the fighting, follow the selected ship. Document the keys in the in-game help text (German) and avoid collisions with existing global keys (space = pause, 1-6 = speed, F1 = help, F5/F9 = quick save/load, Esc = menu).
10. Selection and inspection: click a ship (ray from the cursor onto the plane, nearest ship within its radius or marker) to select it. A panel shows name of the design and class, side, status, bars for hull, shield and the four armour faces (bow, starboard, stern, port), ammunition and fuel, morale, current target, and a table of all modules with their condition (intact, damaged, 'ausgefallen') taken from BattleShip.modules together with the design's module list.
11. HUD (egui): a compact panel per side with name, formation and doctrine, counts per class (active / lost / escaped), bars for remaining fighting value and average morale, a marker when the side is retreating; battle time and state; the time controls that already exist. For the player's side: buttons to choose formation and doctrine and to order retreat (they send BattleOrder). All texts German; formation and doctrine have label() methods in the core.
12. Selftest (extend crates/game/src/selftest.rs and args.rs): with --practice the selftest runs a practice battle instead of a campaign at high speed and writes screenshots at meaningful moments, numbering the files from the --screenshot base name: (a) an overview in the middle of the fight with both fleets, tactical markers and HUD; (b) a close view of ships exchanging fire, with beams or tracers visible; (c) late in the battle with wrecks and debris; and with --lineup a close line-up of one ship of every class side by side with labels, so the meshes can be judged. Pick the moments from the battle state (time since first contact, shots in flight, wrecks present), not from frame numbers alone. The selftest prints a final line 'SELBSTTEST OK: ...' with battle time, ships, peak number of projectiles shown and the average and worst CPU time of the view's update systems per frame in microseconds. Run it also with a large battle (about 400 ships in total) and report those numbers; the view's own CPU work should stay around or below 2 ms per frame for that size.
13. Tests in the game crate (they must run without a window): every class mesh is valid (attribute lengths agree, indices in range, no degenerate triangles, normals unit length, bounding box matches the intended size, fore and aft halves together cover the hull); the pose interpolation helper including the angle seam at +-pi; the plane picking math; the HUD and inspection panels run headless with an egui context (see the existing UI tests in crates/game/src/ui/ for the pattern) on a feed built from a stepped practice battle; pool assignment keeps entity counts bounded.

Integrate cleanly: register the plugin in crates/game/src/main.rs, extend state.rs, the main menu in crates/game/src/ui/panels.rs, the help text, args.rs. The existing galaxy map, its camera and the campaign must keep working unchanged.

When everything is green and you have looked at the screenshots, commit as described.

FINAL REPORT (structured output): summary; what the screenshots show (be honest about weaknesses) and the paths of the final screenshots; performance numbers; api_notes: BattleFeed, BattleOrder, how a campaign bridge has to fill the feed and consume orders, how to enter and leave the view from the campaign, which fields of the core's battle types the view depends on; results of the required commands; deviations; open issues; anything you would want changed in crates/sim.
`

const IMPL_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    screenshots: { type: 'string' },
    measurements: { type: 'string' },
    api_notes: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    deviations: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'screenshots', 'api_notes', 'files_changed', 'checks', 'open_issues'],
}

const FINDINGS_SCHEMA = {
  type: 'object',
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          title: { type: 'string' },
          file: { type: 'string' },
          line: { type: 'integer' },
          severity: { type: 'string', enum: ['critical', 'major', 'minor'] },
          description: { type: 'string' },
          failure_scenario: { type: 'string' },
          suggested_fix: { type: 'string' },
        },
        required: ['title', 'file', 'severity', 'description', 'failure_scenario'],
      },
    },
  },
  required: ['findings'],
}

const VERDICT_SCHEMA = {
  type: 'object',
  properties: { real: { type: 'boolean' }, reasoning: { type: 'string' }, evidence: { type: 'string' } },
  required: ['real', 'reasoning'],
}

const FIX_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    fixed: { type: 'array', items: { type: 'string' } },
    not_fixed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    screenshots: { type: 'string' },
    api_notes: { type: 'string' },
    commit: { type: 'string' },
  },
  required: ['summary', 'fixed', 'not_fixed', 'checks'],
}

const REVIEW_COMMON = `
You are reviewing code in the game project KONFLUENZ (Rust, Bevy 0.20.1, bevy_egui 0.43.1 / egui 0.36). You do NOT edit any file in the repository; you only read, run existing commands and report.
Directory: ${DIR}. Never look into ${DIR}/.worktrees/ (other agents work there). Scratch directory for your own logs and screenshots: ${SCRATCH}/review
Subject: the new 3D battle view and practice-battle mode in crates/game: see 'git diff ${BASE} --stat -- crates/game' and read crates/game/src/battle_view/ completely plus every change in the other files of crates/game. The battle state it displays is defined in crates/sim/src/battle/mod.rs (read it). README.md explains the project.
Engine API truth is in the local sources: ~/.cargo/registry/src/index.crates.io-*/bevy-0.20.1/ (with examples/), the bevy_* crates next to it, bevy_egui-0.43.1, egui-0.36.2. Do not judge API usage from memory of older versions; check the source when in doubt.
You may build and run (one game instance at a time, software rendering is slow, a few frames per second is normal):
  cargo build -p game; cargo test -p game; cargo clippy -p game --all-targets -- -D warnings
  xvfb-run -a -s "-screen 0 1600x900x24" cargo run -q -p game -- --selftest 200 --practice --screenshot ${SCRATCH}/review/x.png > ${SCRATCH}/review/run.log 2>&1; echo exit=$?
  (see crates/game/src/args.rs for all options; look at screenshots with the Read tool)
Redirect cargo output to a log file and check the exit code.
Specification for this part: battles in 3D with a freely movable camera; ships from simple procedural meshes without external assets and with original designs; visible staging of broadsides, beam and projectile weapons, capital ships breaking apart, debris fields; performance for hundreds of ships and thousands of projectiles; ship modules that fail individually must be visible to the player; the game crate only displays the simulation core and sends orders, it contains no game rules; all texts German.
Report only real, concrete problems (file, line, failure scenario). No style nitpicks, no speculation without a scenario.
`

const LENSES = [
  {
    key: 'robustheit',
    prompt: REVIEW_COMMON + `
YOUR LENS: runtime robustness and lifecycle. Look for panics and silent failures: unwrap/expect/indexing on data that changes (the feed switching to another battle with fewer ships, the feed becoming empty, ships appended during the battle, shot counts shrinking), Query::single() results, resources that do not exist in one of the two states (AppState::Practice versus ViewState::Battle inside a campaign), entities that are not despawned when leaving the view or that are despawned twice, the galaxy map camera and map systems running while the battle view is active or not coming back afterwards, input handled by both egui and the camera, state transitions (menu to practice and back, restart, practice after a campaign, campaign after practice), pause and speed levels in practice mode, the selftest paths, interpolation with NaN or across the +-pi seam, selection of a ship that has been destroyed. Run the selftests (campaign and practice) and check that they end with their OK line and exit code 0.`,
  },
  {
    key: 'engine',
    prompt: REVIEW_COMMON + `
YOUR LENS: engine usage and performance. Check against the local Bevy 0.20.1 and egui 0.36 sources that APIs are used correctly (mesh construction with attributes, indices and RenderAssetUsages; material settings for glowing unlit effects and transparency; camera, HDR, tonemapping or bloom configuration; camera ordering together with the dedicated egui camera and the 2D map camera; gizmos; schedules and system ordering between the simulation tick in FixedUpdate, the feed update and the render-side update; run conditions). Then performance for 400 ships and thousands of projectiles: per-frame allocations, cloning of the whole battle more often than once per tick, creation of meshes or materials at runtime that defeats batching, entity spawn/despawn churn, O(n^2) loops, change detection triggered every frame without need, transparency sorting cost. Run the large practice selftest and report the measured numbers of the view systems.`,
  },
  {
    key: 'anforderungen',
    prompt: REVIEW_COMMON + `
YOUR LENS: requirements and what the player actually sees. Run the practice selftests (normal, large battle, line-up) and LOOK at the screenshots with the Read tool. Judge honestly: can the six ship classes be told apart by silhouette in the line-up; do they look like original designs built from more than one box; are both fleets, side colours, tactical markers and the HUD readable in the overview; are beams, tracers, guided weapons, impacts, wrecks, debris and broken capital ships visible in the close views; is anything missing from the specification list above (free camera with comfort functions and documented keys, selection with module status table, orders for formation, doctrine and retreat, summary at the end, practice setup from the main menu and from the command line); is all text German and are there leftover placeholder or English strings; does the game crate contain game rules that belong in the core; are the tests meaningful (mesh validity, interpolation, picking, headless HUD) or could they pass while the feature is broken; is the campaign bridge (BattleFeed, BattleOrder, ViewState::Battle) really usable as the implementer describes it?`,
  },
]

phase('Umsetzen')
const impl = await agent(IMPLEMENT, { label: 'Schlachtansicht', phase: 'Umsetzen', schema: IMPL_SCHEMA })
if (!impl) {
  return { error: 'Umsetzung lieferte kein Ergebnis' }
}
log('Umsetzung fertig: ' + impl.summary.slice(0, 300))

// Die Pruefer starten das Spiel; nacheinander, damit nie zwei Instanzen zugleich rendern.
const all = []
for (const lens of LENSES) {
  const review = await agent(lens.prompt + `

What the implementer reported (claims to check, not facts):
${impl.summary}
Screenshots: ${impl.screenshots}
API notes: ${impl.api_notes}
Open issues named by the implementer: ${JSON.stringify(impl.open_issues)}`, { label: 'Pruefung: ' + lens.key, phase: 'Pruefen', schema: FINDINGS_SCHEMA })
  const findings = (review && review.findings) || []
  const judged = await parallel(findings.map((f, i) => () =>
    agent(REVIEW_COMMON + `
YOUR JOB: check one reported finding independently by READING the code and the engine sources (do not start the game; another checker may be using the display). Try to REFUTE it. Answer real=true only if you are convinced the problem exists and matters (a crash, wrong display, a leak, a clear performance problem, an engine API used wrongly, a requirement not met, a missing or meaningless required test). Answer real=false if it is wrong, purely cosmetic, or already handled elsewhere.

Finding (${f.severity}) in ${f.file}${f.line ? ':' + f.line : ''}: ${f.title}
Description: ${f.description}
Claimed failure scenario: ${f.failure_scenario}
Suggested fix: ${f.suggested_fix || '-'}`, { label: 'Gegenpruefung ' + lens.key + ' ' + (i + 1), phase: 'Pruefen', schema: VERDICT_SCHEMA })
      .then(v => ({ ...f, lens: lens.key, verdict: v }))))
  all.push(...judged.filter(Boolean))
}

const confirmed = all.filter(f => f.verdict && f.verdict.real)
log(all.length + ' Befunde, davon ' + confirmed.length + ' bestaetigt')

phase('Nachbessern')
if (confirmed.length === 0) {
  return { impl, findings: all, confirmed, fix: null }
}
const fix = await agent(COMMON + `
YOUR TASK: the 3D battle view and practice-battle mode (crates/game/src/battle_view/** and the related changes in crates/game, see 'git diff ${BASE} -- crates/game' and 'git log -3') were implemented and then reviewed. Independent checkers confirmed the findings below. Fix every one of them properly (cause, not symptom), add or strengthen tests where a finding shows a gap, run the selftests again and LOOK at the new screenshots. Edit only crates/game (and Cargo.toml only if unavoidable). Keep BattleFeed and BattleOrder stable unless a finding requires a change; report any change.

Implementer's API notes: ${impl.api_notes}

CONFIRMED FINDINGS:
${confirmed.map((f, i) => (i + 1) + '. [' + f.severity + '] ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-') + '\n   Checker: ' + (f.verdict.reasoning || '')).join('\n')}

Finish with all required commands green and a commit (git add crates/game; German message with the two trailer lines). Report what you fixed, what you could not fix and why, the paths of the final screenshots and what they show, and the final API notes for the campaign bridge.`, { label: 'Nachbesserung', phase: 'Nachbessern', schema: FIX_SCHEMA })

return { impl, findings: all, confirmed, fix }
