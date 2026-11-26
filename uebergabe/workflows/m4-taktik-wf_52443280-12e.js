export const meta = {
  name: 'm4-taktik',
  description: 'M4: taktischen Gefechtsrechner entzerren, auswuchten und testen; danach gegenpruefen und nachbessern',
  phases: [
    { title: 'Umsetzen', detail: 'Seitenverzerrung, Tempo, Moral, Rueckzug, Konter, Tests, Leistung', model: AUSWEICHMODELL },
    { title: 'Pruefen', detail: 'drei Blickwinkel, jeder Befund wird gegengeprueft', model: AUSWEICHMODELL },
    { title: 'Nachbessern', detail: 'bestaetigte Befunde beheben und committen', model: AUSWEICHMODELL },
  ],
}

const DIR = '/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/taktik'
const SCRATCH = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/taktik'
const BASE = 'd883061'

const COMMON = `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1).
Workspace: crates/sim = deterministic simulation core without any rendering (Bevy plugins on FixedUpdate, 20 ticks per in-game day); crates/game = Bevy app (map, UI); crates/headless = command line over sim::harness. Game data lives in assets/data/*.ron.
Milestones 1-3 (galaxy, economy with local prices and freighters, stock market and cohesion) are finished. Milestone 4 (space combat) is in progress.

YOUR WORKING DIRECTORY (a git worktree on branch wip/m4-taktik): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout /home/user/exp_bevy_stellaris_fable_onePrompt contains files with the same names and belongs to OTHER agents working in parallel. Never read, search or edit anything outside ${DIR} (except your scratch directory and the read-only cargo registry sources).
- Scratch directory for logs and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first.

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, UI and chronicle strings, error messages, CLI help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts in German text. Match the style of the surrounding code.
2. Determinism (crates/sim): no iteration over HashMap/HashSet (use Vec or BTreeMap), nothing time- or thread-dependent, randomness only through crate::rng::Rng::stream(seed, Domain::X, a, b), sorting with total_cmp plus an id tie-break, loops in index order. Same state + same data must give the same evolution. Everything marked #[serde(skip)] must be rebuilt deterministically before use.
3. Game data (anything that is a balance lever) belongs in assets/data/*.ron with serde structs in crates/sim/src/data/, validated when loaded - not as magic numbers in code.
4. Tests must be honest and meaningful. Never weaken an assertion just to make it pass; find the cause. No tautological tests.
5. Wording: this is an abstract strategy game. Use neutral game-mechanics language (Verluste, Gefecht, ausgefallen, Rueckzug). No graphic descriptions.
6. Git: you may commit inside your worktree only. Never push, never create pull requests, never run git stash / git reset / git clean / git checkout of other branches. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
7. Cargo: never judge success from output piped through tail/grep - redirect to a log file in the scratch directory and check the exit code ($?). The first build in this worktree compiles the dependencies (a few minutes). Use generous timeouts (up to 10 minutes) for cargo commands.
8. Format with: cargo fmt -p sim -p headless
9. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report.
10. Do not create planning or analysis documents in the repository.

REQUIRED FINAL STATE (run all four, all must succeed):
  cargo build -p sim
  cargo test -p sim
  cargo clippy -p sim -p headless --all-targets -- -D warnings
  cargo build --release -p headless
(Do not build the game crate; another agent works on it.)
`

