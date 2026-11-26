export const meta = {
  name: 'm4-spiel-flotten',
  description: 'M4 Spielstrang: Kern mit Staatshaushalt einmergen, Flottenfenster, Gefechte der Partie in der 3D-Ansicht; zwei Blickwinkel nacheinander pruefen, nachbessern',
  phases: [
    { title: 'Umsetzen', detail: 'Merge, Flottenfenster, Haushalt, Gefechte der Partie in 3D' },
    { title: 'Pruefen', detail: 'Bedienung und Anforderungen nacheinander, je eine Sammel-Gegenpruefung' },
    { title: 'Nachbessern', detail: 'bestaetigte Befunde beheben' },
  ],
}

const ROOT = '/home/user/exp_bevy_stellaris_fable_onePrompt'
const DIR = ROOT + '/.worktrees/spiel'
const BRANCH = 'wip/m4-spiel'
const TARGET = ROOT + '/target'
const REF = ROOT + '/.worktrees/uebergabe/uebergabe'
const PAD = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad'
const SCRATCH = PAD + '/spiel3'
const KERN = PAD + '/kern/a1_bericht.md'
const CORE = 'f053104'
const BASE = '9c833d5'
const A2R = PAD + '/kern/a2_bericht.md'
const B2R = PAD + '/spiel2/berichte/b2.md'

const WINDOW = display => `
HOW TO SEE AND DRIVE THE GAME (no GPU here; software rendering under a virtual X server works)
- Build directory: this worktree shares the engine build of the parent checkout. Put CARGO_TARGET_DIR=${TARGET} in front of EVERY cargo command (for example: cd ${DIR} && CARGO_TARGET_DIR=${TARGET} cargo build -p game). Without it cargo would rebuild the whole engine (20 minutes). The binary is then ${TARGET}/debug/game.
- The build directory is shared state: build ONLY this worktree into it. If you ever need a second copy of the sources for an experiment, give it its own CARGO_TARGET_DIR. Cargo judges the freshness of the workspace crates without regard to the checkout path, and 'sim' bakes its source path into the path of the game data, so a 'sim' compiled from another checkout can be reused silently. Therefore run once at the very start and once before your final acceptance run: cd ${DIR} && CARGO_TARGET_DIR=${TARGET} cargo clean -p sim -p game   (the engine build stays).
- Scripted run with photos: cd ${DIR} && xvfb-run -a -s "-screen 0 1600x900x24" ${TARGET}/debug/game --selftest 200 --seed 3 --screenshot ${SCRATCH}/NAME.png   (prints SELBSTTEST OK on success; with --practice it runs a practice battle; see --help). Look at every PNG with the Read tool; do not trust the OK line alone.
- Real input against a running window (your own display number is :${display}, use no other):
    Xvfb :${display} -screen 0 1600x900x24 >${SCRATCH}/xvfb${display}.log 2>&1 &   (remember the PID)
    until [ -e /tmp/.X11-unix/X${display} ]; do sleep 0.2; done
    cd ${DIR} && DISPLAY=:${display} ${TARGET}/debug/game [OPTIONS] >${SCRATCH}/game${display}.log 2>&1 &   (remember the PID)
    wait about 30 seconds for the first frames, then photograph: DISPLAY=:${display} import -window root ${SCRATCH}/NAME.png   and look at it
    click: DISPLAY=:${display} xdotool mousemove X Y click 1      wheel: xdotool click 4 (up) or 5 (down)      drag: xdotool mousedown 3, mousemove_relative, mouseup 3
    keys reach the game only after the window has the focus: W=$(DISPLAY=:${display} xdotool search --name KONFLUENZ | head -1); DISPLAY=:${display} xdotool windowfocus $W; DISPLAY=:${display} xdotool key space   (F5 saves to saves/schnell.ron in the directory the game was started from, F9 loads it; the directory saves/ is ignored by git; number keys set the speed and also unpause)
    after each action wait 1-2 seconds and photograph again; the photo is your evidence
    stop the game and Xvfb by their PIDs. Never use pkill or killall.
- The whole game can also be tested without window and GPU: the recipe is in crates/game/src/testing.rs (whole-game tests, buttons found by their label).
`

