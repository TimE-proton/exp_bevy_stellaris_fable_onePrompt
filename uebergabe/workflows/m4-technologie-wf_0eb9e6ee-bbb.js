export const meta = {
  name: 'm4-technologie',
  description: 'M4: Forschungsbaum mit Wirkungen, Spillover und nur teilweise sichtbarem Stand; danach gegenpruefen und nachbessern',
  phases: [
    { title: 'Umsetzen', detail: 'Forschung, Wirkungen, Befehle, Schaetzungen, Tests' },
    { title: 'Pruefen', detail: 'drei Blickwinkel, jeder Befund wird gegengeprueft' },
    { title: 'Nachbessern', detail: 'bestaetigte Befunde beheben und committen' },
  ],
}

const DIR = '/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech'
const SCRATCH = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/tech'
const BASE = 'd883061'

const COMMON = `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1).
Workspace: crates/sim = deterministic simulation core without any rendering (Bevy plugins on FixedUpdate, 20 ticks per in-game day, 360 days per year); crates/game = Bevy app (map, UI); crates/headless = command line over sim::harness. Game data lives in assets/data/*.ron.
Milestones 1-3 (galaxy, economy with local prices and freighters, stock market and cohesion with secession) are finished. Milestone 4 (space combat and technology) is in progress.

YOUR WORKING DIRECTORY (a git worktree on branch wip/m4-tech): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout /home/user/exp_bevy_stellaris_fable_onePrompt contains files with the same names and belongs to OTHER agents working in parallel. Never read, search or edit anything outside ${DIR} (except your scratch directory and the read-only cargo registry sources).
- Scratch directory for logs and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first. It explains the architecture, determinism rules, the economy and the ownership/cohesion model.

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, UI and chronicle strings, error messages, CLI help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code.
2. Determinism (crates/sim): no iteration over HashMap/HashSet (use Vec or BTreeMap), nothing time- or thread-dependent, randomness only through crate::rng::Rng::stream(seed, Domain::X, a, b) (add a Domain variant if you need one), sorting with total_cmp plus an id tie-break. All simulation state is plain serde data in Resources listed in the sim_state! macro in crates/sim/src/state.rs (that list defines save games and the state hash). The headless harness skips idle ticks (Headless::run_days versus run_days_exact must give the same hash).
3. Money is i64 milli-credits (type Money) and is conserved: it is only moved, never created or destroyed (the only exception is the explicitly booked 'minted' money). sim::state::money_drift(world) must stay 0 and sim::harness::check_invariants checks it. If you move money, use the existing helpers and keep the sum intact.
4. Player and AI act on the simulation only through SimCommand (crates/sim/src/command.rs). AI decisions that are part of the simulation run inside systems.
5. Balance levers belong in assets/data/*.ron with serde structs in crates/sim/src/data/, validated when loaded.
6. Tests must be honest and meaningful. Never weaken an assertion just to make it pass; find the cause.
7. Wording: neutral game-mechanics language.
8. Git: you may commit inside your worktree only. Never push, never create pull requests, never run git stash / git reset / git clean / git checkout of other branches. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
9. Cargo: never judge success from output piped through tail/grep - redirect to a log file in the scratch directory and check the exit code ($?). The first build in this worktree compiles the dependencies (a few minutes). Use generous timeouts (up to 10 minutes).
10. Format with: cargo fmt -p sim -p headless
11. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository.

REQUIRED FINAL STATE (all must succeed):
  cargo build -p sim
  cargo test -p sim
  cargo clippy -p sim -p headless --all-targets -- -D warnings
  cargo build --release -p headless
  target/release/headless sweep --seeds 0..8 --years 60     (no invariant violations)
  target/release/headless hash --seed 3 --days 3600          (twice, identical output; also with --exact)
Do NOT build the game crate (crates/game needs a full engine build; another agent works on it). Make sure your changes cannot break it: keep existing public names and signatures used by other crates, only add.
`

