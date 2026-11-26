export const meta = {
  name: 'm4-spiel-befunde',
  description: 'M4 Paket B1: bestaetigte Befunde der Schlachtansicht und das Durchklicken auf der Karte beheben, dann schlank pruefen und nachbessern',
  phases: [
    { title: 'Umsetzen', detail: 'Befunde beheben, Klicktests, Selbsttests' },
    { title: 'Pruefen', detail: 'zwei Pruefer, je eine Sammel-Gegenpruefung' },
    { title: 'Nachbessern', detail: 'bestaetigte Befunde beheben' },
  ],
}

const ROOT = '/home/user/exp_bevy_stellaris_fable_onePrompt'
const DIR = ROOT + '/.worktrees/spiel'
const BRANCH = 'wip/m4-spiel'
const BASE = '8206ed1'
const TARGET = ROOT + '/target'
const REF = ROOT + '/.worktrees/uebergabe/uebergabe'
const SCRATCH = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel'
const SAVE = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/menu/tour.ron'

const WINDOW = display => `
HOW TO SEE AND DRIVE THE GAME (no GPU here; software rendering under a virtual X server works)
- Build directory: this worktree shares the engine build of the parent checkout. Put CARGO_TARGET_DIR=${TARGET} in front of EVERY cargo command (for example: cd ${DIR} && CARGO_TARGET_DIR=${TARGET} cargo build -p game). Without it cargo would rebuild the whole engine (20 minutes). The binary is then ${TARGET}/debug/game. Another agent may hold the build lock for a moment; cargo waits by itself.
- Scripted run with photos: cd ${DIR} && xvfb-run -a -s "-screen 0 1600x900x24" ${TARGET}/debug/game --selftest 200 --seed 3 --screenshot ${SCRATCH}/NAME.png   (prints SELBSTTEST OK on success; with --practice it runs a practice battle; see --help). Look at every PNG with the Read tool; do not trust the OK line alone.
- Real clicks against a running window (your own display number is :${display}, use no other):
    Xvfb :${display} -screen 0 1600x900x24 >${SCRATCH}/xvfb${display}.log 2>&1 &   (remember the PID)
    until [ -e /tmp/.X11-unix/X${display} ]; do sleep 0.2; done
    cd ${DIR} && DISPLAY=:${display} ${TARGET}/debug/game [OPTIONS] >${SCRATCH}/game${display}.log 2>&1 &   (remember the PID)
    wait about 20 seconds for the first frames, then photograph: DISPLAY=:${display} import -window root ${SCRATCH}/NAME.png   and look at it
    click: DISPLAY=:${display} xdotool mousemove X Y click 1      wheel: xdotool click 4 (up) or 5 (down)      drag: xdotool mousedown 3, mousemove, mouseup 3
    after each action wait 1-2 seconds and photograph again; the photo is your evidence
    stop the game and Xvfb by their PIDs. Never use pkill or killall: other agents run their own instances.
  Key presses sent by xdotool did not reach the game in earlier attempts; drive the interface by mouse and cover keys by headless system tests.
  A campaign save for --load (15 years played, the player leads community 0): ${SAVE}
`

const COMMON = display => `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1, bevy_egui 0.43.1 with egui 0.36).
Workspace: crates/sim = deterministic simulation core without rendering; crates/game = Bevy app (galaxy map, egui interface, 3D battle view with the practice mode 'Gefechtsübung'); crates/headless = command line tool. Game data lives in assets/data/*.ron.

YOUR WORKING DIRECTORY (a git worktree on branch ${BRANCH}): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout ${ROOT} and the other directories under ${ROOT}/.worktrees/ belong to OTHER agents. Never edit anything outside ${DIR} (except your scratch directory). You may READ ${REF} (review findings and reports).
- Scratch directory for logs, photos and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first.
${WINDOW(display)}
BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, interface texts, error messages, CLI help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code. No model or tool names in code, comments or documents.
2. Scope: only crates/game/** and the section 'Bedienung' of README.md (plus a short subsection on the practice mode). Do NOT touch crates/sim, crates/headless, assets/data, Cargo.toml: other agents change them right now on other branches.
3. The interface never changes simulation state directly; it sends commands or requests as the existing code does.
4. Tests must be honest and meaningful. Never weaken an assertion just to make it pass.
5. Git: commit inside your worktree, early and often, and after each commit push exactly your own branch with: git push -u origin ${BRANCH}   (retry up to 4 times with a pause on network errors). Never push any other branch, never create pull requests, never run git stash, git reset --hard, git clean, git rebase or a checkout of another branch. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
6. Cargo: never judge success from output piped through tail or grep - redirect to a log file in the scratch directory and check the exit code ($?). Generous timeouts (up to 10 minutes).
7. Format with: CARGO_TARGET_DIR=${TARGET} cargo fmt -p game
8. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository.

REQUIRED FINAL STATE (all with CARGO_TARGET_DIR=${TARGET}, in ${DIR}):
  cargo fmt -p game -- --check
  cargo build -p game
  cargo test -p game
  cargo clippy -p game --all-targets -- -D warnings
  campaign selftest: --selftest 200 --seed 3 --screenshot FILE  -> SELBSTTEST OK, photo looked at
  practice selftest: --practice --selftest 400 --seed 3 --screenshot FILE -> SELBSTTEST OK, photos looked at
`

