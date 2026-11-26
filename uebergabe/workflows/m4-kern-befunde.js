export const meta = {
  name: 'm4-kern-befunde',
  description: 'M4 Paket A2 und C-Kern: Befunde der strategischen Ebene beheben, Staatshaushalt, Militaer-KI; schlank pruefen und nachbessern',
  phases: [
    { title: 'Befunde', detail: 'offene Befunde der strategischen Ebene, Staatshaushalt' },
    { title: 'KI', detail: 'Ruestung, Forschung und Flottenfuehrung der KI, Kriegsskripte, Kennzahlen' },
    { title: 'Pruefen', detail: 'drei Blickwinkel, je eine Sammel-Gegenpruefung' },
    { title: 'Nachbessern', detail: 'bestaetigte Befunde beheben' },
  ],
}

const ROOT = '/home/user/exp_bevy_stellaris_fable_onePrompt'
const DIR = ROOT + '/.worktrees/integ'
const BRANCH = 'wip/m4-integ'
const MERGE = 'd851b0c'
const REF = ROOT + '/.worktrees/uebergabe/uebergabe'
const PAD = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad'
const SCRATCH = PAD + '/kern'
const KERN = PAD + '/kern/a1_bericht.md'

const COMMON = `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1).
Workspace: crates/sim = deterministic simulation core without any rendering (Bevy plugins on FixedUpdate, 20 ticks per in-game day, 360 days per year); crates/game = Bevy app (map, UI, 3D battle view); crates/headless = command line over sim::harness. Game data lives in assets/data/*.ron.
Milestones 1-3 (galaxy, economy with local prices and freighters, stock market and cohesion with secession) are finished. Milestone 4 (fleets made of ships with shipyards and supply, a tactical battle calculator, technology) has just been joined on this branch.

YOUR WORKING DIRECTORY (a git worktree on branch ${BRANCH}): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout ${ROOT} and the other directories under ${ROOT}/.worktrees/ contain files with the same names and belong to OTHER agents working in parallel. Never edit anything outside ${DIR} (except your scratch directory).
- Reference material you may READ: ${KERN} is the report of the engineer who joined the branches (API notes of the military layer and the research hooks, what was fixed on the way, open issues) - read it first. ${REF}/berichte/*.md are the reports of the engineers who built the parts; ${REF}/befunde/strategie.md holds the review findings on the strategic layer. Treat all of them as claims to check against the code.
- Scratch directory for logs and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first. It explains the architecture, the determinism rules, the economy and the ownership model.

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, UI and chronicle strings, error messages, CLI help, commit messages, README. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code. No model or tool names in code, comments or documents.
2. Determinism (crates/sim): no iteration over HashMap/HashSet (use Vec or BTreeMap), nothing time- or thread-dependent, randomness only through crate::rng::Rng::stream(seed, Domain::X, a, b), sorting with total_cmp plus an id tie-break. All simulation state is plain serde data in Resources listed in the sim_state! macro in crates/sim/src/state.rs (that list defines save games and the state hash). The headless harness skips idle ticks (Headless::run_days versus run_days_exact must give the same hash).
3. Money is i64 milli-credits (type Money) and is conserved: it is only moved, never created or destroyed (the only exception is the explicitly booked minted money). sim::state::money_drift(world) must stay 0 and sim::harness::check_invariants checks it.
4. Player and AI act on the simulation only through SimCommand (crates/sim/src/command.rs). The AI never reads the exact state of another community where the game offers an estimate.
5. Balance levers belong in assets/data/*.ron with serde structs in crates/sim/src/data/, validated when loaded, with German error messages.
6. Tests must be honest and meaningful: they play real days or months through the world wherever the property is about the running game, and never fabricate the state they claim to reach. Never weaken an assertion just to make it pass; find the cause.
7. Wording: neutral, abstract game-mechanics language in code, comments and texts.
8. Git: commit inside your worktree, early and often (after every self-contained step that compiles), and after each commit push exactly your own branch with: git push -u origin ${BRANCH}   (retry up to 4 times with a pause on network errors). Never push any other branch, never create pull requests, never run git stash, git reset --hard, git clean, git rebase or a checkout of another branch. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
9. Cargo: never judge success from output piped through tail or grep - redirect to a log file in the scratch directory and check the exit code ($?). Use generous timeouts (up to 10 minutes per command; run longer things in the background and poll the log). The machine is shared with other builds; a full 'cargo test -p sim' can take 10 minutes or more - use single test binaries and filters while you work and the full run at the end.
10. Format with: cargo fmt -p sim -p headless
11. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository. Keep README.md true: update the sections your work changes.
12. Other engineers work in parallel on other branches that will be merged with yours: one builds a single path for a planet changing its owner and for a community ending (new files crates/sim/src/polity/succession.rs, crates/sim/src/fleet/succession.rs, crates/sim/src/endgame.rs, a block at the START of balance.ron), one builds planetary defence (crates/sim/src/siege/**, a block at the END of balance.ron, later a claim on the state budget). Do not build anything for those topics, do not change SAVE_FORMAT, and do not build crates/game - but report precisely every public item of sim that crates/game uses and that you change (grep crates/game/src).

REQUIRED FINAL STATE (all must succeed in ${DIR}):
  cargo build -p sim -p headless
  cargo test -p sim                      (every test binary)
  cargo clippy -p sim -p headless --all-targets -- -D warnings
  cargo fmt -p sim -p headless -- --check
  cargo build --release -p headless
  target/release/headless sweep --seeds 0..8 --years 60     (no invariant violations)
  target/release/headless hash --seed 3 --days 3600          (twice, identical output; also identical with --exact)
  a sweep or run with scripted wars that completes without violations and in which battles begin and end
`