const IMPLEMENT = COMMON + `
YOUR TASK: implement technology and research in the simulation core.

What the game specification demands:
- A research tree with the branches economy, propulsion, weapons, defence and ground warfare.
- Communities research at different speeds depending on economic power, population and starting position.
- A technology lead is decisive and only partly visible to others.

What already exists (read it first): assets/data/techs.ron (30 technologies, 5 branches x 6 tiers, cost 3000 * 2^(tier-1), effects); the structs TechBranch, TechEffect, TechDef and ResearchBalance in crates/sim/src/data/ships.rs; their loading and validation in crates/sim/src/data/ship_loader.rs and data/mod.rs (GameData.techs, tech_index, best_design(class, known)); designs and modules that require technologies (designs.ron 'speer_2' needs 'waffen_2'; several modules in modules.ron); the 'research' section of assets/data/balance.ron; SimSet::Research in crates/sim/src/lib.rs (the slot in the tick order where research runs). Nothing uses any of it yet.
Also read: crates/sim/src/state.rs (sim_state!), command.rs, economy/ (production, treasuries, households), logistics/mod.rs (freighter speed), finance/ (PolityFinance.revenue = smoothed monthly state revenue in credits; plugin.rs monthly books; cohesion.rs apply_secession, where a new community appears), polity/mod.rs, diplomacy/mod.rs, harness.rs, clock.rs (month_start, day_start), rng.rs.

Your files: crates/sim/src/tech/** (new module), crates/sim/src/lib.rs (register module and plugin), crates/sim/src/state.rs, crates/sim/src/command.rs, the economy and logistics files where an effect has to hook in, crates/sim/src/finance/plugin.rs or cohesion.rs only for the secession hook, crates/sim/src/data/ships.rs ONLY the tech-related structs (TechDef, TechEffect, TechBranch, ResearchBalance), crates/sim/src/data/defs.rs and assets/data/polities.ron only if you add starting technologies, crates/sim/src/harness.rs (small additions), crates/sim/src/rng.rs (a Domain variant), crates/sim/tests/tech.rs (new), crates/headless/src/research.rs (new) plus the few lines in crates/headless/src/main.rs that register a subcommand, assets/data/techs.ron, and ONLY the 'research' section of assets/data/balance.ron.
Do NOT touch: crates/sim/src/fleet/**, crates/sim/src/battle/**, crates/game/**, assets/data/hulls.ron, modules.ron, designs.ron, other sections of balance.ron. Other agents change those right now on other branches; your branch will be merged with theirs. To keep the merge clean: in command.rs insert your SimCommand variants directly after the ClaimPlanet variant (and the match arms directly after the ClaimPlanet arm); in the sim_state! list insert your entry directly after 'finance: Finance,'.

DESIGN (names of the public API are fixed, other agents will rely on them)
Module crate::tech with:
  #[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)] pub struct Research { pub polities: Vec<PolityResearch>, /* intel matrix etc. */ }
  pub struct PolityResearch { pub known: Vec<bool>, pub progress: Vec<f64>, pub target: Option<u16>, pub auto: bool, pub budget_share: f64, pub points_last_month: f64, pub spent_last_month: Money, pub effects: TechEffects, .. }
  #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)] pub struct TechEffects { pub yield_factor: f64, pub freighter_speed: f64, pub fleet_speed: f64, pub ship_speed: f64, pub weapon_damage: f64, pub weapon_range: f64, pub shields: f64, pub armor: f64, pub hull: f64, pub ground_attack: f64, pub ground_defense: f64, pub research: f64 }   // multipliers, 1.0 = no effect; Default gives all 1.0
  impl Research {
    pub fn effects(&self, polity: PolityId) -> TechEffects      // neutral effects if the polity has no entry
    pub fn known(&self, polity: PolityId) -> &[bool]              // empty slice if no entry
    pub fn knows(&self, polity: PolityId, tech: u16) -> bool
    pub fn tier(&self, data: &GameData, polity: PolityId, branch: TechBranch) -> u8   // highest tier known in the branch, 0 = none
    pub fn available(&self, data: &GameData, polity: PolityId) -> Vec<u16>            // researchable now
    pub fn intel(&self, viewer: PolityId, target: PolityId) -> f32                    // 0..1, 1 for oneself
    pub fn add_intel(&mut self, viewer: PolityId, target: PolityId, amount: f32)      // for events such as observed battles; capped at 1
    pub fn estimate(&self, data: &GameData, viewer: PolityId, target: PolityId, seed: u64, year: u64) -> [TierEstimate; 5]
  }
  pub struct TierEstimate { pub lo: u8, pub hi: u8 }
  pub fn setup_research(data: &GameData, polities: &Polities, ..) -> Research
  a TechPlugin that runs the monthly step in SimSet::Research (run_if(month_start)), registered in SimPlugins.
Commands: SimCommand::SetResearchTarget { tech: Option<u16> } (None hands the choice back to the automatic rule) and SimCommand::SetResearchBudget { share: f64 } with validation and German error texts.

Rules:
1. Monthly research. Each living community spends budget_share of its smoothed monthly state revenue (finance.polities[p].revenue), limited by what its treasury can pay, on research. The money is the researchers' pay: it moves from the treasury to households of the community's planets (conserve money exactly; look at how the state pays households elsewhere and reuse that). Points = credits spent * efficiency * f(population) * effects.research, with a sub-linear population term (population_exponent in balance.ron). An empty treasury means less research: war and debt slow science.
2. Starting position matters in three ways: through the economy (budget), through knowledge spillover - a technology that other communities in contact already know is cheaper to research, the more so the closer those communities are in the lane network (central, well-connected communities learn faster, isolated ones slower; parameters in balance.ron) - and optionally through different starting technologies per community template (polities.ron), if you find that worthwhile.
3. Tree. A technology needs its prerequisites ('requires' in techs.ron). Progress on a technology is kept when the target changes. When a technology completes: mark it known, recompute the cached TechEffects, write a chronicle entry (German), and pick the next target if 'auto'. AI-led communities are always automatic; the player's community is automatic until the player sets a target. Automatic rule: deterministic, cost-aware, with sensible priorities (economy and propulsion in peace, weapons and defence when at war - Diplomacy::is_at_war); keep it simple and documented, a later milestone refines it with AI dispositions.
4. Effects. Aggregate the effects of all known technologies into TechEffects (additive bonuses, stored as multipliers). Wire up the two effects that belong to systems that already exist: Yield multiplies the output of all production on the community's planets, FreighterSpeed the speed of freighters of the community's trade houses. Expose everything else through Research::effects for the agents working on fleets, battles and ground combat (do not touch their files). Unlocking: GameData::best_design(class, research.known(p)) must return the better design once its technologies are known.
5. Partial visibility. Others never see the exact tree. estimate() returns per branch a range [lo, hi] that always contains the true tier; its width shrinks with intel (0..1): intel 1 gives lo = hi = truth, low intel gives a wide range (up to +-2 tiers, clamped to 0..6). Where the truth sits inside the range must come from a deterministic random stream keyed by (seed, viewer, target, branch, year), so it is stable within a year, cannot be averaged away by asking repeatedly, and is the same in every run. Intel: a base level from balance.ron, raised by economic ties between the two communities (use the ownership data in crate::finance: how much of the issuers seated on one side is held by holders of the other) and by events through add_intel; event intel decays slowly back to the level given by the ties. Keep the intel matrix in Research (serializable).
6. Secession. When a region breaks away and founds a new community (finance::cohesion::apply_secession returns its id), the new community inherits the knowledge of the community it left. All per-community vectors must always have an entry for every community; make that impossible to get wrong (for example a sync step at the start of every research step plus the explicit inheritance at secession).
7. Balance. With the default budget share a typical community should need about 3..6 years per tier-1 technology, reach tier 4..5 in a few branches around year 80..100, and nobody should finish the whole tree within 150 years. Calibrate 'efficiency' and the other parameters from measured revenues (target/release/headless economy --seed N --years 40 shows the economy; add a 'research' subcommand that prints per community and decade: revenue, research spending, points per month, known technologies per branch, current target). The Yield effect changes the economy: compare 'headless sweep --seeds 0..8 --years 100' before and after (population, supply, cohesion, invariant violations) and make sure the economy stays healthy; report both tables.

TESTS (crates/sim/tests/tech.rs, plus unit tests where more direct):
  1. Speed follows economy and population: in a controlled comparison more budget and more people give more points, sub-linearly in population; in real runs over 30 years the communities with the highest revenue know at least as many technologies as those with the lowest.
  2. Research spending conserves money (money_drift stays 0, the treasury falls by exactly what households receive) and stops at an empty treasury.
  3. Effects apply: with a Yield technology granted, a planet's output is higher by that factor than in an otherwise identical world; freighters of a community with FreighterSpeed are faster; best_design returns 'speer_2' once 'waffen_2' is known.
  4. Tree rules: prerequisites are enforced by the command, progress survives a change of target, automatic choice continues after completion, budget share is validated.
  5. Estimates: the truth is always inside [lo, hi]; intel 1 gives the exact tier; less intel never gives a narrower range; stable within a year; identical across runs.
  6. Spillover: a technology known by nearby communities is cheaper than the same technology known by nobody; distance matters.
  7. Secession: the new community inherits knowledge and all vectors stay consistent.
  8. Determinism: two runs give the same hash; run_days (idle skipping) equals run_days_exact; a save in the middle of a research project loads and continues identically.

When everything is green, commit in the worktree (git add -A; git commit) with a German message.

FINAL REPORT (structured output): summary; measurements (research pace per community from the new subcommand, sweep comparison before/after); api_notes with the exact public API (names, signatures, semantics, which effects are already wired and which are only exposed, how other subsystems should call add_intel and effects); results of the required commands; deviations; open issues.
`