const IMPLEMENT = COMMON + `
YOUR TASK: make the tactical battle resolver correct, unbiased, well paced and thoroughly tested.

The resolver was just written and compiles, but has only been exercised briefly. Read these files completely before changing anything:
  crates/sim/src/battle/mod.rs (state types, setup), step.rs (one tick), formation.rs, grid.rs, math.rs, matchup.rs (practice battles)
  crates/sim/src/data/ships.rs and ship_loader.rs (hulls, modules, designs, DesignStats, BattleBalance)
  assets/data/hulls.ron, modules.ron, designs.ron, techs.ron, and the sections 'military' and 'battle' of assets/data/balance.ron
  crates/headless/src/battle.rs (command line for practice battles)

Your files (edit only these): crates/sim/src/battle/**, crates/sim/src/data/ships.rs, crates/sim/src/data/ship_loader.rs, crates/sim/tests/battle.rs (new), crates/headless/src/battle.rs, assets/data/hulls.ron, assets/data/modules.ron, assets/data/designs.ron, and ONLY the 'battle' section of assets/data/balance.ron. (Another agent edits the 'research' section and the tech structs in data/ships.rs on another branch: keep your edits in data/ships.rs local to BattleBalance, WeaponDef, HullDef, ModuleDef, DesignStats.)

Tooling (100 battles run in about a second - iterate with many seeds):
  cargo build --release -p headless > LOG 2>&1
  target/release/headless battle --designs
  target/release/headless battle --a 'speer:5' --b 'speer:5' --seeds 0..12
  target/release/headless battle --a 'speer:5' --b 'speer:5' --seeds 0..1 --trace 5
  target/release/headless battle --matrix --seeds 0..8
  target/release/headless battle --bench 400
Extend this tool freely if it helps (e.g. both side assignments in the matrix, mixed fleets, win-rate summaries).

WHAT THE GAME SPECIFICATION DEMANDS OF COMBAT (all must hold when you are done)
- Large fleet battles with the ship classes fighter, corvette, frigate, cruiser, battleship, carrier. Each class has a role and counters.
- Ships consist of modules (shields, armour, hull, turrets with firing arcs and ranges, engine, reactor); individual systems can fail and that must matter.
- Formations, flanking, range control, fighter squadrons, point defence, retreat, morale and supply (ammunition, fuel) all matter.
- Counters must EMERGE from mechanics and data (size and signature, agility, turret tracking, projectile flight time, point defence, armour facing, range) and never from hard-coded class-versus-class bonuses.
- Performance for hundreds of ships and thousands of projectiles (dense arrays, spatial index).
- Deterministic and save/load-safe in the middle of a battle.

PROBLEMS I MEASURED (fix every one)
P1 Side bias. Mirror matches are not fair. 'speer:5' vs 'speer:5': side B (index 1) won 12 of 12 seeds (A lost 93 % of its value, B 60 %). In the matrix the same pairing gives different results depending on which side a fleet is on (battleships as side 0 against frigates: lose 50 % : 82 %; as side 1: lose 0 % : 100 %). Candidates: ships are processed in index order and damage is applied immediately within the tick (side 0 fires first, or its targets are chosen first); the two sides get opposite 'orbit' signs; the target-choice jitter '(index*31 + other*17) % 7'; headings near +-pi for side 1; formation mirroring. Find the real cause(s) by experiment, do not guess. One measurement from an earlier, interrupted investigation (verify it yourself): with the target-choice jitter term switched off, 'speer:5' against 'speer:5' over 300 seeds came out as 118 wins for A, 114 for B and 68 undecided - so that term, which depends on the raw ship indices, looks like the main source; the opposite orbit signs were not needed to explain it. A robust design is to make each tick's fire simultaneous (collect all damage events of the tick, apply them afterwards in a fixed order) and to make all per-ship decisions depend only on the state at the start of the tick. Required: over at least 40 seeds every mirror pairing (each class alone, and a mixed fleet) is won by side 0 in 35 %..65 % of the decided battles, with similar mean losses.
P2 Pacing. Three cruisers against three are all destroyed about 55 s after first contact and both sides are usually wiped out. Too fast and too bloody. Targets: 3 vs 3 cruisers take roughly 90..240 s of fighting after contact; 20 vs 20 mixed fleets finish in at most about 600 s; practically no battle reaches time_limit (900 s). Add a global lever in the 'battle' section of balance.ron (for example damage_scale) rather than retyping every weapon, and document it there.
P3 Morale and retreat barely matter. Hull damage costs at most 45 morale from 72, the break threshold is 18, so ships never break before they are destroyed; fleets only retreat on a strength ratio that stays near 1 in even fights (and there is a 'self.time > 25.0' hack - replace it by 'has been engaged'). Rework so that: (a) badly damaged ships on a losing side break off individually and try to leave the field; morale shocks (friendly losses nearby, hits from behind) are transient and recover, while the part caused by own hull damage does not recover; (b) a fleet also retreats when it has lost more than a doctrine-dependent share of its starting value and is not clearly winning (aggressive: late, defensive: early); (c) both sides must not order retreat in the same instant - evaluate sides one after another and apply a retreat immediately so the other side sees it; (d) leaving the field takes time: a routing ship escapes when it passes exit_radius, pursuers get shots at its weaker rear armour; consider an additional jump-out timer for ships that have been routing long enough, so slow capital ships have a chance. Target picture for an even 10 vs 10 cruiser battle over 20 seeds: the winner typically keeps 30..70 % of its value, the loser typically saves some ships (mean loser loss clearly below 100 %).
P4 Drift. With opposite orbit signs both lines run parallel in the same direction and leave the field (anchors reach y > 1000 after 85 s; ships then stick to the exit radius clamp). Engagements must stay around the middle of the field: let fleets circle the common centre (same rotational sense) or reverse direction when they approach the edge. Ships that are not routing must never pile up at the border.
P5 Class roles. Current matrix at equal build value (row vs column, 'row wins / runs, row loss % : column loss %'):
                 14xKorvette  11xFregatte  5xKreuzer  2xSchlachtschiff  2xTraeger
  Korvette        0/4 100:79   0/4 71:23    4/4 4:100   4/4 7:100         0/4 98:4
  Fregatte        4/4 9:73     4/4 82:93    0/4 100:60  0/4 100:0         4/4 0:100
  Kreuzer         0/4 100:0    4/4 25:100   0/4 95:60   3/4 75:75         4/4 0:100
  Schlachtschiff  0/4 100:7    2/4 50:82    4/4 0:95    4/4 38:100        4/4 0:88
  Traeger         4/4 5:100    0/4 100:0    0/4 100:0   3/4 71:88         0/4 18:19
Intended roles:
  - Corvette (fast torpedo boat that swings around to the rear): strong against battleships and carriers (big slow targets), weak against frigates (point defence stops torpedoes, fast lasers track it) and against cruisers (medium guns track it).
  - Frigate (escort with point defence): strong against fighters, guided weapons and corvettes; weak against cruisers and battleships.
  - Cruiser (ship of the line, all-rounder): strong against frigates and corvettes; weak against battleships.
  - Battleship: strong against cruisers and frigates; vulnerable to torpedo corvettes and to fighters when it has no escort.
  - Carrier: strikes from range with fighter squadrons; strong against battleships and other ships with little point defence; weak against frigates (point defence) and against corvettes that reach it.
  - Fighter: dangerous for capital ships, easy prey for point defence.
  Required at equal build value, over at least 12 seeds and in BOTH side assignments, the favoured side wins at least 70 % of battles: frigate > corvette, cruiser > frigate, cruiser > corvette, battleship > cruiser, battleship > frigate, corvette > battleship, corvette > carrier, carrier > battleship, frigate > carrier. Cruiser against carrier may go either way. Also show that escorts work: a battleship group with frigates does clearly better against a carrier than battleships alone at the same total value, and a balanced mixed fleet is competitive against every single-class fleet of the same value.
  Today the carrier is far too weak (its fighters achieve nothing against battleships or cruisers and it even loses to nothing but frigates' targets) and corvettes beat cruisers. Tune mechanics in step.rs and numbers in the RON files. If, after serious attempts, one relation cannot be reached without a hard-coded bonus, say so plainly in your report and let the test assert what really holds - but never fake it.
P6 Point defence scans every shot for every ready weapon (pick_aim loops over self.shots) - quadratic with thousands of projectiles. Use a spatial index for guided shots or a per-tick candidate list.
P7 Check the rest critically while you are in there: stale indices (fighters are pushed onto self.ships during a tick; Aim::Shot indices; shots.retain), division by zero and NaN (normalising zero vectors, zero hull_max, zero module hp), ships of a destroyed carrier, three-way battles, a battle whose last ships are disabled, reinforcements added in the middle of a battle (add_combatant must work at any time and keep the hostility matrix consistent), end detection.

REQUIRED API FOR THE STRATEGIC LAYER (another agent builds the campaign integration on exactly this - implement it and keep these names)
In crates/sim/src/battle/mod.rs:
  pub enum Fate { Holding, Escaped, Lost }
     Holding = still on the field when the battle ended (fighting, or adrift on the side that holds the field); Escaped = left the field; Lost = destroyed, or adrift and abandoned to the enemy.
  pub struct ShipResult { pub source: ShipId, pub side: u16, pub fate: Fate, pub hull: f32, pub modules: Vec<f32>, pub ammo: f32, pub fuel: f32, pub fighters: u32 }
     hull, ammo, fuel and each module value are fractions 0..1 of the design's maximum.
  impl Battle { pub fn results(&self, data: &GameData) -> Vec<ShipResult> }
     One entry per ship with source = Some(id), in ship index order. Surviving carrier-borne fighters (ships with source None and class Fighter that are Holding or Escaped) are counted back into the hangars of surviving carriers of their own side, in ship index order, up to each design's hangar capacity; the rest are lost. Works for a finished battle and (as a snapshot) for a running one.
Keep working: Battle::new(id, system, seed), add_combatant(data, setup, ships) -> usize (also in mid-battle), set_hostile, step(data), set_orders, order_retreat, finished, timed_out, victors(), fighting_value, active_by_class, the per-side bookkeeping in Combatant (initial, lost, escaped, initial_value, lost_value), ShipSpec, CombatantSetup, CombatModifiers, and matchup::{Side, build, fight, outcome_of, Outcome}.
IMPORTANT for the renderer: another agent is building the 3D view in parallel against the CURRENT public fields of Battle, BattleShip, Combatant, Shot, Beam, Impact and Wreck. Do not rename or remove existing pub fields and do not change their meaning (pos, vel, heading, status, class, side, hull, shield, armor, modules, ttl, kind ...). Adding fields is fine. List every public API change in your report.

TESTS (new file crates/sim/tests/battle.rs, plus unit tests next to the code where that is more direct). Use sim::GameData::load_workspace(). Keep the whole file reasonably fast (a few seconds). At least:
  1. Mirror matches are fair (P1 criteria, several classes and a mixed fleet).
  2. The counter relations listed under P5 hold at equal build value in both side assignments.
  3. Escorts protect capital ships against fighters and guided weapons (P5).
  4. Flanking pays: hits into the rear cause more hull and module damage than the same hits into the bow (direct test through the damage path), and at battle level a force that arrives in the rear or flank of an engaged enemy achieves a clearly better exchange than the same force arriving in front of it. Build the scenario so the comparison is fair and not tautological.
  5. Supply: a fleet with nearly empty magazines loses clearly against the same fleet with full magazines; weapons that need ammunition fall silent when it is gone while beam weapons keep firing.
  6. Technology: at equal build value, one tier of weapons technology (CombatModifiers damage 1.08, as 'waffen_2' in techs.ron grants) wins a clear majority of mirror battles over at least 40 seeds, and a full tier in weapons and defence wins almost always.
  7. Modules fail and it matters: lost engines reduce mobility, a lost reactor leaves a ship adrift or destroys it, damaged shield generators hold less; after a long battle some survivors carry damaged modules, and results() reports them.
  8. Morale and retreat (P3): a clearly outnumbered fleet breaks off and saves part of its ships; defensive doctrine retreats earlier than aggressive; routing ships can actually escape.
  9. Determinism: the same seed gives byte-identical state (serialize with postcard) at several checkpoints; a battle serialized in the middle (RON or postcard), deserialized and continued ends in exactly the same state as the uninterrupted one.
  10. Reinforcements joining in the middle and three-way battles end properly; no battle in the standard pairings runs into the time limit; pacing is inside the P2 windows.
  11. results(): fates, fractions in 0..1, fighters returned to carriers only up to capacity.
  12. Formations: every ship gets its own slot, no two slots coincide, for all four formations and odd fleet mixes.

PERFORMANCE: 'headless battle --bench 400' must stay well inside the budget (a tick is 50 ms of real time at normal speed; aim for a mean below 3 ms and a worst tick below 15 ms in the release build). Report the numbers.

When everything is green, commit in the worktree (git add -A; git commit) with a German message that says what changed and why.

FINAL REPORT (structured output): summary of what you changed and why; the final counter matrix and mirror results as measured; benchmark numbers; api_notes with every public item the strategic layer and the renderer need (exact names and semantics, including anything you added or changed); the result of the four required commands; deviations from this brief with reasons; open issues you could not solve.
`

