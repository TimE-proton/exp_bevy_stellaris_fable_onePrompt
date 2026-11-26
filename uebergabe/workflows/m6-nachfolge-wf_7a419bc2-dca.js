export const meta = {
  name: 'm6-nachfolge',
  description: 'Paket D: Besitzwechsel, Nachfolge und die beiden Enden im Kern - umsetzen, schlank pruefen, nachbessern',
  phases: [
    { title: 'Umsetzen', detail: 'succession.rs, endgame.rs, Stand der Diplomatie, Invarianten, Tests' },
    { title: 'Pruefen', detail: 'zwei Blickwinkel, je eine Sammel-Gegenpruefung' },
    { title: 'Nachbessern', detail: 'bestaetigte Befunde beheben' },
  ],
}

const ROOT = '/home/user/exp_bevy_stellaris_fable_onePrompt'
const DIR = ROOT + '/.worktrees/nachfolge'
const BRANCH = 'wip/m6-nachfolge'
const BASE = '5ac904d'
const REF = ROOT + '/.worktrees/uebergabe/uebergabe'
const M6 = REF + '/entwurf/M6_Synthese.md'
const M5 = REF + '/entwurf/M5_Synthese.md'
const PAD = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad'
const SCRATCH = PAD + '/nachfolge'
const KERN = PAD + '/kern/a1_bericht.md'

const COMMON = `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1).
Workspace: crates/sim = deterministic simulation core without any rendering (Bevy plugins on FixedUpdate, 20 ticks per in-game day, 360 days per year); crates/game = Bevy app; crates/headless = command line over sim::harness. Game data lives in assets/data/*.ron.
Finished: galaxy, economy with local prices and freighters, stock market and cohesion with secession, fleets made of ships with shipyards and supply, a tactical battle calculator, technology. You build the groundwork that the later parts (contest for planets, diplomacy, the endings) share.

YOUR WORKING DIRECTORY (a git worktree on branch ${BRANCH}): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout ${ROOT} and the other directories under ${ROOT}/.worktrees/ contain files with the same names and belong to OTHER agents working in parallel. Never edit anything outside ${DIR} (except your scratch directory).
- You may READ the reference material in ${REF} (design documents under entwurf/, reports of earlier agents under berichte/). The report of the engineer who joined the fleet, battle and technology branches is being written right now and will appear at ${KERN}; read it when it is there (API notes of the fleet layer), otherwise read the code.
- Scratch directory for logs and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first. It explains the architecture, the determinism rules, the economy and the ownership model.

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, UI and chronicle strings, error messages, CLI help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code. No model or tool names in code, comments or documents.
2. Determinism (crates/sim): no iteration over HashMap/HashSet (use Vec or BTreeMap), nothing time- or thread-dependent, randomness only through crate::rng::Rng::stream(seed, Domain::X, a, b), sorting with total_cmp plus an id tie-break. All simulation state is plain serde data in Resources listed in the sim_state! macro in crates/sim/src/state.rs (that list defines save games and the state hash). The headless harness skips idle ticks (Headless::run_days versus run_days_exact must give the same hash).
3. Money is i64 milli-credits (type Money) and is conserved: it is only moved, never created or destroyed. sim::state::money_drift(world) must stay 0. Shares are conserved too: the total number of shares of an issuer never changes through your code.
4. Player and AI act on the simulation only through SimCommand.
5. Balance levers belong in assets/data/*.ron with serde structs in crates/sim/src/data/, validated when loaded, with German error messages.
6. Tests must be honest and meaningful. Never weaken an assertion just to make it pass; find the cause.
7. Wording: neutral, abstract game-mechanics language in code, comments and texts (communities, planets changing owner, a community ending, accession, union).
8. Git: commit inside your worktree, early and often (after every self-contained step that compiles), and after each commit push exactly your own branch with: git push -u origin ${BRANCH}   (retry up to 4 times with a pause on network errors). Never push any other branch, never create pull requests, never run git stash, git reset --hard, git clean, git rebase or a checkout of another branch. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
9. Cargo: never judge success from output piped through tail or grep - redirect to a log file in the scratch directory and check the exit code ($?). Generous timeouts (up to 10 minutes; longer things in the background, then poll the log). The machine is shared with other builds; a full 'cargo test -p sim' can take 10 minutes or more.
10. Format with: cargo fmt -p sim -p headless
11. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository. Do not edit README.md (the integrator does that from your report).

REQUIRED FINAL STATE (all must succeed in ${DIR}):
  cargo build -p sim -p headless
  cargo test -p sim
  cargo clippy -p sim -p headless --all-targets -- -D warnings
  cargo fmt -p sim -p headless -- --check
  cargo build --release -p headless
  target/release/headless sweep --seeds 0..8 --years 60     (no invariant violations)
  target/release/headless hash --seed 3 --days 3600          (twice, identical output; also identical with --exact)
Do NOT build crates/game. Do not change SAVE_FORMAT (the integrator raises it once).
`