const IMPL_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    measurements: { type: 'string' },
    api_notes: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
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
    api_notes: { type: 'string' },
    commit: { type: 'string' },
  },
  required: ['summary', 'fixed', 'not_fixed', 'checks'],
}

const REVIEW_COMMON = `
You are reviewing code in the game project KONFLUENZ (Rust, deterministic simulation core). You do NOT edit any file in the repository; you only read, run existing commands and report.
Directory (git worktree): ${DIR} - start every Bash command with 'cd ${DIR} &&' and use absolute paths below it. Never touch the parent checkout.
Scratch directory for your own logs: ${SCRATCH}
Subject: the new technology and research subsystem. See it with 'git diff ${BASE}' and 'git diff ${BASE} --stat'; read the new module crates/sim/src/tech/ completely, every hook it added in other files, crates/sim/tests/tech.rs, crates/headless/src/research.rs, assets/data/techs.ron and the 'research' section of assets/data/balance.ron. README.md explains the project.
You may run: cargo test -p sim; cargo build --release -p headless; target/release/headless research ...; target/release/headless sweep --seeds 0..8 --years 60; target/release/headless hash --seed 3 --days 3600 [--exact]. Redirect cargo output to a log file and check the exit code.
Project rules that matter: determinism (no HashMap iteration, randomness only via Rng::stream, state only in Resources listed in sim_state!, idle-tick skipping must not change results, save/load must continue identically); money is conserved exactly (money_drift == 0); player and AI act only through SimCommand; balance levers live in RON files; every per-community vector must have an entry for every community, including ones founded later by secession; comments and strings are German.
Specification: research tree with branches economy, propulsion, weapons, defence, ground warfare; different research speed depending on economic power, population and starting position; a technology lead is decisive and only partly visible to others.
Report only real, concrete problems (file, line, failure scenario). No style nitpicks, no speculation without a scenario.
`

