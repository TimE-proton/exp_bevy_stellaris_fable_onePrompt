export const meta = {
  name: 'm5-partie',
  description: 'M5 Paket F1: Belagerung in der laufenden Partie - Orbit, Tagesablauf, Befehle, Haken in Wirtschaft und Zusammenhalt, Welttests',
  phases: [
    { title: 'Orbit und Tagesablauf', detail: 'Merge des Kerns, orbit.rs, siege_day, Lebenszyklus, erste Befehle, Uebergabe' },
    { title: 'Befehle und Rueckwirkungen', detail: 'Truppen, Kapitulation, Haken, Monatsabschluss, Welttests' },
  ],
}

const ROOT = '/home/user/exp_bevy_stellaris_fable_onePrompt'
const DIR = ROOT + '/.worktrees/belagerung'
const BRANCH = 'wip/m5-rechner'
const KERN_COMMIT = '5ac904d'
const REF = ROOT + '/.worktrees/uebergabe/uebergabe'
const SPEC = REF + '/entwurf/M5_Synthese.md'
const PAD = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad'
const SCRATCH = PAD + '/belagerung'
const KERN = PAD + '/kern/a1_bericht.md'
const E1 = REF + '/berichte/belagerungsrechner_umsetzung.md'

const COMMON = `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1).
Workspace: crates/sim = deterministic simulation core without any rendering (Bevy plugins on FixedUpdate, 20 ticks per in-game day, 360 days per year); crates/game = Bevy app; crates/headless = command line over sim::harness. Game data lives in assets/data/*.ron.
Finished: galaxy, economy with local prices and freighters, stock market and cohesion with secession, fleets made of ships with shipyards and supply, a tactical battle calculator, technology. Milestone 5 is the contest for a planet between a fleet in orbit and the planet's defences; its pure day calculator with forecast and tests exists on this branch.

YOUR WORKING DIRECTORY (a git worktree on branch ${BRANCH}): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout ${ROOT} and the other directories under ${ROOT}/.worktrees/ contain files with the same names and belong to OTHER agents working in parallel. Never edit anything outside ${DIR} (except your scratch directory).
- Reference material you may READ: the specification ${SPEC} (binding where this task does not say otherwise; it was written before fleets with ships and technology existed, so check every name against the code); ${E1} (report of the engineer who built the day calculator on this branch: its API, his deviations from the specification, open issues); ${KERN} (report on the joined fleet, battle and technology layer: its API). Treat reports as claims to check against the code.
- Scratch directory for logs and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first. It explains the architecture, the determinism rules, the economy and the ownership model.

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, UI and chronicle strings, error messages, CLI help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code. No model or tool names in code, comments or documents.
2. Determinism (crates/sim): no iteration over HashMap/HashSet (use Vec or BTreeMap), nothing time- or thread-dependent, randomness only through crate::rng::Rng::stream(seed, Domain::X, a, b), sorting with total_cmp plus an id tie-break. All simulation state is plain serde data in Resources listed in the sim_state! macro in crates/sim/src/state.rs. The headless harness skips idle ticks (Headless::run_days versus run_days_exact must give the same hash). Systems inside one SimSet need an explicit order; ambiguity is an error.
3. Money is i64 milli-credits (type Money) and is conserved: only moved, never created or destroyed. sim::state::money_drift(world) must stay 0 on every day.
4. Player and AI act on the simulation only through SimCommand (crates/sim/src/command.rs); rejections are German sentences.
5. Balance levers belong in assets/data/*.ron with serde structs in crates/sim/src/data/, validated when loaded, with German error messages.
6. Tests must be honest and meaningful: world tests play real days through the harness with commands, as crates/sim/tests/economy.rs and military.rs do, and never write the state they claim to reach. Never weaken an assertion just to make it pass; find the cause.
7. Wording: neutral, abstract game-mechanics language in code, comments and texts, in the style of the existing siege module.
8. Git: commit inside your worktree, early and often (after every self-contained step that compiles), and after each commit push exactly your own branch with: git push -u origin ${BRANCH}   (retry up to 4 times with a pause on network errors). Never push any other branch, never create pull requests, never run git stash, git reset --hard, git clean, git rebase or a checkout of another branch. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
9. Cargo: never judge success from output piped through tail or grep - redirect to a log file in the scratch directory and check the exit code ($?). The machine is shared with builds that have priority: prefix every cargo command and every run of the headless tool with 'nice -n 10'. Generous timeouts (up to 10 minutes; longer things in the background, then poll the log). A full 'cargo test -p sim' can take 10 to 20 minutes - use single test binaries and filters while you work and the full run at the end.
10. Format with: cargo fmt -p sim -p headless
11. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository. Do not edit README.md (the integrator does that from your report). Do not change SAVE_FORMAT. Do not build crates/game.

WHAT OTHER ENGINEERS ARE BUILDING IN PARALLEL (do not build it; keep the seams clean)
- Branch A (fleets): fixes in crates/sim/src/fleet/{mod,battles,yard,supply,movement}.rs and an explicit monthly state budget with an ordered list of claims (planetary defence will become one more claim later). Therefore: only crates/sim/src/siege/orbit.rs may name fleet, ship or research types; touch the fleet files themselves only with the few lines the specification demands (the station veto on blockades), and do not build the purchase of defences, recruiting or their upkeep (step 5 of the specification).
- Branch B (ownership): ONE shared path for a planet changing its owner and for a community ending (crates/sim/src/polity/succession.rs with transfer_planet(planet, to, terms) and the rules for a lost capital and for a community without planets). Therefore: put the hand-over of a conquered planet behind ONE small function in crates/sim/src/siege/conquest.rs that does only what an ordinary planet needs today (owner, tax exemption, region bookkeeping, the state's shares in the local issuers, price shock - see section 6 of the specification) and is marked in a doc comment as the seam that the shared path replaces. A conquest of a CAPITAL stays in the pending list untouched until then (the siege record remains, nothing else happens); say so in the daily report of the siege.

REQUIRED FINAL STATE (all must succeed in ${DIR}, each with nice):
  cargo build -p sim -p headless
  cargo test -p sim
  cargo clippy -p sim -p headless --all-targets -- -D warnings
  cargo fmt -p sim -p headless -- --check
  cargo build --release -p headless
  target/release/headless sweep --seeds 0..8 --years 60     (no invariant violations)
  target/release/headless hash --seed 3 --days 3600          (twice, identical output; also identical with --exact)
`

const SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    api_notes: { type: 'string' },
    decisions: { type: 'string' },
    measurements: { type: 'string' },
    game_api_changes: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'api_notes', 'decisions', 'checks', 'commit', 'open_issues'],
}

phase('Orbit und Tagesablauf')
const first = await agent(COMMON + `
YOUR TASK (first of two engineers on step 4 of the specification's implementation order):
1. Merge the joined core into this branch: git merge ${KERN_COMMIT} (a real merge commit with a German message; use exactly this commit, not the moving branch). Expect conflicts at the end of assets/data/balance.ron, in crates/sim/src/state.rs, harness.rs, economy/mod.rs and crates/headless/src/main.rs; keep the intent of both sides. After the merge everything of the required final state must be green before you go on (the siege state is still neutral, so the sweep must behave like the core alone - compare the CSV without the hash column against a binary built from ${KERN_COMMIT} and explain any difference).
2. crates/sim/src/siege/orbit.rs - the only file of the module that names fleets, ships and research: build OrbitView and TechView for a siege from the real fleets and the research state (firepower from the ships' weapons without point defence, weighted by hull condition and available ammunition; magazines; berths from hull segments; toughness per class; fuel), the predicate 'control' of section 3, and the application of a day's effects to the ships (hull by class weight, ammunition, fuel, troops lost with lost berths). Use the fleet layer's own functions for hull, ammunition and fuel where they exist.
3. The start values of the home worlds (specification section 3, lever home_start) in setup_sieges, so that every community has regular units from day one.
4. The daily system siege_day in SimSet::Siege with the lifecycle of section 3 (assignments dropped, embarked troops clamped to berths, records closed through one function siege::close, records created in fleet order with stakes, militia, zones, deep reserve, garrison and prize at the start, the station veto on blockades, the mirror Region.besieged), the call of the day calculator per record in planet order with the forage purchase on the system market, and the application of its effects to ships, population, capital, issuers and production disruption. Verdicts go to the pending list.
5. resolve_conquests in SimSet::Aftermath (crates/sim/src/siege/conquest.rs) with steps 1 to 7 of the specification EXCEPT what the seam excludes: the hand-over function described above for ordinary planets, occupation values, founded day, installations and stores passing, the army becoming regular units, blockades refreshed, the record closed, a chronicle entry. Sack comes with the second engineer.
6. Commands (new SimCommand variants appended at the END of the enum, validation exactly as in section 4, German rejections): BeginSiege, SetSiegeOrders, LiftSiege, SetGarrisonOrders. And the rule that a community cannot make peace with the one that is besieging one of its planets (section 4, last paragraph).
7. Extend siege::check for the new states (defender is the owner and at war with the attacker, embarked troops within berths, assignments point at living fleets) - still one call from check_invariants.
8. World tests in a new file crates/sim/tests/siege.rs, played through the harness with commands on real seeds: test 9 of section 8 in its three variants as far as they are possible without landing troops (starving out an ordinary planet with an occupation force aboard needs embarked troops - if EmbarkTroops is required for it, implement EmbarkTroops now as well and say so; the variant with a station; the variant with a relief fleet), test 14 (peace), and the determinism part of test 16 (same seed twice, run_days against run_days_exact, saving and loading in the middle of a siege).
Commit and push after each numbered step.

FINAL REPORT (structured output): summary; api_notes (what exists now and how the second engineer and later packages use it: systems and their order, functions, commands, the seam in conquest.rs); decisions (where the specification left room or did not fit the code, with reasons); measurements (a day-by-day excerpt of one real siege from a headless run or test log: phase, stores, forces, ships); game_api_changes (public sim items used by crates/game that you changed - grep crates/game/src); files_changed; checks (each required command with its result); commit; open_issues (including everything you leave for the second engineer).
`, { label: 'Orbit und Tagesablauf', phase: 'Orbit und Tagesablauf', schema: SCHEMA })
if (!first) {
  return { error: 'Erster Schritt lieferte kein Ergebnis' }
}
log('Orbit und Tagesablauf fertig: ' + first.summary.slice(0, 300))