const BUDGET = `
THE STATE BUDGET (binding in its aims; you choose the mechanics after measuring, and report them)
Problem - read the finding 'anforderungen 4' with the checker's analysis in ${REF}/befunde/strategie.md and point 12 of ${REF}/berichte/technologie_nachbesserung.md: crew pay and research pay reach the households through the same function as the monthly hand-out of the treasury surplus, so spending on fleets and research only relabels a transfer. Treasuries do not react, arrears never occur, and neither player nor AI ever has to weigh a ship against anything.
Aims:
 a. One explicit monthly budget per community with a fixed, documented order of claims on the state's money: debt service; fleet (crew pay and maintenance); shipbuilding, restocking and repair as the purchases happen; research; filling the reserve; and only then the hand-out to the households. A claim that cannot be paid in full is cut, with the consequences the code already has (arrears lower morale, unpaid research advances slower, yards wait). Keep the list of claims extensible - a later package adds planetary defence as one more claim.
 b. Maintenance is real consumption, not only a transfer: fleets use up spare parts and fuel bought on the markets, also while holding station far from home, so that a fleet shows in prices and stocks and a blockade far from home runs dry without supply.
 c. Magnitudes (levers in balance.ron, set by measurement over seeds 0..8): during the first ten years the starting fleet costs a community about 8 to 12 % of its state revenue a month and research at its default share about 10 to 15 %; every further fleet of that size costs about as much again. A community that keeps four times the starting fleet in peace without raising taxes runs a deficit: the treasury falls below its reserve, the hand-out stops, and it has to borrow or falls into arrears. Research at its maximum share visibly reduces what is left for ships.
 d. The reverse holds as well: a community that spends nothing on fleet and research hands out more to its households, and that shows in their consumption - armament has a price in welfare that the player can see.
 e. A read model for interface and AI: per community the month's revenue, every claim (wanted and paid), the remainder and the hand-out, and the same figures summed over the last twelve months. Plain data; no logic left to the interface.
 f. The economy of the first three milestones stays healthy: compare population, supply, cohesion, price level and secessions of 'sweep --seeds 0..8 --years 60' before and after and report the table; no invariant violations; money_drift 0.
Prove c and d with headless runs (extend the fleets tool) and with tests that play real months: a doubled fleet lowers hand-out or treasury measurably in every one of 8 seeds; four times the starting fleet without a tax increase produces a deficit within a few years; arrears occur in a real, scripted, long war.
`