const COMMON = display => `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1, bevy_egui 0.43.1 with egui 0.36).
Workspace: crates/sim = deterministic simulation core without rendering; crates/game = Bevy app (galaxy map, egui interface, 3D battle view with the practice mode 'Gefechtsübung'); crates/headless = command line tool. Game data lives in assets/data/*.ron.

YOUR WORKING DIRECTORY (a git worktree on branch ${BRANCH}): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout ${ROOT} and the other directories under ${ROOT}/.worktrees/ belong to OTHER agents. Never edit anything outside ${DIR} (except your scratch directory). You may READ ${REF} (review findings and reports of earlier engineers; treat them as claims to check against the code).
- Scratch directory for logs, photos and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first.
${WINDOW(display)}
BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, interface texts, error messages, CLI help, commit messages, README. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code. No model or tool names in code, comments or documents.
2. The interface never changes simulation state directly; it sends commands or requests as the existing code does, and it shows only what the simulation offers as read data.
3. Tests must be honest and meaningful. Never weaken an assertion just to make it pass; find the cause.
4. Wording: neutral, abstract game-mechanics language in code, comments and texts.
5. Git: commit inside your worktree, early and often, and after each commit push exactly your own branch with: git push -u origin ${BRANCH}   (retry up to 4 times with a pause on network errors). Never push any other branch, never create pull requests, never run git stash, git reset --hard, git clean, git rebase or a checkout of another branch. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
6. Cargo: never judge success from output piped through tail or grep - redirect to a log file in the scratch directory and check the exit code ($?). Run a long command as ONE foreground call with a generous timeout (up to 10 minutes) instead of starting it in the background and polling it: every poll costs a full turn.
7. Format with: CARGO_TARGET_DIR=${TARGET} cargo fmt -p game
8. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository.
9. Economy of effort: one test or one photo per fix or feature is enough evidence - no before-and-after galleries, no duplicate proofs. Keep the final report compact: the summary at most 1500 characters, list entries one or two sentences each.

REQUIRED FINAL STATE (all with CARGO_TARGET_DIR=${TARGET}, in ${DIR}, after the cargo clean named above):
  cargo fmt -p game -- --check
  cargo build -p game
  cargo test -p game
  cargo clippy -p game --all-targets -- -D warnings
  campaign selftest: --selftest 200 --seed 3 --screenshot FILE  -> SELBSTTEST OK, photo looked at
  practice selftest: --practice --selftest 400 --seed 3 --screenshot FILE -> SELBSTTEST OK, photos looked at
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
    checks: { type: 'string' },
    commit: { type: 'string' },
  },
  required: ['summary', 'fixed', 'not_fixed', 'checks', 'commit'],
}


const GOAL = `
WHAT THIS STEP DELIVERS (the last interface work of milestone 4: 'combat as the focus of the game - fleets of ships built in yards, supplied and paid from a state budget, battles the player can watch and command in 3D')
Situation
- This branch (${BASE}) holds the game on the new simulation core as of core commit 8758cd7: map with fleets and their states, a small fleet window, the technology window, practice battles in the 3D view. Report of the engineer who did that: ${B2R} (read 'api_drift', 'sim_defects', 'open_issues').
- Meanwhile the core got the state budget and the resolved findings of the strategic layer: commit ${CORE} of branch wip/m4-integ. Its report: ${A2R} - read 'api_notes' (budget read model Economy::budget, BudgetMonth, Claim; BuildOrder status; fleet states) and 'game_api_changes'. Merge exactly ${CORE} (git merge ${CORE}; available locally), NOT the tip of that branch: the AI's military behaviour is being built there right now and reaches this branch later; nothing in your work may depend on it.
- All your own changes stay inside crates/game/** and README.md. If you believe crates/sim has a defect, report it under sim_defects with a reproduction and do not change it. A tiny additive read helper in crates/sim is acceptable only where the interface would otherwise copy simulation logic; name each one.

Work
1. Merge ${CORE} (real merge commit, German message), make everything compile and all tests pass again; follow the changes in meaning the core report lists (MoveFleet rejected while regrouping, strength falling daily in a battle, blockade kept while fighting, the treasury following the budget).
2. The state budget in the interface (in the existing window 'Staat'): the last closed month and the sum of the last twelve months from Economy::budget(player) - revenue, every claim in rank order with wanted and paid (Claim::label), unpaid amounts marked, remainder, reserve target and hand-out; a short German explanation of the order of claims and of what a cut means; arrears of the fleet visible. No logic in the interface beyond formatting.
3. The fleet window, reworked (crates/game/src/ui/fleets.rs; split into several files if it grows): 
   - list of own fleets with place, state, strength, supplies (fuel, ammunition), orders, formation and doctrine;
   - a selected fleet shows its ships: class, design, hull and crew state as the core offers them, damaged ships marked;
   - merging two fleets in the same system and splitting chosen ships off (SimCommand::MergeFleets, SplitFleet), march by map target, retreat while fighting; every button is disabled with a reason shown when the core would reject the command (look at the validation in crates/sim/src/command.rs and its users);
   - shipyards: every own yard with its slots, the build orders with progress, state and the reason when they wait (BuildOrder::status / status_text), cancelling (CancelBuild), and ordering new ships (BuildShip for a design, BuildClass where it exists): the designs the community knows with class, cost, upkeep per month and the parts needed, unknown designs shown as locked with the technology they need;
   - what the fleet costs: the fleet and procurement lines of the budget, and what one more ship of the chosen design would add per month;
   - usable at 1280x720 and 1600x900, texts in the style of the other windows, clicks do not reach the map (crates/game/src/pointer.rs).
4. Battles of the running campaign in the 3D view:
   - a battle in which the player takes part is shown on the map (the marker exists) and reported; the player can open it in the 3D battle view from the map marker, the fleet window and the chronicle notice, watch it there, and leave back to the map at any time while it goes on;
   - the view is fed from the real battle of the simulation (Battles.active - read how the practice mode feeds the view through BattleFeed and BattleOrder in crates/game/src/battle_view/feed.rs and practice.rs, and how battles advance inside a running game in crates/sim/src/fleet/battles.rs). The campaign battle advances with the game's clock, not with a clock of its own: pause and speed of the campaign apply; say in the view how battle time relates to game days. Never run a private copy of the battle that could diverge from the simulation;
   - the orders the view offers (formation, doctrine, retreat, and whatever the practice mode offers that the core accepts in a running game) go to the simulation as SimCommand; orders the core does not accept in a campaign battle are not offered there;
   - battles between other communities can be watched where the player's estimate allows (decide and report the rule; simplest honest rule: only battles in systems where the player has a fleet or a world);
   - when the battle ends while the view is open, the result is shown (winner, losses per side, who holds the field, where the loser withdraws to) and the view returns to the map on request; BattleViewRequest::Leave and a save/load or quit to the menu during an open campaign battle work without a crash;
   - the practice mode keeps working unchanged.
5. Tests - whole-game tests without a window (recipe in crates/game/src/testing.rs): the budget panel shows the figures of Economy::budget; ordering a ship from the window creates the build order and the ship later joins a fleet in a played game; merge and split through the window; a disabled button names its reason; a campaign battle is opened in the view, the ships shown equal the ships of the simulation's battle on several days, an order given in the view reaches the simulation, leaving and re-entering works, the battle's end shows the result; loading a save while the campaign battle view is open. Extend the click-through walkthrough by the new windows.
6. Prove it on the running game with real input (declare a war and march a fleet at an opposing fleet, or use the options of the game binary that help - see --help): photos of the budget panel, the fleet window with ships and a yard with a running order, a campaign battle in the 3D view while it runs, and its result. Keep README.md true (section 'Bedienung' and what the milestone now offers).
`

const IMPL_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    merge: { type: 'string' },
    budget_panel: { type: 'string' },
    fleet_window: { type: 'string' },
    campaign_battles: { type: 'string' },
    tests_added: { type: 'string' },
    photos: { type: 'array', items: { type: 'string' } },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    sim_defects: { type: 'array', items: { type: 'string' } },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'merge', 'budget_panel', 'fleet_window', 'campaign_battles', 'photos', 'checks', 'commit', 'sim_defects', 'open_issues'],
}

phase('Umsetzen')
const impl = await agent(COMMON(88) + GOAL + `
Commit and push after each of the six steps. Step 4 is the largest; start it only when 1 to 3 are committed and green.

FINAL REPORT (structured output): summary; merge; budget_panel; fleet_window (what it shows, which commands it sends, when buttons are disabled); campaign_battles (how the view is fed, how time runs, which orders exist, the rule for foreign battles); tests_added; photos (path and what each shows); files_changed; checks (each required command with its result); commit; sim_defects; open_issues.
`, { label: 'Flottenfenster und Gefechte der Partie', phase: 'Umsetzen', schema: IMPL_SCHEMA })
if (!impl) {
  return { error: 'Umsetzung lieferte kein Ergebnis' }
}
log('Umsetzung fertig: ' + impl.summary.slice(0, 300))

const CLAIMS = `
What the engineer reported (claims to check, not facts):
${impl.summary}
Merge: ${impl.merge}
Budget panel: ${impl.budget_panel}
Fleet window: ${impl.fleet_window}
Campaign battles: ${impl.campaign_battles}
Photos: ${(impl.photos || []).join(' | ')}
Core defects he suspects: ${JSON.stringify(impl.sim_defects)}
Open issues he named: ${JSON.stringify(impl.open_issues)}
`

const REVIEW_COMMON = display => `
You are reviewing changes in the game project KONFLUENZ (Rust, Bevy 0.20.1, bevy_egui 0.43.1). You do NOT edit any file in the repository; you read, build, run and report.
Directory (git worktree on branch ${BRANCH}): ${DIR} - start every Bash command with 'cd ${DIR} &&' and use absolute paths below it. Never touch the parent checkout or other worktrees. The working tree must be clean when you finish.
Scratch directory for your own logs and photos: ${SCRATCH}/pruefung
Subject: the interface work of milestone 4 in crates/game since commit 5f997cb: the game on the new simulation core, the technology window, and - the newest part, since ${BASE} - the state budget panel, the reworked fleet window with shipyards, and battles of the campaign in the 3D view. See 'git log --oneline 5f997cb..HEAD -- crates/game' and 'git diff ${BASE} -- crates/game'. crates/sim is NOT your subject, only how the game uses it; where the game misbehaves because of the core, say so.
${WINDOW(display)}
You may run (always with CARGO_TARGET_DIR=${TARGET}): cargo build -p game, cargo test -p game, the selftests, and the game itself with real clicks as described. Run one cargo command at a time as one foreground call, output to a log file, judged by exit code. Do not run cargo clean.
The task the engineer had:
${GOAL}
${CLAIMS}
Report only real, concrete problems with a location and a failure scenario you observed or can derive step by step: a crash, a wrong reaction to input, a number on screen that differs from the simulation, a command that is offered but rejected (or possible but not offered), a view that diverges from the simulation, a required item that is missing, a regression, a test that would pass while the property is false, a false README statement. No style remarks, no wishes. At most the 12 most important findings.
`

const LENSES = [
  {
    key: 'bedienung',
    display: 82,
    prompt: `
YOUR LENS: operation and robustness, on the running game with real clicks, keys and photos. Play a campaign for some years as a player would: open every new window at 1600x900 and at 1280x720; order ships until the money runs out and watch the orders wait and resume; cancel orders; merge and split fleets; march, declare a war, meet an opposing fleet, open the battle in the 3D view from each entry point, give orders, leave and re-enter, pause and change speed while inside, save (F5) and load (F9) inside, quit to the main menu from inside and start again; let a battle end while watching and while not watching. Check that clicks and wheel on every new window do not reach the map or the 3D scene, that figures in the budget panel add up, and that nothing a player reads is cut off, overlapping, in the wrong language or misleading. Also look for regressions in the practice mode and the technology window.`,
  },
  {
    key: 'anforderungen',
    display: 83,
    prompt: `
YOUR LENS: requirements, code and honesty of the tests. Read the diff since ${BASE} line by line. Is the campaign battle view really fed from the simulation's battle on every frame, or is there a copy that can diverge (after loading, after a ship joins or leaves, after a retreat, across a pause)? Do all orders go through SimCommand, and does the interface never write simulation state? Is every command the windows send valid when offered - compare each enabling condition with the core's validation - and is every reason shown true? Do the figures of the budget panel and of 'what one more ship costs' equal the core's read model (write a throw-away test under a name starting with tmp_ and delete it again)? Does the rule for watching foreign battles leak exact foreign state the estimates do not give? Systems that read Battles or fleets while a load or a quit replaces the simulation: any ordering that can panic? Read the new tests critically: could each pass while its property is false? Is README.md accurate about fleets, yards, budget and battles?`,
  },
]

const verify = async (review, lens) => {
  const findings = (review && review.findings) || []
  if (findings.length === 0) {
    return []
  }
  const result = await agent(REVIEW_COMMON(lens.display + 2) + `
YOUR JOB: check the findings of one reviewer independently. For EACH finding try to REFUTE it: read the code yourself and, where it is cheap, reproduce the claimed failure on the running game or with a test. Give real=true only if you are convinced the problem exists and matters. Give real=false if it is wrong, purely cosmetic, a wish, or already handled. Spend your effort in proportion to the severity. Return one verdict per finding, with the finding's index (starting at 1), your own severity and a short reasoning that names what you checked.

FINDINGS OF THE REVIEWER:
${findings.map((f, i) => (i + 1) + '. [' + f.severity + '] ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-')).join('\n')}`, { label: 'Gegenpruefung ' + lens.key, phase: 'Pruefen', schema: VERDICTS_SCHEMA })
  const verdicts = (result && result.verdicts) || []
  return findings.map((f, i) => {
    const verdict = verdicts.find(v => v.index === i + 1)
    return { ...f, lens: lens.key, verdict: verdict || null }
  })
}

phase('Pruefen')
// Nacheinander: der gemeinsame Bauordner und die Rechenzeit vertragen nur einen Agenten dieses Strangs.
const reviewed = []
for (const lens of LENSES) {
  const review = await agent(REVIEW_COMMON(lens.display) + lens.prompt, { label: 'Pruefung: ' + lens.key, phase: 'Pruefen', schema: FINDINGS_SCHEMA })
  reviewed.push(await verify(review, lens))
}
const all = reviewed.flat().filter(Boolean)
// Ohne Urteil (Gegenpruefer ausgefallen) gilt ein Befund als offen: der Nachbesserer prueft ihn selbst.
const confirmed = all.filter(f => !f.verdict || f.verdict.real)
log(all.length + ' Befunde, davon ' + confirmed.length + ' bestaetigt oder ungeprueft')

phase('Nachbessern')
if (confirmed.length === 0) {
  return { impl, findings: all, confirmed, fix: null }
}
const fix = await agent(COMMON(86) + GOAL + `
YOUR TASK: the work described above was done by another engineer (see 'git diff ${BASE}') and then reviewed. The findings below were confirmed by an independent checker (or could not be checked - then verify them yourself first). Fix every real one properly - the cause, not the symptom - with a whole-game test or a photo as proof, and keep everything else working. Findings whose cause lies in crates/sim: do not change the core; make the game behave sensibly and report the defect with a reproduction under not_fixed.
${CLAIMS}
FINDINGS TO RESOLVE:
${confirmed.map((f, i) => (i + 1) + '. [' + ((f.verdict && f.verdict.severity) || f.severity) + '] (' + f.lens + ') ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-') + '\n   Checker: ' + (f.verdict ? f.verdict.reasoning : 'not checked')).join('\n')}

Finish with all required commands green, committed and pushed. FINAL REPORT (structured output): summary; fixed; not_fixed (with reasons); checks; commit; open_issues.`, { label: 'Nachbesserung', phase: 'Nachbessern', schema: FIX_SCHEMA })

return { impl, findings: all, confirmed, fix }