const ISSUES = `
THE OPEN ISSUES (full text with locations, failure scenarios, suggested fixes and the checkers' reasoning: ${REF}/befunde/schlachtansicht.md - read every entry marked 'bestätigt' and 'ungeprüft'; several entries describe the same defect from different angles; entries marked 'widerlegt' are not defects)
 1. CRASH: choosing 'Zum Hauptmenü' in a running campaign terminates the game. The quit request removes the simulation state while systems of the same frame still need it (crates/game/src/session.rs). An earlier fix covered only one path.
 2. CLICK-THROUGH: clicks and mouse wheel on the battle top bar act on the scene behind it (ship picking, camera): the picking and camera systems are guarded only by 'egui does not want the pointer', and parts of the interface are drawn in a way egui does not count (crates/game/src/battle_view/hud.rs, camera.rs). THE SAME DEFECT EXISTS ON THE CAMPAIGN MAP: crates/game/src/map.rs (pick_system, camera_controls) - a click on the side panels or bars also clicks the map and loses the selected system. Fix both with ONE shared, tested mechanism that answers 'is the pointer over any part of the interface', and use it everywhere a click, drag or wheel acts on the world.
 3. Inspector: only 9 module rows are visible, the rest hides behind a scroll bar that is invisible until hovered.
 4. Practice setup dialog: accepts any ship count - arithmetic overflow while typing, battles with zero ships, a hang with huge fleets. Bound and validate the input and show a German hint.
 5. Tests and selftest pass even when the scene shows nothing; the interactive paths are untested. Make the selftest fail when no ship is actually visible (for example by counting visible ship entities in the camera frustum) and add headless system tests for the interaction paths you fix.
 6. 'Neu starten' does not start paused at 00:00 when the clock was running.
 7. The help text for the numpad view keys does not match what the keys do.
 8. 'Übersicht' does not show all ships: those near the lower rim are below the screen.
 9. Wrecks and debris are close to invisible at normal viewing distances; small wrecks look like intact ships.
10. Scene and effect bookkeeping resynchronise only when a new battle arrives: ships that join later stay invisible, and effect pools go stale when the same battle is shown again after the view was hidden. This matters for the coming connection to battles of a running campaign; make the bookkeeping robust against ships joining, the view being hidden and shown again, and the battle being replaced at any time (the entry marked 'ungeprüft' on the campaign bridge lists more cases - check each).
11. 'ungeprüft': markers and labels shine through the Bilanz window and the panels; the top bar overlaps itself in narrow windows. Check, fix if real.
12. 'ungeprüft': README does not document the practice mode, its options or the battle controls. Add it to the section 'Bedienung'.
`

const IMPLEMENT = COMMON(81) + ISSUES + `
YOUR TASK: fix every issue above properly (the cause, not the symptom) and prove each fix.
- Before changing code for an issue, reproduce it: for interaction issues with real clicks and photos, for the crash by actually quitting to the main menu from a campaign (start with --load and click through the menu), for logic issues with a failing test.
- After the fix, show the same steps working (photos in ${SCRATCH}) and add a test where a test can express it.
- Quitting to the main menu and starting or loading again must work repeatedly: campaign -> menu -> practice -> menu -> campaign -> menu.
- Keep the interface layout and texts as they are unless an issue requires a change.
- Commit in sensible steps and push after each commit.

FINAL REPORT (structured output): summary; fixes (one entry per issue number: what the cause was, what you changed, how you proved it, which photos show it); not_fixed (with reasons); tests_added; files_changed; checks (each required command with its result); commit; open_issues.
`

const IMPL_SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    fixes: { type: 'array', items: { type: 'string' } },
    not_fixed: { type: 'array', items: { type: 'string' } },
    tests_added: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    checks: { type: 'string' },
    commit: { type: 'string' },
    open_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'fixes', 'not_fixed', 'checks', 'commit', 'open_issues'],
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
    checks: { type: 'string' },
    commit: { type: 'string' },
  },
  required: ['summary', 'fixed', 'not_fixed', 'checks', 'commit'],
}