const LENSES = [
  {
    key: 'korrektheit',
    prompt: REVIEW_COMMON + `
YOUR LENS: correctness, money and economic side effects. Check the monthly step line by line: where the money goes and whether the sum is conserved in every branch (rounding, empty treasury, dead communities, communities without planets); whether points, prerequisites, progress, completion and the automatic choice work in all cases (no target available, everything known, target set to an unavailable technology, division by zero with zero population or zero revenue); whether cached effects are always recomputed when knowledge changes (including after loading and after secession); whether the Yield and FreighterSpeed hooks apply to the right owner and cannot destabilise the economy (run the sweep and look at the numbers); whether estimate() can ever exclude the truth or leak the exact value at low intel; whether intel stays in 0..1 and its matrix is resized correctly when a community is added.`,
  },
  {
    key: 'determinismus',
    prompt: REVIEW_COMMON + `
YOUR LENS: determinism, state and integration. Is every piece of state that influences the future inside the Research resource and registered in sim_state! (and created in SimState::generate)? Is anything cached outside? Do save/load and replay continue identically (verify with the hash command and the tests)? Does the research system run in the right place of the tick order and only on month starts, and does idle-tick skipping still give the same hash (compare 'hash' with and without --exact)? Are per-community vectors safe when a community appears by secession in the same tick, in the tick before the monthly step, or between a command and its execution? Are the new commands validated, logged and replayable like the others? Could the changes break other crates that were not built here (crates/game uses sim's public API: look for removed or changed public items)? Are the new RON fields validated at load with clear German errors?`,
  },
  {
    key: 'anforderungen',
    prompt: REVIEW_COMMON + `
YOUR LENS: requirements, balance and honesty of the tests. Does research speed really depend on economic power, population AND starting position, and can you show it with the research subcommand over several seeds? Is the pace plausible (about 3..6 years per tier-1 technology at default budget, tier 4..5 in some branches around year 80..100, tree not finished within 150 years)? Is a technology lead only partly visible (estimates) and is the visibility model sound? Read crates/sim/tests/tech.rs critically: is each test meaningful, could it pass while the property is false, is a required property untested? Are balance levers hard-coded that belong in balance.ron? Is anything in the public API (Research, PolityResearch, TechEffects, estimate, add_intel, the two commands) missing or awkward for the agents who will build fleets, battles, ground combat and the user interface on it?`,
  },
]