const DESIGN = `
THE DESIGN (binding in its semantics; adapt names and signatures to the style of the code where needed and report it)
Sources: ${M6} (sections 2, 3 'Victory' and 'Why every game ends once', 6 first bullet, 8 tests 1, 4 and 12, 9 step 2) and ${M5} (section 6: transfer_planet and wind_up; section 8 tests 12 and 13). Both documents were written before fleets with ships and technology existed; where they disagree with each other or with the code, this design wins, then the code. Read both sections before you start.

A. crates/sim/src/polity/succession.rs - the ONLY place where a planet changes its owner, where a community comes into being after the start, and where a community ends.
   - A struct of mutable borrows of everything such a change touches (galaxy, polities, economy, finance, logistics, fleets, research, diplomacy, endgame, chronicle, the game configuration) plus the game data, with a bevy SystemParam twin so that systems and the command execution can build it, and a helper on sim::harness::Headless so that tests and later tools can call the functions on a world. Where a per-community or per-planet struct is taken apart or filled, destructure it exhaustively (no '..'), so that a field added later fails to compile here instead of being forgotten.
   - spawn(parent, founding, tick) -> id of the new community. The ONLY place where per-community vectors grow. Find all of them (today the push code sits in finance::cohesion::apply_secession and misses some): Polities.list, every per-community vector in Economy, Finance.polities, the research state, the new Diplomacy vectors, anything in fleets, logistics or AI keyed by the community index. The child inherits tax rate and culture of the parent and starts without treasury, debt and knowledge beyond what the code gives a community at its start (decide and report what a child knows of its parent's technology; a child that knows nothing cannot defend itself).
   - transfer_planet(planet, to, terms, tick) -> { from, new_capital, eliminated }. terms = { state_shares_to: HolderId, state_share: f64, cohesion: f64, stage: Stage, shock: f64, elite_flight: f64, path: Path }.
       * sets the owner, clears the tax exemption, sets the region bookkeeping (owner seen, day, stage, month counters, cohesion) from the terms;
       * moves the share 'state_share' of the old state's holdings in the issuers seated on that planet to 'state_shares_to' and multiplies their price shock by 'shock'. A secession passes (the planet's public, 0.5, base + honeymoon, loyal, 0.85) - exactly today's behaviour;
       * records in the endgame state for that planet: the path, the population at that moment and the cultural distance between old and new owner at that moment (path Own clears them);
       * if the planet was the capital of the old owner: the most populous remaining planet becomes the capital (ties: lower id); the trade house's home follows; that region is at most restless and taxable; the share 'elite_flight' of what the elite of the old capital holds in issuers on the loser's remaining territory (trade house included) and of its cash moves to the elite of the new capital;
       * anything bound to the planet that belongs to the old owner is settled: build orders of its shipyard (decide: cancel, with what happens to money and goods already spent, and report it), and whatever else you find by searching for uses of the planet's owner;
       * if the old owner has no planet left, it ends through absorb(old, to, path) in the same call ('eliminated').
   - absorb(from, into, path, tick): 'from' ends, 'into' is its heir. Remaining planets change owner keeping their region state; planet tags: a peaceful path (Joined) retags only planets tagged Own or Joined - a planet tagged Forced keeps that tag through every later union; path Forced tags all. A capital that joins becomes a 'seat' (a flag per planet, to be read by the cohesion rules later). The trade house, the freighters and their cargo pass to the heir (check that the code copes with a community owning several trade houses; make it cope). Fleets pass to the heir on a peaceful path and are dissolved on a forced one - including fleets that are in a running battle, in transit, under repair or in a build queue; nothing may keep pointing at them. Bonds: issuer and holder are relabelled; a bond that the heir then owes itself is cancelled. Shares held by the state of 'from' (holdings and open offers) pass to the state of the heir. The treasury is moved, and the heir's books are adjusted so that the money does not count as revenue of the month. Research is merged (union of what both know, the larger progress). Wars of 'from' end (new diplomacy::drop_wars_of - otherwise the heir's opponents stay at war with nobody for ever). Wards of 'from' pass to the heir. Finally alive = false and capital = None. If the configured player is 'from': the player's fate becomes Out and the configuration has no player any more (the game runs on; a later union command will set Merged itself). Chronicle entries in German.
   - subordinate(ward, lord, voluntary, tick): 'ward' keeps its planets, treasury and fleets but gets 'lord' as overlord. Both must be alive and free; wards of the ward pass to the lord (no chains). The ward's own wars end; from now on it shares the wars of its head. Its planets are tagged Joined (voluntary) or Forced, with the same rule that Forced survives. Player as ward: fate Out, no player.
   - All four return Result<_, String> with German reasons for impossible calls (dead or identical communities, a ward as target, a planet without owner where one is needed) and never leave a half-done change behind: check first, then change.

B. crates/sim/src/diplomacy: Diplomacy keeps 'wars' and gains 'standing: Vec<Standing>' (per community: overlord: Option<PolityId>, voluntary: bool, since_day: u64) and 'culture: Vec<[f32; 3]>' (openness, public spirit, readiness; 0..1; from a new field 'culture: (o, g, w)' of every entry in assets/data/polities.ron - choose values that fit each community's name, motto and character and that differ visibly from each other). Helpers: head(p) = overlord or p; is_free(p); wards_of(p); bloc(p); affinity(a, b) = 1 - |c_a - c_b| / sqrt(3). at_war, enemies_of and is_at_war keep their names and resolve heads, so a ward shares the wars of its head and members of one bloc are never at war with each other. declare_war and make_peace are only open to free communities (German reason otherwise). Check every caller of these functions in the fleet, logistics and command code for what head resolution means there.

C. crates/sim/src/endgame.rs:
   - Endgame (new entry in sim_state!, directly after the line 'diplomacy: Diplomacy,'): per planet 'path: Vec<Path>' (Own | Joined | Forced), 'absorbed_pop: Vec<f64>', 'gap: Vec<f32>', 'seat: Vec<bool>'; 'peak_population: f64', 'months_below: u32', 'free_states: u32' (communities founded by secession so far), 'player: PlayerFate' (Active | Merged | Out), 'outcome: Option<Outcome>' with Outcome { ending: Ending (Metamorphosis | Extinction, German labels 'Metamorphose' and 'Auslöschung'), survivor: Option<PolityId>, day, joined: f64, forced: f64 }.
   - Census: plain data (total population, peak, months below, and per free living community its bloc population and the population sums of its bloc's planets by tag, taken from absorbed_pop). classify(&Census, &levers) -> Option<verdict> is a PURE, TOTAL function. Sovereign = free and bloc population >= sovereign_min_share of the total. Rule 1: total population below collapse_share x peak for collapse_months in a row, or no sovereign at all -> Extinction without survivor. Rule 2: exactly one sovereign -> Metamorphosis if joined > forced, else Extinction; that sovereign is the survivor; free communities below the floor are ignored. Otherwise None.
   - judge: once a month in SimSet::Victory after everything else of that month: update peak and the month counter, build the census, classify, and write the outcome exactly ONCE together with one chronicle entry of a new kind for the end (German text naming the ending, the survivor and the two sums).
   - After the outcome is written the simulation stands still: sim::game_running (crates/sim/src/lib.rs) requires that there is no outcome; Headless::run_days and run_days_exact return early; the state hash does not change any more. RunSummary and the headless 'run' and 'sweep' outputs show the ending and its year (new columns at the END of the CSV).

D. finance::cohesion::apply_secession is rewritten on top of spawn and transfer_planet with unchanged behaviour, plus: founding a new community needs free_states < max_free_states (count them in Endgame). An accession to an existing community passes path Joined. Whoever needs the issuers seated on a planet uses one shared function.

E. Levers: a new block 'endgame: (max_free_states: 6, sovereign_min_share: 0.02, collapse_share: 0.30, collapse_months: 24)' as the FIRST block of assets/data/balance.ron and first field of the Balance struct, validated (shares inside (0, 1), sovereign_min_share x max_polities < 1, at least one month). And 'reserve_floor: 0.02' in the economy block, replacing the hard-coded '.max(0.02)' on the reserve of a deposit in crates/sim/src/economy/production.rs - same value, no change of behaviour.

F. Invariants: a function succession::check(world) -> Result<(), String>, called by exactly ONE added line as the FIRST statement of sim::harness::check_invariants: all per-community vectors have the same length; all per-planet endgame vectors have the planet count; every living community has a capital and owns it; a community that ended owns no planet, no fleet, no trade house and no build order, is in no war, issues and holds no bond, holds no shares as a state, has no treasury, no overlord and no ward; an overlord is alive and free; the configured player, if any, is alive and free; nothing in the state refers to a community that ended (search every place that stores a PolityId).

G. Tests (new file crates/sim/tests/succession.rs for world tests, unit tests next to the code):
   1. classify_is_total: a table - collapse, no sovereign, a tie of joined and forced (Extinction), wards counted through their head by tag, small free communities ignored, two sovereigns (None), and a property loop over generated censuses that never panics and returns at most one verdict.
   2. succession_conserves: on a running game (several seeds, some years in) perform, one per twin: a secession into a new community, an accession of a region, a peaceful absorption, a forced absorption, a subordination. Each keeps money_drift == 0, the share totals of every issuer, the total population, and passes check_invariants on that day and after two more years of play.
   3. capital_falls: transfer a capital - capital and trade-house home move, the new capital's elite gains exactly the stated share of each holding and of the cash, the new capital is not autonomous, and the gainer's foreign share in a remaining colony of the loser is above zero.
   4. last_planet: transferring the last planet ends the community - not alive, its opponents are not at war with it any more, treasury and bonds passed on, no fleets left, money_drift == 0.
   5. tags: a planet tagged Forced keeps the tag through a later peaceful absorption; a secession into a new community clears it.
   6. wards: a ward shares the wars of its head, cannot declare war or make peace, and members of a bloc are not at war with each other.
   7. endings: subordinating or absorbing the last rival by force gives Extinction with the survivor named; a peaceful majority gives Metamorphosis; with a test data set whose deposits run out quickly and reserve_floor 0 a passive game ends in Extinction without survivor; in each case run_days returns early, the hash is frozen afterwards and the chronicle holds exactly one end entry.
   8. determinism: the same seed twice, run_days against run_days_exact, and saving and loading right after a succession give equal hashes.

OUT OF SCOPE (other packages): treaties, proposals, embargo, tribute, war score and peace terms, the hidden minds of the AI, cohesion rules that read the new tags, any new SimCommand, the contest for planets (crates/sim/src/siege exists on another branch), crates/game.

KEEPING THE LATER MERGE CLEAN: other engineers are changing crates/sim/src/fleet/{mod,battles,yard,supply,movement}.rs, crates/sim/src/economy/capital.rs, crates/sim/tests/military.rs, the 'military' block of balance.ron and crates/headless at the same time, and another branch adds a block at the END of balance.ron, an entry after 'fleets: Fleets,' in sim_state! and a line directly before the final Ok(()) of check_invariants. Therefore: put fleet-side helpers (dissolving or relabelling fleets, removing a community from running battles and build queues) into a NEW file crates/sim/src/fleet/succession.rs and touch the existing fleet files only with the few lines that are unavoidable; add your plugin as the last one; add harness helpers at the end of the file; add command-line output at the end of existing lists.
`