const IMPL_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    measurements: { type: 'string', description: 'Kontermatrix, Spiegelgefechte, Tempo, Benchmark - wie gemessen' },
    api_notes: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string', description: 'Ergebnis der vier Pflichtbefehle' },
    commit: { type: 'string' },
    deviations: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'measurements', 'api_notes', 'files_changed', 'checks', 'open_issues'],
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
  properties: {
    real: { type: 'boolean' },
    reasoning: { type: 'string' },
    evidence: { type: 'string' },
  },
  required: ['real', 'reasoning'],
}

const FIX_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    fixed: { type: 'array', items: { type: 'string' } },
    not_fixed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    measurements: { type: 'string' },
    api_notes: { type: 'string' },
    commit: { type: 'string' },
  },
  required: ['summary', 'fixed', 'not_fixed', 'checks'],
}

const REVIEW_COMMON = `
You are reviewing code in the game project KONFLUENZ (Rust). You do NOT edit any file in the repository; you only read, run existing commands and report.
Directory (git worktree): ${DIR} - start every Bash command with 'cd ${DIR} &&' and use absolute paths below it. Never touch the parent checkout.
Scratch directory for your own logs or throwaway files: ${SCRATCH}
Subject: the tactical space battle resolver of the simulation core: crates/sim/src/battle/{mod.rs,step.rs,formation.rs,grid.rs,math.rs,matchup.rs}, crates/sim/src/data/{ships.rs,ship_loader.rs}, crates/sim/tests/battle.rs, crates/headless/src/battle.rs, assets/data/{hulls,modules,designs}.ron and the 'battle' section of assets/data/balance.ron. Read these files COMPLETELY (the whole module is new and has never been reviewed), not just the latest diff ('git diff ${BASE}' shows the most recent change).
You may build and run: cargo build --release -p headless; target/release/headless battle --matrix --seeds 0..8; ... --a 'speer:5' --b 'speer:5' --seeds 0..12 --trace 5; ... --bench 400; cargo test -p sim --test battle. Redirect cargo output to a log file and check the exit code.
Project rules that matter for the review: the simulation must be deterministic (no HashMap iteration, randomness only via Rng::stream, loops in index order, #[serde(skip)] data rebuilt before use, save/load in the middle of a battle must continue identically); balance levers belong in the RON files; class counters must emerge from mechanics, never from hard-coded class-versus-class bonuses; comments and strings are German.
Report only real, concrete problems that you can point to (file, line, failure scenario). No style nitpicks, no speculation without a scenario. If you find nothing in an area, do not invent something.
`