phase('Umsetzen')
const impl = await agent(IMPLEMENT, { label: 'Technologie', phase: 'Umsetzen', schema: IMPL_SCHEMA })
if (!impl) {
  return { error: 'Umsetzung lieferte kein Ergebnis' }
}
log('Umsetzung fertig: ' + impl.summary.slice(0, 300))

const reviewed = await pipeline(
  LENSES,
  lens => agent(lens.prompt + `

What the implementer reported (claims to check, not facts):
${impl.summary}
Measurements claimed: ${impl.measurements}
API notes: ${impl.api_notes}
Open issues named by the implementer: ${JSON.stringify(impl.open_issues)}`, { label: 'Pruefung: ' + lens.key, phase: 'Pruefen', schema: FINDINGS_SCHEMA }),
  (review, lens) => {
    const findings = (review && review.findings) || []
    return parallel(findings.map((f, i) => () =>
      agent(REVIEW_COMMON + `
YOUR JOB: check one reported finding independently. Try to REFUTE it: read the code yourself, and where it is cheap run an existing test or the headless tool to see whether the claimed failure really happens. Answer real=true only if you are convinced the problem exists and matters (wrong results, lost or created money, nondeterminism, a crash, a requirement not met, a missing or meaningless required test). Answer real=false if it is wrong, purely cosmetic, or already handled elsewhere.

Finding (${f.severity}) in ${f.file}${f.line ? ':' + f.line : ''}: ${f.title}
Description: ${f.description}
Claimed failure scenario: ${f.failure_scenario}
Suggested fix: ${f.suggested_fix || '-'}`, { label: 'Gegenpruefung ' + lens.key + ' ' + (i + 1), phase: 'Pruefen', schema: VERDICT_SCHEMA })
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
YOUR TASK: the technology and research subsystem (crates/sim/src/tech/** and its hooks, see 'git diff ${BASE}') was implemented and then reviewed. Independent checkers confirmed the findings below. Fix every one of them properly (cause, not symptom) and add or strengthen tests where a finding shows a gap. Stay inside the files the subsystem already touches; do NOT touch crates/sim/src/fleet/**, crates/sim/src/battle/**, crates/game/**, hulls.ron, modules.ron, designs.ron or other sections of balance.ron. Keep the public API stable unless a finding requires a change; report any change.

Implementer's API notes: ${impl.api_notes}

CONFIRMED FINDINGS:
${confirmed.map((f, i) => (i + 1) + '. [' + f.severity + '] ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-') + '\n   Checker: ' + (f.verdict.reasoning || '')).join('\n')}

Finish with all required commands green and a commit in the worktree (German message with the two trailer lines). Report what you fixed, what you could not fix and why, and the final API notes.`, { label: 'Nachbesserung', phase: 'Nachbessern', schema: FIX_SCHEMA })

return { impl, findings: all, confirmed, fix }