const A2 = COMMON + `
YOUR TASK: resolve every confirmed finding on the strategic layer that is still open, properly - the cause, not the symptom.
${REF}/befunde/strategie.md lists them (entries marked 'bestätigt'; entries marked 'widerlegt' are not defects). It was written against the strategy branch BEFORE the join, so file and line references are from then, and some findings may already be resolved by the new battle interface (see 'findings_fixed' and the notes on stalemates in ${KERN}). For every finding: check whether it still exists, reproduce it (a headless run or a failing test), fix it, and keep a test that fails without the fix.

Group 1 - after a battle: a forced retreat cannot be cancelled by another order and the protection while regrouping is not immunity with freedom of movement (determinismus 1, korrektheit 4, anforderungen 2); a fleet leaves a battle as soon as it has no opponent left or no ship on the field (korrektheit 2); who holds the field is decided per side, not per fleet (korrektheit 3); undecided battles never restart for ever (korrektheit 1 - check what the join did and close what is left: both sides disengage, and there is a clear rule for when the same opponents may meet again).
Group 2 - shipyards: colony yards do not flicker with noise-level capital (anforderungen 1); a build order whose yard disappears does not hang silently (korrektheit 5) - it gets an explicit state with a reason that the read model shows, and a rule for what happens to money and goods already spent; a yard on a world in a system that another community's majority holds (anforderungen 7).
Group 3 - supply and cost: holding station consumes fuel (anforderungen 3); crew pay, counts and strength exclude ships already lost in a running battle (korrektheit 6); war weariness and the threshold of a heavy defeat are not scaled by what is left of the navy (anforderungen 6).
Group 4 - the state budget (anforderungen 4 together with the open point of technology), as specified below.
Group 5 - tools and tests: no panic of the headless tools on unknown communities (determinismus 2, korrektheit 7); the headless tools can combine build orders with wars, so that a run exercises losses, rebuilding, goods and treasury together (anforderungen 8); tests on fabricated states are replaced or joined by tests through real play (anforderungen 10). Also review the tests the join had to change (listed under 'deviations', item 10, in ${KERN}): the test larger_economies_research_faster_in_a_running_game in crates/sim/tests/tech.rs now makes a weaker, seed-dependent claim - give it a sound measure (for example research points earned over the run, over several seeds) so that it states what the game guarantees.
Residuals the join left open (see its open issues): the wider reach of the protection while regrouping; three or more mutually hostile parties without enough free neighbouring systems; withdrawal decided per fleet although a sister fleet holds the field - decide each by the same principles and report.
${BUDGET}
Order: groups 1 to 3 and 5 first (commit and push after each finding), then the budget.

FINAL REPORT (structured output): summary; findings (one entry per finding id: still present or not, cause, change, test); budget (the mechanics you chose, the order of claims, every lever with its value, and the measurement tables for aims c, d and f); api_notes (what changed in the public API of the military layer and the budget read model as the AI and the interface will use it); game_api_changes (every public sim item used by crates/game that you changed, with the replacement); files_changed; checks (each required command with its result); commit; deviations; open_issues.
`

const A2_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    findings: { type: 'array', items: { type: 'string' } },
    budget: { type: 'string' },
    api_notes: { type: 'string' },
    game_api_changes: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    deviations: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'findings', 'budget', 'api_notes', 'game_api_changes', 'checks', 'commit', 'open_issues'],
}

const AI_DESIGN = `
THE BASE BEHAVIOUR OF THE AI COMMUNITIES IN MILITARY MATTERS (hidden dispositions, treaties and the decision between war and peace come with a later milestone; today wars are started by the player or by scripts, and the AI has to behave sensibly in both peace and war)
 - A new file crates/sim/src/ai/levers.rs is the ONLY AI file that names fleet, yard and research commands, so that later AI layers steer through its functions. The AI acts only through SimCommand, never for the configured player, and issues a command only when the wanted state differs from the present one. Rejected AI commands are counted (ai_applied and ai_rejected in RunSummary); target: below 2 % rejected.
 - Armament, monthly: the target share of state revenue for fleet upkeep is a lever for peace, raised with a threat measure (fleet value of other communities within some jumps of the own systems as far as the game's estimates allow, weighted higher for communities it is at war with) and multiplied by a lever at war, with a cap. It orders ships while the budget's read model leaves room and never orders what the budget cannot sustain; it replaces losses in war. Class choice: a counter table 'counter' in the military block of balance.ron (6 x 6: how well class c does against class e), derived from the battle matrix of the headless battle tool (check its real options and output, and add a way to print the table in RON form) - it picks the class with the best sum over the estimated class mix of the most threatening community and builds the best design it knows for that class (GameData::best_design). It keeps a sensible minimum mix instead of a fleet of one class, unless the table really says so.
 - Research: the AI sets focus and share (today these are fixed defaults per community); at war it shifts toward what helps its fleet, inside the budget.
 - Fleets in peace: supplied and repaired; the main fleet waits at the home system or at the own chokepoint system nearest to the strongest neighbour; small fleets are merged; trade is escorted or guarded as far as the orders allow.
 - Fleets at war: defence first - a fleet meets an intruder in its own systems when the estimated local odds are at least even (strength estimate with the counter table, or the battle forecast if one exists); otherwise it avoids the meeting and stays under supply. With local superiority (lever, default 1.3) it goes on the offensive: it seeks the opposing main fleet when it is stronger, otherwise it blockades the most populous system of the opponent that it can reach and keep supplied. It breaks off below 30 % supplies or after losing more than a lever share, and it retreats where the orders allow instead of being lost.
 - A hook for later planners: fleets reserved by another planner are left alone (a predicate that answers no today).
 - Harness and tools: scripted wars for sweeps (a sweep option like the run tool's --war, and a preset 'border war': community 0 declares war on its nearest neighbour in year 25 and offers peace in year 45). RunSummary and the CSV get columns at the END: ships built and lost, battles by outcome, months at war, months with arrears, fleet value, ai_applied, ai_rejected.
Proof by measurement, reported as tables:
 (1) peace sweep, seeds 0..8 over 60 years: AI fleets grow with the economy inside the budget (ships, upkeep share of revenue, no arrears in peace, no order hanging);
 (2) war sweep with the preset: battles happen, both sides rebuild, at least one side goes on the offensive, blockades occur, a whole fleet is lost in a hopeless meeting only rarely, rejected AI commands below 2 %;
 (3) with equal economies, a community run by this AI clearly beats one whose fleet stays passive;
 (4) determinism with AI wars: the same seed twice, run_days against run_days_exact, saving and loading in the middle of a war give equal hashes.
`