const LENSES = [
  {
    key: 'korrektheit',
    prompt: REVIEW_COMMON + `
YOUR LENS: correctness of the mechanics. Go through step.rs line by line and look for: sign and convention errors in angles and firing arcs (RON: 0 = bow, 90 = starboard, clockwise; math: counter-clockwise, +y is port); damage layering (shield, armour of the hit side, hull) and the 'vs' multipliers; which side of the ship is hit; cooldown, ammunition and power handling; stale indices (ships appended during a tick, shot indices, retain); division by zero and NaN sources; ships stuck at the field border; fighters without carrier; disabled ships; battles that cannot end or end too early; retreat and morale logic that cannot trigger or triggers at once for both sides; reinforcement handling in add_combatant; results() and the return of fighters to hangars; formation slot assignment; grid queries (cells outside the field, radius larger than a cell); mismatches between what the comments promise and what the code does. Also sanity-check the numbers in the RON files for obvious typos (a weapon nobody can use, a design without power, ranges that make a weapon pointless).`,
  },
  {
    key: 'determinismus',
    prompt: REVIEW_COMMON + `
YOUR LENS: determinism, state and fairness. Look for anything that makes two runs with the same seed differ or makes a battle continue differently after saving and loading in the middle: unordered collections, state that is not serialized but influences the future (check every #[serde(skip)] field and every cache), data derived at setup that is not re-derived after loading, random numbers drawn in an order that depends on something unstable, use of time or addresses. Then fairness: is there any remaining structural advantage for the side with the lower index (order of processing, order of fire and damage application within a tick, target selection, formation mirroring, spawn geometry, the orbit direction)? Verify empirically with the headless tool: run mirror pairings for every class and a mixed fleet over at least 40 seeds in both assignments and report measured win shares. Also check that the tests for determinism really compare complete state and would fail if something diverged.`,
  },
  {
    key: 'anforderungen',
    prompt: REVIEW_COMMON + `
YOUR LENS: requirements and honesty of the tests. The specification demands: classes fighter, corvette, frigate, cruiser, battleship, carrier with roles and counters; ships made of modules (shields, armour, hull, turrets with firing arcs and ranges, engine, reactor) whose individual failure matters; formations, flanking, range control, fighter squadrons, point defence, retreat, morale, supply (ammunition, fuel); performance for hundreds of ships and thousands of projectiles. Intended counters at equal build value: frigate > corvette, cruiser > frigate, cruiser > corvette, battleship > cruiser, battleship > frigate, corvette > battleship, corvette > carrier, carrier > battleship, frigate > carrier; mirror matches fair; pacing: 3 vs 3 cruisers about 90..240 s after contact, big battles below about 600 s, no time-outs; an even battle should not end in mutual annihilation and the loser should usually save some ships by retreating. Check each point against the code AND by measurement with the headless tool. Is any counter produced by a hidden class-specific bonus instead of mechanics? Then read crates/sim/tests/battle.rs critically: is each test meaningful, could it pass while the property is false (too few seeds, thresholds that are trivially met, comparing a thing with itself, asserting only that code runs)? Which required property has no test? Are there balance levers hard-coded in step.rs that belong in balance.ron? Does 'headless battle --bench 400' stay inside the budget (mean below 3 ms per tick)?`,
  },
]

