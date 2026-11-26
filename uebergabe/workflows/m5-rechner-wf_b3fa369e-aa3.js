export const meta = {
  name: 'm5-rechner',
  description: 'M5 Paket E1: Zustand, Stellschrauben und reiner Tagesrechner der Planetenbelagerung mit Prognose, Tests und Werkzeug',
  phases: [
    { title: 'Umsetzen', detail: 'Zustand, Stellschrauben, Tagesrechner, Prognose, Tests 1-8, Werkzeug' },
  ],
}

const ROOT = '/home/user/exp_bevy_stellaris_fable_onePrompt'
const DIR = ROOT + '/.worktrees/belagerung'
const BRANCH = 'wip/m5-rechner'
const REF = ROOT + '/.worktrees/uebergabe/uebergabe'
const SPEC = REF + '/entwurf/M5_Synthese.md'
const SCRATCH = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/belagerung'

const COMMON = `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1).
Workspace: crates/sim = deterministic simulation core without any rendering (Bevy plugins on FixedUpdate, 20 ticks per in-game day, 360 days per year); crates/game = Bevy app; crates/headless = command line over sim::harness. Game data lives in assets/data/*.ron.
Milestones 1-3 (galaxy, economy with local prices and freighters, stock market and cohesion with secession) are finished; milestone 4 (fleets, tactical battles, technology) is being joined on another branch. You start milestone 5: the contest for a planet between a fleet in orbit and the planet's defences.

YOUR WORKING DIRECTORY (a git worktree on branch ${BRANCH}): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout ${ROOT} and the other directories under ${ROOT}/.worktrees/ contain files with the same names and belong to OTHER agents working in parallel. Never edit anything outside ${DIR} (except your scratch directory).
- You may READ the reference material in ${REF} (design documents, reports of earlier agents).
- Scratch directory for logs and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first. It explains the architecture, the determinism rules, the economy and the ownership model.

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, UI and chronicle strings, error messages, CLI help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code. No model or tool names in code, comments or documents.
2. Determinism (crates/sim): no iteration over HashMap/HashSet (use Vec or BTreeMap), nothing time- or thread-dependent, randomness only through crate::rng::Rng::stream(seed, Domain::X, a, b), sorting with total_cmp plus an id tie-break. All simulation state is plain serde data in Resources listed in the sim_state! macro in crates/sim/src/state.rs. The headless harness skips idle ticks (Headless::run_days versus run_days_exact must give the same hash).
3. Money is i64 milli-credits (type Money) and is conserved; sim::state::money_drift(world) must stay 0.
4. Player and AI act on the simulation only through SimCommand.
5. Balance levers belong in assets/data/*.ron with serde structs in crates/sim/src/data/, validated when loaded, with German error messages.
6. Tests must be honest and meaningful. Never weaken an assertion just to make it pass; find the cause.
7. Wording: neutral, abstract game-mechanics language in code, comments and texts.
8. Git: commit inside your worktree, early and often (after every self-contained step that compiles), and after each commit push exactly your own branch with: git push -u origin ${BRANCH}   (retry up to 4 times with a pause on network errors). Never push any other branch, never create pull requests, never run git stash, git reset --hard, git clean, git rebase or a checkout of another branch. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
9. Cargo: never judge success from output piped through tail or grep - redirect to a log file in the scratch directory and check the exit code ($?). Generous timeouts (up to 10 minutes; longer things in the background).
10. Format with: cargo fmt -p sim -p headless
11. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository.

REQUIRED FINAL STATE (all must succeed in ${DIR}):
  cargo build -p sim -p headless
  cargo test -p sim
  cargo clippy -p sim -p headless --all-targets -- -D warnings
  cargo fmt -p sim -p headless -- --check
  cargo build --release -p headless
  target/release/headless sweep --seeds 0..8 --years 60     (no invariant violations; with your state at neutral values the economy must behave as before)
  target/release/headless hash --seed 3 --days 3600          (twice, identical output; also identical with --exact)
Do NOT build crates/game. Keep existing public names and signatures; only add.
`