const IMPL_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    api_notes: { type: 'string' },
    decisions: { type: 'string' },
    game_api_changes: { type: 'string' },
    readme_text: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'api_notes', 'decisions', 'game_api_changes', 'checks', 'commit', 'open_issues'],
}

const IMPLEMENT = COMMON + DESIGN + `
YOUR TASK: implement the design completely, with the tests, and prove the required final state.
Suggested order (commit and push after each): (1) the Diplomacy vectors, culture data, Endgame state at neutral values, the lever block, spawn, and apply_secession on top of spawn - behaviour unchanged: compare the sweep metrics (CSV without the hash column) of seeds 0..8 over 60 years with a binary built from the commit you started on (${BASE}); every difference must be explained in your report; (2) transfer_planet with the capital rules; (3) absorb and the fleet-side helpers; (4) subordinate and head resolution; (5) classify, judge, the standstill after the end, harness and command-line output; (6) invariants and the remaining tests.

FINAL REPORT (structured output): summary; api_notes (the public API as later packages will use it: types, functions, semantics, how to call them from a system, from command execution and from a test); decisions (everything you decided where the design left room, with reasons); game_api_changes (every public item of sim that crates/game uses and that you changed, with the replacement - grep crates/game/src); readme_text (German paragraphs for the README describing change of ownership, the end of a community, wards and the two endings); files_changed; checks (each required command with its result, and the sweep comparison); commit; open_issues.
`

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
    api_notes: { type: 'string' },
    checks: { type: 'string' },
    commit: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'fixed', 'not_fixed', 'checks', 'commit'],
}