phase('Befehle und Rueckwirkungen')
const second = await agent(COMMON + `
YOUR TASK (second of two engineers on step 4 of the specification's implementation order). The first engineer merged the core and built orbit.rs, the daily system, the lifecycle, the hand-over of ordinary planets and the first commands. His report (claims to check against the code):
${first.summary}
API notes: ${first.api_notes}
Decisions: ${first.decisions}
Open issues he left: ${JSON.stringify(first.open_issues)}

Complete step 4:
1. The remaining commands of section 4 (appended at the END of SimCommand, validation as specified, German rejections): EmbarkTroops (unless it exists), LandTroops in both forms, Capitulate. Where EmbarkTroops buys supply, buy on the planet's market with the state's money through the existing purchase function of the economy; the monthly armament allowance of the specification comes later with the state budget - leave one clearly named place where that limit will be applied.
2. Sack after a conquest (section 3, resolve_conquests step 5), weariness and the grievances of third parties (section 3, 'Weariness'), each conserving money.
3. The hooks of section 6 that are still missing: production loses the disrupted share of its staffed capital (one line in the production code); the risk discount of issuers seated on a besieged planet; the three region values copied into the cohesion factors.
4. The bookkeeping half of the monthly system siege_month in SimSet::Aftermath: militia by the formula for planets not under siege, decay of war damage and occupation, the mirrors to the regions (occupation, garrison cover, war damage), stakes of running sieges refreshed. NOT the purchase of defences, recruiting, building levels or their upkeep (that is step 5 and waits for the state budget).
5. World tests in crates/sim/tests/siege.rs through the harness with commands on real seeds: whatever of test 9 is still missing; test 10 (bombardment leaves cohesion and market value lower and the attacker's weariness higher twelve months later); test 11 for an ordinary planet (state shares, private holders now counted as foreign, stage, the occupation contribution, the effect of a prior stake on militia and on cohesion at conquest); test 15 (disruption and recovery); the full test 16; a test that money_drift stays 0 on every day of a siege with landing, sack and conquest; and a test that a conquest of a capital stays pending and breaks nothing.
6. Measure and tune only what real games show to be broken (report a table: for three real sieges on different seeds the days, the path taken, losses on both sides, hull damage, prize kept, cohesion of the planet one and five years later); the systematic balance of the three roads is a later step with the AI.
Commit and push after each numbered step.

FINAL REPORT (structured output): summary; api_notes (the complete public API of the siege in the running game for the next packages: commands with their rules, systems and order, what the read model for the interface will need, the seam for the shared ownership path, the place for the armament allowance); decisions; measurements; game_api_changes; files_changed; checks (each required command with its result); commit; open_issues.
`, { label: 'Befehle und Rueckwirkungen', phase: 'Befehle und Rueckwirkungen', schema: SCHEMA })

return { first, second }
