export const meta = {
  name: 'm4-kern-merge',
  description: 'M4 Paket A1: strategische Ebene in den Kern mergen, neue Gefechts-Schnittstelle und Forschung verdrahten',
  phases: [
    { title: 'Zusammenfuehren', detail: 'Merge, Konflikte, Schnittstellen, Forschung, Tests' },
  ],
}

const ROOT = '/home/user/exp_bevy_stellaris_fable_onePrompt'
const DIR = ROOT + '/.worktrees/integ'
const BRANCH = 'wip/m4-integ'
const REF = ROOT + '/.worktrees/uebergabe/uebergabe'
const SCRATCH = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/kern'

const COMMON = `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1).
Workspace: crates/sim = deterministic simulation core without any rendering (Bevy plugins on FixedUpdate, 20 ticks per in-game day, 360 days per year); crates/game = Bevy app (map, UI, 3D battle view); crates/headless = command line over sim::harness. Game data lives in assets/data/*.ron.
Milestones 1-3 (galaxy, economy with local prices and freighters, stock market and cohesion with secession) are finished. Milestone 4 (fleets made of ships, a tactical battle calculator, technology) was built on separate branches that are now being joined.

YOUR WORKING DIRECTORY (a git worktree on branch ${BRANCH}): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout ${ROOT} and the other directories under ${ROOT}/.worktrees/ contain files with the same names and belong to OTHER agents working in parallel. Never edit anything outside ${DIR} (except your scratch directory).
- You may READ the reference material in ${REF}: berichte/*.md are the final reports of the agents who built the parts (with API notes, measurements, open issues), befunde/strategie.md holds the review findings on the strategic layer. Treat them as claims to check against the code, not as facts.
- Scratch directory for logs and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first. It explains the architecture, the determinism rules, the economy and the ownership model.

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, UI and chronicle strings, error messages, CLI help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code. No model or tool names in code, comments or documents.
2. Determinism (crates/sim): no iteration over HashMap/HashSet (use Vec or BTreeMap), nothing time- or thread-dependent, randomness only through crate::rng::Rng::stream(seed, Domain::X, a, b), sorting with total_cmp plus an id tie-break. All simulation state is plain serde data in Resources listed in the sim_state! macro in crates/sim/src/state.rs (that list defines save games and the state hash). The headless harness skips idle ticks (Headless::run_days versus run_days_exact must give the same hash).
3. Money is i64 milli-credits (type Money) and is conserved: it is only moved, never created or destroyed (the only exception is the explicitly booked minted money). sim::state::money_drift(world) must stay 0 and sim::harness::check_invariants checks it.
4. Player and AI act on the simulation only through SimCommand (crates/sim/src/command.rs).
5. Balance levers belong in assets/data/*.ron with serde structs in crates/sim/src/data/, validated when loaded.
6. Tests must be honest and meaningful. Never weaken an assertion just to make it pass; find the cause. Where a test encodes a data value that another branch changed on purpose, read the value from the game data instead of hard-coding it.
7. Wording: neutral game-mechanics language.
8. Git: commit inside your worktree, early and often (after every self-contained step that compiles), and after each commit push exactly your own branch with: git push -u origin ${BRANCH}   (retry up to 4 times with a pause on network errors). Never push any other branch, never create pull requests, never run git stash, git reset --hard, git clean, git rebase or a checkout of another branch. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
9. Cargo: never judge success from output piped through tail or grep - redirect to a log file in the scratch directory and check the exit code ($?). Use generous timeouts (up to 10 minutes per command; run longer things in the background and poll the log).
10. Format with: cargo fmt -p sim -p headless
11. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository.

REQUIRED FINAL STATE (all must succeed in ${DIR}):
  cargo build -p sim -p headless
  cargo test -p sim                      (every test binary: unit tests, battle, economy, finance, tech, military)
  cargo clippy -p sim -p headless --all-targets -- -D warnings
  cargo fmt -p sim -p headless -- --check
  cargo build --release -p headless
  target/release/headless sweep --seeds 0..8 --years 60     (no invariant violations)
  target/release/headless hash --seed 3 --days 3600          (twice, identical output; also identical with --exact)
  one run with a scripted war (the headless tool has options for it on the strategy branch) that completes without violations and in which battles begin and end
Do NOT build crates/game: it needs a full engine build and another agent adapts it afterwards. Instead, report precisely every public item of sim that crates/game uses and that you changed, removed or renamed (grep crates/game/src for the names).
`