const REVIEW_COMMON = `
You are reviewing a change in the game project KONFLUENZ (Rust 1.97.1, Bevy 0.20.1; deterministic simulation core in crates/sim, command line in crates/headless). You do NOT edit any file in the repository; you read, build, run and report.
Directory (git worktree on branch ${BRANCH}): ${DIR} - start every Bash command with 'cd ${DIR} &&' and use absolute paths below it. Never touch the parent checkout or other worktrees.
Scratch directory for your own logs and throw-away test files: ${SCRATCH}/pruefung (to try a failing scenario, write a throw-away test file there and run it by copying it into crates/sim/tests under a name starting with 'tmp_', and delete it again before you finish; the working tree must be clean when you are done - check with git status).
Subject: everything since commit ${BASE} (git diff ${BASE} --stat, git diff ${BASE}). README.md explains the project. The design the change had to implement:
${DESIGN}
Rules of the project that matter here: determinism (no HashMap iteration, randomness only through Rng::stream, state only in the sim_state! resources, run_days equals run_days_exact), money and shares are conserved (sim::state::money_drift == 0), tests must be able to fail.
Cargo: redirect output to a log file and check the exit code; the machine is shared, a full 'cargo test -p sim' can take 10 minutes or more - run single test binaries or filters where that is enough.
Report only real, concrete problems with a location and a failure scenario that you observed or can derive step by step from the code: wrong results, lost or created money or shares, state that refers to a community that ended, a panic, a path the design demands and the code lacks, non-determinism, a test that would pass while the property is false. No style remarks, no wishes, nothing that is explicitly out of scope.
`