const AI_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    behaviour: { type: 'string' },
    measurements: { type: 'string' },
    api_notes: { type: 'string' },
    game_api_changes: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    deviations: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'behaviour', 'measurements', 'api_notes', 'game_api_changes', 'checks', 'commit', 'open_issues'],
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

const VERDICTS_SCHEMA = {
  type: 'object',
  properties: {
    verdicts: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          index: { type: 'integer' },
          real: { type: 'boolean' },
          severity: { type: 'string', enum: ['critical', 'major', 'minor'] },
          reasoning: { type: 'string' },
        },
        required: ['index', 'real', 'reasoning'],
      },
    },
  },
  required: ['verdicts'],
}

const FIX_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    fixed: { type: 'array', items: { type: 'string' } },
    not_fixed: { type: 'array', items: { type: 'string' } },
    measurements: { type: 'string' },
    api_notes: { type: 'string' },
    game_api_changes: { type: 'string' },
    checks: { type: 'string' },
    commit: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'fixed', 'not_fixed', 'checks', 'commit'],
}

phase('Befunde')
const a2 = await agent(A2, { label: 'Befunde und Haushalt', phase: 'Befunde', schema: A2_SCHEMA })
if (!a2) {
  return { error: 'Befunde: kein Ergebnis' }
}
log('Befunde und Haushalt fertig: ' + a2.summary.slice(0, 300))

phase('KI')
const ai = await agent(COMMON + `
YOUR TASK: build the base military behaviour of the AI communities as designed below, on top of the joined military layer and the state budget that another engineer has just finished on this branch. His report (claims to check against the code):
${a2.summary}
Budget: ${a2.budget}
API notes: ${a2.api_notes}
Open issues he named: ${JSON.stringify(a2.open_issues)}
${AI_DESIGN}
Before you design, read the existing AI (crates/sim/src/ai/**), the fleet commands and orders, the supply rules and the budget read model, and play a few scripted wars with the headless tools to see what fleets do without your code.
Commit and push after each self-contained step.

FINAL REPORT (structured output): summary; behaviour (the rules you implemented, every lever with its value, the counter table and how it was derived); measurements (the four proofs as tables); api_notes (functions of ai/levers.rs for later AI layers, new RunSummary fields, tool options); game_api_changes; files_changed; checks (each required command with its result); commit; deviations; open_issues.
`, { label: 'Militaer-KI', phase: 'KI', schema: AI_SCHEMA })
if (!ai) {
  return { a2, error: 'KI: kein Ergebnis' }
}
log('Militaer-KI fertig: ' + ai.summary.slice(0, 300))