const TASK = COMMON + `
YOUR TASK: implement steps 1 and 3 of the implementation order in the specification ${SPEC} (read it completely first; it was checked against the main line, not against your branch, so verify every name it mentions against the code).

In scope
A. Step 1, the skeleton, with no change of behaviour:
   - New module crates/sim/src/siege/ with mod.rs (data and read model) and step.rs (day resolver and forecast) and plugin.rs (an empty SiegePlugin registered in SimPlugins before the AI plugin).
   - The state of section 2: Sieges (with defenses per planet, active, assign, embarked, pending, budget, plans, grievances, stats), PlanetDefense, DefensePlan, Siege, SiegeOrders, Zone, the enums Fire, ZoneKind, Posture, Outcome, and the plain input structs of the resolver (PlanetFacts, OrbitView, TechView). Sieges joins sim_state! directly after the line 'fleets: Fleets,' and is built in SimState::generate with one PlanetDefense per planet at neutral values (the start values of home worlds come in a later step).
   - SiegeBalance in crates/sim/src/data/defs.rs as the LAST field of Balance, and the block 'siege' of section 3 as the LAST block of assets/data/balance.ron, loaded and validated (key resolution of goods and technologies, rates inside (0, 1), fort >= 1, and the termination bound of section 3). The three cohesion levers (occupation, occupation_hold, war_damage) go into the cohesion balance.
   - The new fields at neutral values: PlanetEconomy.disruption; Region.besieged, occupation, garrison_cover, war_damage; the matching CohesionFactors fields with their two contributions ('Besatzung', 'Kriegsschäden') and the factor on 'Aufbruchstimmung', all without effect while the values are zero. Do NOT yet add the production hook or the risk hook of section 6 (a later step does).
   - Invariants: a function siege::check(world) -> Result<(), String> (strengths and stores finite and not negative, disruption within 0..1, vector lengths) called from sim::harness::check_invariants by exactly ONE added line directly before its final Ok(()).
   - Do not change SAVE_FORMAT (the integration does that once).
B. Step 3, the pure day resolver and the forecast: step::resolve(&mut Siege, &mut PlanetDefense, &PlanetFacts, &OrbitView, &SiegeBalance) -> DayEffects exactly as specified in section 3 (derived predicates, the seven parts of the day, zone effects, verdicts), without randomness, plus the zone construction when a record forms, and siege::forecast for the three courses of section 7 by iterating the same resolver. Everything here is plain data in, plain data out; it must not touch fleets, markets or technology state.
C. Tests 1 to 8 of section 8 as unit tests next to the resolver (hand-built inputs).
D. A tool for tuning: a subcommand 'siege' of the headless program (new file crates/headless/src/siege.rs plus the registration lines in main.rs, added at the END of the command enum and of the match) that runs the pure resolver on a scenario given by options (population, cohesion, defence levels, garrison, stores, firepower in orbit, troops aboard, standing orders, days) and prints the daily report and the three forecasts in German.
E. Tune the levers with that tool so that the three roads have the published exchange rate of section 1 for three reference planets (a colony of 50 million, a world of 500 million, a home world of 4000 million with the start values of section 3): waiting ends within the bound and leaves the prize intact, the landing takes weeks and costs troops, fire is fast and costs prize. Report the table (days, losses on both sides, prize kept) and every lever you changed against the specification, with the reason.

Out of scope (other agents, later steps): the ownership path of step 2 (transfer of a planet, the end of a community) is being built elsewhere in a different shape - do not implement transfer_planet, wind_up, conquest.rs; no orbit.rs, no daily system in the running game, no commands, no purchases on markets, no AI, nothing in crates/game. Keep the interfaces of section 2 and 7 so that those steps can build on your work.

To keep the later merge clean: besides your new files touch only crates/sim/src/lib.rs, state.rs, rng.rs (a Domain variant at the end), harness.rs (the one line), data/defs.rs, data/mod.rs, finance/mod.rs and finance/cohesion.rs (the new fields and contributions), economy/mod.rs (the one field), assets/data/balance.ron, crates/headless/src/main.rs (registration at the end). Do not touch crates/sim/src/fleet/**, battle/**, tech/**, economy/production.rs, command.rs.

FINAL REPORT (structured output): summary; measurements (the tuning table); api_notes (the public API of the module as later steps will use it: types, functions, semantics, what is deliberately missing); deviations from the specification with reasons; files_changed; checks (each required command with its result); commit; open_issues.
`

const SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    measurements: { type: 'string' },
    api_notes: { type: 'string' },
    deviations: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'measurements', 'api_notes', 'deviations', 'checks', 'commit', 'open_issues'],
}

phase('Umsetzen')
const report = await agent(TASK, { label: 'Belagerungsrechner', phase: 'Umsetzen', schema: SCHEMA })
if (!report) {
  return { error: 'Umsetzung lieferte kein Ergebnis' }
}
log('Belagerungsrechner fertig: ' + report.summary.slice(0, 300))
return { report }