const LENSES = [
  {
    key: 'erhaltung',
    prompt: `
YOUR LENS: correctness and conservation of every path. Walk through spawn, transfer_planet, absorb and subordinate line by line and ask for each piece of state in the simulation (search for every struct that stores a PolityId or a PlanetId: galaxy, economy, finance with issuers, holdings, offers, bonds, funds and the regions, logistics with trade houses, freighters and flows, fleets with ships, yards, build orders, supply, battles in progress, research with its estimates, diplomacy, the chronicle, the configuration): is it handled, in the right order, without loss? Try the hard cases, by test where it is cheap: the capital changes owner while a fleet of the loser is in a battle or in transit; the last planet goes while the loser has bonds held by the gainer and the gainer has bonds held by the loser; absorb where the heir is the overlord of the one that ends, or its ward; two successions on the same day; a community that ends while it is the target of a running build order, a takeover offer or a dividend; a ward whose head ends; head resolution in every caller of at_war / enemies_of / is_at_war (does a ward's fleet fight its head's opponents, can freighters of bloc members use each other's ports as before, can a ward still be attacked?). Check the capital rules and the elite's holdings with numbers. Check that the invariant function really covers what the design lists - for each item name the line that checks it or report it missing.`,
  },
  {
    key: 'enden',
    prompt: `
YOUR LENS: the endings, determinism, integration and the honesty of the tests. Is classify pure and total, and does it match the two rules exactly (boundaries: exactly at the floor, exactly at the threshold, joined == forced, zero population, one community left without any absorbed population)? Is the outcome written exactly once, and does the simulation really stand still afterwards on every path (the fixed-update systems, command execution, the harness loops, needs_fine_ticks) - run a game to its end and compare hashes. Is anything the new code does dependent on iteration order or on values that are not part of the saved state (save and load in the middle of a month in which a succession happens, compare hashes)? Is the behaviour of games without any succession beyond secessions unchanged against ${BASE} - build both, compare 'sweep --seeds 0..8 --years 60' without the hash column and explain every difference. Read every new test critically: does it exercise the real path through the world, or a fabricated state; could it pass while the property is false; are the claimed quantities really compared? Run the complete required final state yourself and report every command that does not succeed. Check the merge hygiene the design asks for (position of the state entry, the lever block, the single invariant line, the plugin, the new fleet file).`,
  },
]