const REVIEW_COMMON = `
You are reviewing changes in the game project KONFLUENZ (Rust 1.97.1, Bevy 0.20.1; deterministic simulation core in crates/sim, command line in crates/headless). You do NOT edit any file in the repository; you read, build, run and report.
Directory (git worktree on branch ${BRANCH}): ${DIR} - start every Bash command with 'cd ${DIR} &&' and use absolute paths below it. Never touch the parent checkout or other worktrees.
Scratch directory for your own logs and throw-away files: ${SCRATCH}/pruefung (to try a failing scenario, write a throw-away test, run it by copying it into crates/sim/tests under a name starting with 'tmp_', and delete it again; the working tree must be clean when you finish - check with git status).
Subject: the military layer of the game as it stands now - fleets, shipyards, supply, upkeep, battles inside a running game, technology hooks, the state budget and the AI's military behaviour. It was joined from several branches at merge commit ${MERGE}; three engineers then worked on it: the join itself (report: ${KERN}), the open review findings plus the state budget, and the AI. See 'git log --oneline ${MERGE}..HEAD' and 'git diff ${MERGE}'. The tactical battle calculator in crates/sim/src/battle/** and the technology module were reviewed separately before and are NOT your subject, only how they are used.
The earlier review findings on the strategic layer are in ${REF}/befunde/strategie.md (the ones marked 'bestätigt' had to be resolved). README.md explains the project and must be true.
Project rules that matter: determinism (no HashMap iteration, randomness only through Rng::stream, state only in the sim_state! resources, run_days equals run_days_exact, save and load continue identically), money is conserved (sim::state::money_drift == 0), player and AI act only through SimCommand, the AI does not read exact foreign state where an estimate exists, tests must play the real game and be able to fail.
Cargo: redirect output to a log file and check the exit code; the machine is shared - prefer single test binaries, filters and the release headless tool (target/release/headless; build it with cargo build --release -p headless) over full test runs.
Report only real, concrete problems with a location and a failure scenario you observed or can derive step by step: wrong results, money created or lost, a rule that can be circumvented, an order that hangs, non-determinism, a requirement that is not met, a measurement in the reports that does not reproduce, a test that would pass while the property is false, README statements that are false. No style remarks, no wishes.

What the engineers reported (claims to check, not facts):
FINDINGS AND BUDGET: ${a2.summary}
Per finding: ${a2.findings.join(' | ')}
Budget: ${a2.budget}
AI: ${ai.summary}
Behaviour: ${ai.behaviour}
Measurements: ${ai.measurements}
Open issues named: ${JSON.stringify((a2.open_issues || []).concat(ai.open_issues || []))}
`

const LENSES = [
  {
    key: 'korrektheit',
    prompt: `
YOUR LENS: correctness of the rules and the flow of money and goods. Go through the life of a fleet in a running game - order, purchase of parts, construction, commissioning, movement, supply, upkeep, battle, retreat, regrouping, repair, loss - and through the monthly state budget, and try to break each rule: cancel a forced retreat by any sequence of orders; gain protection or free movement from regrouping; keep a blockade for ever without supply; make an order hang or lose its money silently (yard disappears, owner short of money, goods missing, design unknown); pay or count ships that no longer exist; get crew pay, research pay or hand-outs paid twice or not at all when the treasury is short; make the budget's claims exceed the money or leave money unaccounted (follow one month with numbers taken from a real run and add them up); find an order of claims that differs from the documented one. Check every one of the earlier confirmed findings against the present code: resolved by cause or only at the reported spot?`,
  },
  {
    key: 'determinismus',
    prompt: `
YOUR LENS: determinism, saving and loading, integration and robustness. Hash equality with wars and AI running: the same seed twice, run_days against run_days_exact, save and load at awkward moments (in the middle of a battle, on the day of the monthly budget, while a retreat or a build order is in progress) - run these with the headless tools and with throw-away tests, on several seeds. Look for iteration over unordered containers, floating-point sums whose order depends on insertion history, state kept outside the sim_state! resources (caches, Local, statics) that changes behaviour, and systems whose order is not fixed. Is the wiring of technology into fleets and battles complete and consistent (modifiers for both sides, speed, availability of designs, insight after battles)? Do the headless tools fail cleanly on wrong input? How long do 'sweep --seeds 0..8 --years 60' and a war sweep take now compared with the numbers in the reports - is anything quadratic in the number of ships or fleets? Run the complete required final state of the project yourself (build, the full cargo test -p sim, clippy with -D warnings, fmt --check, sweep, hash) and report every command that does not succeed.`,
  },
  {
    key: 'anforderungen',
    prompt: `
YOUR LENS: requirements, behaviour and the honesty of tests and measurements. Requirements of this milestone: war costs feed back into economy and cohesion; fleets need supply; a visible but only partly known technological lead matters in battle; the AI communities arm, research and handle their fleets sensibly in peace and war; hundreds of ships are possible. Reproduce the central measurements of the reports with the release headless tool on seeds of your own choice (not only the ones in the reports): does a larger fleet really cost the state and the households something a player would notice; do arrears occur in a long war; does the AI build within its budget, rebuild after losses, go on the offensive with superiority and avoid hopeless meetings; are rejected AI commands rare; does a technological lead change outcomes through the game-level path? Watch one scripted war day by day in the chronicle and the tool output and report behaviour a player would find absurd (fleets idling next to a blockade of their home, building ships they cannot supply, endless back and forth). Read the new and changed tests critically: do they play the real game or fabricate the state they claim to reach; could each pass while its property is false? Is README.md accurate about the military layer, the budget, the tools and their options?`,
  },
]