const TASK = COMMON + `
YOUR TASK: join the strategic layer with the reworked tactical battle calculator and with technology.

Situation
- Your branch ${BRANCH} = main line + branch wip/m4-taktik (the tactical battle calculator in crates/sim/src/battle/**, reworked and rebalanced; its data in assets/data hulls.ron, modules.ron, designs.ron and the battle section of balance.ron) + branch wip/m4-tech (crates/sim/src/tech/**, research with effects, spillover, estimates). Everything on it is green.
- Branch wip/m4-sim (available locally) holds the strategic layer: fleets made of ships, shipyards with build orders, supply, repair, upkeep, the commands BuildShip, CancelBuild, MergeFleets, SplitFleet, SetFleetOrders, Retreat, and battles inside a running game (crates/sim/src/fleet/**, crates/sim/tests/military.rs, headless subcommands). It was written against the OLD battle module (as of commit d883061, the common ancestor) and without the tech module.
- The reports in ${REF}/berichte/ describe each part: gefechtsrechner_umsetzung.md and gefechtsrechner_nachbesserung.md (the new battle API: Fate, ShipResult, Battle::results, CombatantSetup::with_war_weariness, the stalemate rule, fleet retreat by loss share, and more), technologie_umsetzung.md and technologie_nachbesserung.md (Research::effects, known, add_intel, estimate, state income as budget base), strategie_1_flotten_werften.md and strategie_2_gefechte.md (the strategic layer).

Steps
1. Merge: git merge wip/m4-sim into your branch (a real merge commit, German message). A trial merge showed textual conflicts in crates/headless/src/main.rs, crates/sim/src/economy/capital.rs, crates/sim/src/economy/mod.rs, crates/sim/src/economy/production.rs and crates/sim/src/harness.rs. Resolve them by keeping the intent of BOTH sides (compare with git show wip/m4-sim:PATH, git show HEAD:PATH and the ancestor d883061). Then expect compile errors where the strategic layer uses the battle module: fix them by adapting the strategic layer to the new battle API. Do not change the behaviour of crates/sim/src/battle/** to suit the old callers; if you believe the battle module itself has a defect, describe it in your report instead of changing it (small additive helpers are fine).
2. Use the new battle API faithfully in the game-level battle code (crates/sim/src/fleet/battles.rs and neighbours): write results back to fleets from Battle::results and Fate instead of own derivations; pass war weariness through CombatantSetup::with_war_weariness; treat the stalemate outcome as its own case (nobody wins, both sides disengage). Where the new API directly answers one of the confirmed findings in ${REF}/befunde/strategie.md you may fix that finding now and name it in your report; all other findings are the next agent's job - do not start on them.
3. Wire technology into the military layer:
   - Each combatant's CombatModifiers come from Research::effects(polity) (weapon damage, weapon range, shields, armour, hull, ship speed).
   - Fleet travel speed is multiplied by the fleet_speed effect.
   - Designs: a build order may only use a design whose required technologies the community knows; an automatic choice uses GameData::best_design(class, research.known(polity)). The command rejects unknown designs with a German message.
   - Observed battles sharpen the picture of the opponent: after a battle call Research::add_intel for each pair of opposing participants, with the amount as a lever in balance.ron.
   - Research pay and fleet costs draw on the same treasury. Make the order of claims explicit and deterministic and describe it; the deeper budget question (finding 'Fleet costs leave no measurable trace in the treasury') is the next agent's job.
4. Tests. Keep every existing test of all three branches meaningful and green. Add integration tests through the game-level path (not only through the battle module): (a) with equal fleets, a community that knows weapon and defence technologies of tier 2 wins clearly more often over a set of seeds than without them; (b) a design with unknown technology cannot be ordered and becomes available once the technology is known; (c) a battle raises the participants' intel about each other; (d) determinism with a war running: same seed twice gives the same hash, run_days equals run_days_exact, saving and loading in the middle of a battle continues identically.
5. Measure and report: the sweep table (population, supply, cohesion, fleets, violations) and a scripted-war run (how many battles, how they ended, losses).

FINAL REPORT (structured output): summary; measurements; api_notes (the public API of the joined military layer and the research hooks, as other agents will use it: types, functions, commands, semantics); game_api_changes (every public sim item used by crates/game that changed, with the replacement); findings_fixed (ids from befunde/strategie.md that you fixed on the way, e.g. 'korrektheit 1'); files_changed; checks (each required command with its result); commit (hash); deviations; open_issues.
`

const SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    measurements: { type: 'string' },
    api_notes: { type: 'string' },
    game_api_changes: { type: 'string' },
    findings_fixed: { type: 'array', items: { type: 'string' } },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    deviations: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'api_notes', 'game_api_changes', 'findings_fixed', 'checks', 'commit', 'open_issues'],
}

phase('Zusammenfuehren')
const report = await agent(TASK, { label: 'Kern zusammenfuehren', phase: 'Zusammenfuehren', schema: SCHEMA })
if (!report) {
  return { error: 'Zusammenfuehrung lieferte kein Ergebnis' }
}
log('Kern zusammengefuehrt: ' + report.summary.slice(0, 300))
return { report }