phase('Umsetzen')
const impl = await agent(IMPLEMENT, { label: 'Gefechtsrechner', phase: 'Umsetzen', schema: IMPL_SCHEMA, model: AUSWEICHMODELL })
if (!impl) {
  return { error: 'Umsetzung lieferte kein Ergebnis' }
}
log('Umsetzung fertig: ' + impl.summary.slice(0, 300))

const reviewed = await pipeline(
  LENSES,
  lens => agent(lens.prompt + `

What the implementer reported (treat as claims to check, not as facts):
${impl.summary}
Measurements claimed: ${impl.measurements}
Open issues named by the implementer: ${JSON.stringify(impl.open_issues)}`, { label: 'Pruefung: ' + lens.key, phase: 'Pruefen', schema: FINDINGS_SCHEMA, model: AUSWEICHMODELL }),
  (review, lens) => {
    const findings = (review && review.findings) || []
    return parallel(findings.map((f, i) => () =>
      agent(REVIEW_COMMON + `
YOUR JOB: check one reported finding independently. Try to REFUTE it: read the code yourself, and where it is cheap run the headless tool or an existing test to see whether the claimed failure really happens. Answer real=true only if you are convinced the problem exists and matters (wrong results, unfairness, nondeterminism, a missing or meaningless required test, a requirement that is not met, a crash). Answer real=false if it is wrong, purely cosmetic, or already handled elsewhere.

Finding (${f.severity}) in ${f.file}${f.line ? ':' + f.line : ''}: ${f.title}
Description: ${f.description}
Claimed failure scenario: ${f.failure_scenario}
Suggested fix: ${f.suggested_fix || '-'}`, { label: 'Gegenpruefung ' + lens.key + ' ' + (i + 1), phase: 'Pruefen', schema: VERDICT_SCHEMA, model: AUSWEICHMODELL })
        .then(v => ({ ...f, lens: lens.key, verdict: v }))))
  },
)