const reviewed = await pipeline(
  LENSES,
  lens => agent(REVIEW_COMMON + lens.prompt, { label: 'Pruefung: ' + lens.key, phase: 'Pruefen', schema: FINDINGS_SCHEMA }),
  (review, lens) => {
    const findings = (review && review.findings) || []
    if (findings.length === 0) {
      return []
    }
    return agent(REVIEW_COMMON + `
YOUR JOB: check the findings of one reviewer independently. For EACH finding try to REFUTE it: read the code yourself and, where it is cheap, reproduce the claimed failure with the headless tool or a throw-away test. Give real=true only if you are convinced the problem exists and matters (wrong result, money created or lost, a rule that can be circumvented, a hanging order, non-determinism, an unmet requirement, a measurement that does not reproduce, a test that cannot fail, a false README statement). Give real=false if it is wrong, purely cosmetic, a wish, or already handled. Spend your effort in proportion to the severity. Return one verdict per finding with the finding's index (starting at 1), your own severity and a short reasoning that names what you checked.

FINDINGS OF THE REVIEWER:
${findings.map((f, i) => (i + 1) + '. [' + f.severity + '] ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-')).join('\n')}`, { label: 'Gegenpruefung ' + lens.key, phase: 'Pruefen', schema: VERDICTS_SCHEMA })
      .then(result => {
        const verdicts = (result && result.verdicts) || []
        return findings.map((f, i) => {
          const verdict = verdicts.find(v => v.index === i + 1)
          return { ...f, lens: lens.key, verdict: verdict || null }
        })
      })
  },
)

const all = reviewed.filter(Boolean).flat().filter(Boolean)
// Ohne Urteil (Gegenpruefer ausgefallen) gilt ein Befund als offen: der Nachbesserer prueft ihn selbst.
const confirmed = all.filter(f => !f.verdict || f.verdict.real)
log(all.length + ' Befunde, davon ' + confirmed.length + ' bestaetigt oder ungeprueft')

phase('Nachbessern')
if (confirmed.length === 0) {
  return { a2, ai, findings: all, confirmed, fix: null }
}
const fix = await agent(COMMON + `
YOUR TASK: the military layer of this branch was completed by two engineers (open review findings and the state budget; then the AI's military behaviour) and reviewed afterwards. The findings below were confirmed by an independent checker (or could not be checked - then verify them yourself first). Fix every real one properly - the cause, not the symptom - with a test that fails without the fix wherever a test can express it. Where a finding is about balance or behaviour, change levers or rules and show the effect with the headless tools on several seeds. If a finding is wrong after all, say why instead of changing code. Keep everything else working; commit and push after each group of fixes; then prove the required final state again and repeat the central measurements (peace sweep and war sweep tables).

What the engineers reported:
FINDINGS AND BUDGET: ${a2.summary}
Budget: ${a2.budget}
API: ${a2.api_notes}
AI: ${ai.summary}
Behaviour: ${ai.behaviour}
API: ${ai.api_notes}
${BUDGET}
${AI_DESIGN}

CONFIRMED FINDINGS:
${confirmed.map((f, i) => (i + 1) + '. [' + ((f.verdict && f.verdict.severity) || f.severity) + '] (' + f.lens + ') ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-') + '\n   Checker: ' + (f.verdict ? f.verdict.reasoning : 'not checked')).join('\n')}

FINAL REPORT (structured output): summary; fixed (one entry per finding: cause, change, proof); not_fixed (with reasons); measurements (the tables after your changes); api_notes (only what changed in the public API); game_api_changes (public sim items used by crates/game that you changed); checks (each required command with its result); commit; open_issues.
`, { label: 'Nachbesserung', phase: 'Nachbessern', schema: FIX_SCHEMA })

return { a2, ai, findings: all, confirmed, fix }