phase('Umsetzen')
const impl = await agent(IMPLEMENT, { label: 'Nachfolge und Enden', phase: 'Umsetzen', schema: IMPL_SCHEMA })
if (!impl) {
  return { error: 'Umsetzung lieferte kein Ergebnis' }
}
log('Umsetzung fertig: ' + impl.summary.slice(0, 300))

const CLAIMS = `
What the implementer reported (claims to check, not facts):
${impl.summary}
Decisions: ${impl.decisions}
API notes: ${impl.api_notes}
Checks claimed: ${impl.checks}
Open issues named by the implementer: ${JSON.stringify(impl.open_issues)}`

const reviewed = await pipeline(
  LENSES,
  lens => agent(REVIEW_COMMON + lens.prompt + CLAIMS, { label: 'Pruefung: ' + lens.key, phase: 'Pruefen', schema: FINDINGS_SCHEMA }),
  (review, lens) => {
    const findings = (review && review.findings) || []
    if (findings.length === 0) {
      return []
    }
    return agent(REVIEW_COMMON + `
YOUR JOB: check the findings of one reviewer independently. For EACH finding try to REFUTE it: read the code yourself and, where it is cheap, reproduce the claimed failure with a throw-away test. Give real=true only if you are convinced the problem exists and matters (wrong result, lost or created money or shares, a dangling reference, a panic, non-determinism, a demanded path that is missing, a test that cannot fail). Give real=false if it is wrong, out of scope, purely cosmetic or already handled. Spend your effort in proportion to the severity. Return one verdict per finding with the finding's index (starting at 1), your own severity and a short reasoning that names what you checked.

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
  return { impl, findings: all, confirmed, fix: null }
}
const fix = await agent(COMMON + DESIGN + `
YOUR TASK: the design above was implemented by another engineer (git diff ${BASE}) and then reviewed. The findings below were confirmed by an independent checker (or could not be checked - then verify them yourself first). Fix every real one properly - the cause, not the symptom - and add or sharpen a test for each where a test can express it. Keep everything else working. If a finding is wrong after all, say why instead of changing code. Commit and push after each group of fixes. Then prove the required final state again.

What the first engineer reported:
${impl.summary}
API notes: ${impl.api_notes}

CONFIRMED FINDINGS:
${confirmed.map((f, i) => (i + 1) + '. [' + ((f.verdict && f.verdict.severity) || f.severity) + '] (' + f.lens + ') ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-') + '\n   Checker: ' + (f.verdict ? f.verdict.reasoning : 'not checked')).join('\n')}

FINAL REPORT (structured output): summary; fixed (one entry per finding: cause, change, proof); not_fixed (with reasons); api_notes (only what changed in the public API against the first engineer's notes); checks (each required command with its result); commit; open_issues.
`, { label: 'Nachbesserung', phase: 'Nachbessern', schema: FIX_SCHEMA })

return { impl, findings: all, confirmed, fix }