const all = reviewed.filter(Boolean).flat().filter(Boolean)
const confirmed = all.filter(f => f.verdict && f.verdict.real)
log(all.length + ' Befunde, davon ' + confirmed.length + ' bestaetigt')

phase('Nachbessern')
if (confirmed.length === 0) {
  return { impl, findings: all, confirmed, fix: null }
}
const fix = await agent(COMMON + `
YOUR TASK: the tactical battle resolver (crates/sim/src/battle/**, data in assets/data/{hulls,modules,designs}.ron and the 'battle' section of balance.ron, tests in crates/sim/tests/battle.rs, tool in crates/headless/src/battle.rs) was implemented and then reviewed. Independent checkers confirmed the findings below. Fix every one of them properly (cause, not symptom), add or strengthen tests where a finding shows a gap, and re-measure balance afterwards because fixes in the mechanics shift it: the counter relations, mirror fairness, pacing and performance targets of the original brief still apply and are repeated here.

Your files: the same as the implementer's (see list above in this prompt's subject), nothing else. Keep the public API described below stable unless a finding requires a change; report any change.

Original targets: mirror pairings won by side 0 in 35..65 % of decided battles over at least 40 seeds; at equal build value over at least 12 seeds in both side assignments the favoured side wins at least 70 %: frigate > corvette, cruiser > frigate, cruiser > corvette, battleship > cruiser, battleship > frigate, corvette > battleship, corvette > carrier, carrier > battleship, frigate > carrier; counters must emerge from mechanics, no class-versus-class bonuses; 3 vs 3 cruisers about 90..240 s after contact, big battles below about 600 s, no time-outs; even battles do not end in mutual annihilation and the loser usually saves ships; bench 400 mean below 3 ms per tick. If a target cannot be met without faking, say so plainly and make the test assert what really holds.

Implementer's API notes: ${impl.api_notes}

CONFIRMED FINDINGS:
${confirmed.map((f, i) => (i + 1) + '. [' + f.severity + '] ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-') + '\n   Checker: ' + (f.verdict.reasoning || '')).join('\n')}

Finish with the four required commands green and a commit in the worktree (German message with the two trailer lines). Report what you fixed, what you could not fix and why, the final measurements (matrix, mirror, pacing, bench) and the final API notes for the strategic layer and the renderer.`, { label: 'Nachbesserung', phase: 'Nachbessern', schema: FIX_SCHEMA, model: AUSWEICHMODELL })

return { impl, findings: all, confirmed, fix }