const REVIEW_COMMON = display => `
You are reviewing changes in the game project KONFLUENZ (Rust, Bevy 0.20.1, bevy_egui 0.43.1). You do NOT edit any file in the repository; you read, build, run and report.
Directory (git worktree on branch ${BRANCH}): ${DIR} - start every Bash command with 'cd ${DIR} &&' and use absolute paths below it. Never touch the parent checkout or other worktrees.
Scratch directory for your own logs and photos: ${SCRATCH}/pruefung
Subject: the fixes made since commit ${BASE} in crates/game (see 'git diff ${BASE} --stat' and 'git diff ${BASE}'), which claim to resolve the issues listed below. README.md explains the project; ${REF}/befunde/schlachtansicht.md holds the original findings.
${WINDOW(display)}
You may run (always with CARGO_TARGET_DIR=${TARGET}): cargo build -p game, cargo test -p game, the selftests, and the game itself with real clicks as described. Redirect cargo output to a log file and check the exit code.
${ISSUES}
Report only real, concrete problems with a location and a failure scenario you observed or can derive step by step: an issue that is still open, a fix that only hides the symptom, a regression, a new crash, a test that would pass while the property is false. No style remarks, no wishes.
`

const LENSES = [
  {
    key: 'bedienung',
    display: 82,
    prompt: `
YOUR LENS: operation and robustness, checked on the running game with real clicks and photos. Go through the issues one by one and try to make each fail again: quit to the main menu from a campaign and start again several times in different orders (campaign, practice, load); click and scroll on every bar, panel and window in both the campaign and the battle view and check that the world behind it does not react (selection, camera), and that clicks next to the interface still do reach the world; type nonsense and extreme numbers into the practice setup dialog; restart a running practice battle; open the inspector on a ship with many modules; use 'Übersicht'; look for wrecks; hide and show the battle view. Also look for regressions: things that worked at ${BASE} and do not now.`,
  },
  {
    key: 'anforderungen',
    display: 83,
    prompt: `
YOUR LENS: requirements, code and honesty of the tests. Read the diff line by line. Is there really ONE shared mechanism that decides whether the pointer is over the interface, used by every system through which a click, drag or wheel acts on the world (grep for mouse and pointer handling in crates/game/src), and is it correct for every way the interface is drawn (panels, windows, areas, tooltips, the top bars)? Is the quit-to-menu fix sound for every schedule in which a system may still need the simulation state, or does it only move the race? Does the selftest now fail when nothing is visible - prove it by reasoning about the check, and where cheap by running it in a situation without ships? Read the new tests critically: could each pass while the property is false? Is the scene bookkeeping (ships joining late, view hidden and shown, battle replaced) correct by construction? Is the README text accurate against the code (keys, options)?`,
  },
]

phase('Umsetzen')
const impl = await agent(IMPLEMENT, { label: 'Befunde beheben', phase: 'Umsetzen', schema: IMPL_SCHEMA })
if (!impl) {
  return { error: 'Umsetzung lieferte kein Ergebnis' }
}
log('Umsetzung fertig: ' + impl.summary.slice(0, 300))

const reviewed = await pipeline(
  LENSES,
  lens => agent(REVIEW_COMMON(lens.display) + lens.prompt + `

What the implementer reported (claims to check, not facts):
${impl.summary}
Fixes claimed:
${impl.fixes.map((f, i) => (i + 1) + '. ' + f).join('\n')}
Not fixed according to the implementer: ${JSON.stringify(impl.not_fixed)}
Open issues named by the implementer: ${JSON.stringify(impl.open_issues)}`, { label: 'Pruefung: ' + lens.key, phase: 'Pruefen', schema: FINDINGS_SCHEMA }),
  (review, lens) => {
    const findings = (review && review.findings) || []
    if (findings.length === 0) {
      return []
    }
    return agent(REVIEW_COMMON(lens.display + 2) + `
YOUR JOB: check the findings of one reviewer independently. For EACH finding try to REFUTE it: read the code yourself and, where it is cheap, reproduce the claimed failure on the running game or with a test. Give real=true only if you are convinced the problem exists and matters (a crash, a wrong reaction to input, an issue from the list that is still open, a regression, a test that cannot fail). Give real=false if it is wrong, purely cosmetic, or already handled. Spend your effort in proportion to the severity. Return one verdict per finding, with the finding's index (starting at 1), your own severity and a short reasoning that names what you checked.

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
const fix = await agent(COMMON(86) + ISSUES + `
YOUR TASK: the issues above were fixed by another engineer (see 'git diff ${BASE}') and then reviewed. The findings below were confirmed by an independent checker (or could not be checked - then verify them yourself first). Fix every real one properly, prove each fix as the first engineer did (reproduce, fix, show it working with photos or a test), and keep everything else working.

What the first engineer reported: ${impl.summary}

FINDINGS TO RESOLVE:
${confirmed.map((f, i) => (i + 1) + '. [' + ((f.verdict && f.verdict.severity) || f.severity) + '] ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-') + '\n   Checker: ' + (f.verdict ? f.verdict.reasoning : 'not checked')).join('\n')}

Finish with all required commands green, committed and pushed. Report what you fixed, what you did not fix and why.`, { label: 'Nachbesserung', phase: 'Nachbessern', schema: FIX_SCHEMA })

return { impl, findings: all, confirmed, fix }
