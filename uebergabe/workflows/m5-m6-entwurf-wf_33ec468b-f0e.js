export const meta = {
  name: 'm5-m6-entwurf',
  description: 'Entwurfsrunde fuer Meilenstein 5 (Belagerung) und 6 (KI, Diplomatie, Enden): je drei unabhaengige Entwuerfe, Jury, Synthese',
  phases: [
    { title: 'Entwerfen', detail: 'je drei Entwuerfe aus verschiedenen Blickwinkeln' },
    { title: 'Bewerten', detail: 'zwei Juroren je Meilenstein' },
    { title: 'Zusammenfuehren', detail: 'eine umsetzbare Spezifikation je Meilenstein' },
  ],
}

const DIR = '/home/user/exp_bevy_stellaris_fable_onePrompt'

const CONTEXT = `
You are a senior game systems designer and Rust engineer. Project: KONFLUENZ, an original single-player 4X strategy game in a finite galaxy (Rust, Bevy 0.20). The guiding idea: economy, financial markets and war are causally linked in a living simulation, and in the end exactly one galactic community prevails - by merger or by extinction.
Repository: ${DIR}. You are READ-ONLY: do not create, edit or delete any file, do not run cargo, do not look into ${DIR}/.worktrees/ or ${DIR}/target. Read README.md first, then the code you need: crates/sim/src/lib.rs (SimSet = the fixed order of a tick), state.rs (sim_state! = what is saved and hashed), command.rs (SimCommand, the only way player and AI act), galaxy/ (systems, lanes, chokepoints, planets with deposits, gravity, atmosphere, climate, population), polity/, economy/ (production per planet and recipe, markets per system, households, treasuries, taxes), logistics/ (trade houses, freighters on real routes), finance/ (issuers, holders, shares, bonds, cohesion per planet, secession - cohesion.rs is important), fleet/, diplomacy/ (currently only a war matrix), ai/ (currently only colonisation and finance stewardship), battle/ (tactical resolver), harness.rs (headless runs, invariants, sweeps), data/ and assets/data/*.ron (balance.ron, polities.ron, goods.ron, recipes.ron).

What exists (milestones 1-3, finished): procedural galaxy with clusters and chokepoints; multi-stage production chains with local prices from supply and demand; physical freighters that can be raided, blockaded and escorted; companies, shipyards, trade houses and system trading posts that issue shares; state, regional elites and population funds as holders; dividends, bonds, takeovers, expropriation; cohesion per planet driven by who owns what (broad mutual ownership binds, concentration, one-sided profit outflow or foreign majorities drive autonomy up to secession, with regions joining the foreign community that holds the majority).
What is being implemented right now by other engineers (milestone 4; exact names may differ slightly): fleets consist of ships (crate::fleet::Fleet { ships: Vec<Ship>, stance, formation, doctrine, system, route, strength, .. }; Ship { design, hull, modules, ammo, fuel, fighters }); ships are built in yards on planets with arms-sector capital from goods bought on the local market; fleets buy ammunition and fuel from markets while docked at home and cost monthly upkeep; when fleets of communities at war meet in a system a tactical battle (crate::battle::Battle) runs tick by tick, with ship classes fighter, corvette, frigate, cruiser, battleship, carrier, modules, formations, morale and retreat; losses raise war weariness (finance.polities[p].war_weariness, 0..100), which lowers cohesion and raises bond yields; Fleets::is_blockaded(system) - a fleet with stance Blockade and orbital superiority cuts a system off from all freighters, prices rise and factories idle; technology (crate::tech::Research) gives per community TechEffects { ground_attack, ground_defense, weapon_damage, shields, armor, hull, .. } and only a partly visible picture of others (estimate() with ranges, an intel value 0..1 per pair of communities).

Hard constraints for any design:
- Simulation core only (crates/sim): plain serde data in Resources registered in sim_state!, systems in the fixed SimSet order (there are slots Siege, Aftermath, Victory, Diplomacy, Ai), deterministic (no HashMap iteration, randomness only via Rng::stream(seed, Domain, a, b)), daily or monthly steps gated by the clock; headless runs skip idle ticks.
- Money is conserved exactly (i64 milli-credits, only moved). Goods are physical: they come from markets and stocks.
- Player and AI act only through SimCommand. AI decisions run inside simulation systems in SimSet::Ai.
- Balance levers in RON data files, validated at load.
- Everything must be testable headless, and AI-versus-AI games over many seeds are the balancing tool.
- All player-facing texts are German; the design document itself may be English. Use neutral, abstract game-mechanics language throughout (no graphic descriptions).
- It has to be implementable by one strong engineer in one or two focused sessions per subsystem: causal completeness and robustness beat breadth. Prefer a few state variables with clear feedback loops over many special cases.
`

const M5_SPEC = `
MILESTONE 5 - PLANETARY SIEGE. Specification (binding):
A planetary siege proceeds in phases:
 (1) Orbital superiority and blockade.
 (2) Planetary defence: orbital guns on the ground and stations can also fire at capital ships; planetary shields prevent bombardment.
 (3) Bombardment with a trade-off: it destroys the economy one wants to conquer.
 (4) Landing and ground war over regions, cities and shield generators.
 (5) Underground combat (bunkers, tunnels, deep facilities), where orbital support has no effect, the defenders have the advantage and sieges take long; starving the planet out by blockade is the alternative.
War costs feed back into economy and cohesion.
Planned tests: a shield blocks bombardment; underground combat gets no orbital support; starving out ends; bombardment lowers the value of the prize. A siege AI must exist so AI communities can attack and defend planets.
The game application will later need a siege view (planet with regions, orbit, shield, forces, supplies) and controls (bombardment level, landing, assault or wait); your design must say what data and commands that needs.
Design question to answer fully: data structures (planet defences, regions of a planet, ground forces and how they are raised, transported and supplied, siege state), the daily algorithms of every phase with formulas and parameters, how each phase interacts with economy (population, capital stock, markets, supply), finance (share prices, war weariness, bonds, cohesion of the conquered planet and of the attacker's home regions), fleets (ships in orbit take fire from orbital guns; ammunition use for bombardment and orbital support), technology (ground_attack, ground_defense), what happens on conquest (ownership change, existing shareholdings, cohesion of a freshly conquered region, loot versus damage), commands, AI behaviour for attacker and defender, tests, and the risks.
`

const M6_SPEC = `
MILESTONE 6 - AI COMMUNITIES, DIPLOMACY, HIDDEN DISPOSITIONS, ENDINGS. Specification (binding):
Leitmotiv: several communities with their own values, goals and temperaments. Every AI has a hidden inner disposition (cooperative, opportunistic, paranoid, expansionist) that can only be inferred from behaviour, intelligence and trade data, and that can change under pressure. Coexistence is unstable. In the end there are exactly two outcomes: Metamorphosis (merger through interdependence, shareholdings, treaties and cultural convergence) or Extinction (one community destroys or subjugates all others, or all destroy each other and everybody loses).
Securities also act outward: stakes in foreign communities create influence, dependency and a peaceful path to merger.
Planned tests: every game ends with exactly one ending; sweeps over many seeds; the hidden disposition is never exposed to the player except through evidence.
Starting point from the approved architecture plan (improve on it where you can): weights of the dispositions in a RON file; the core of the AI is a trend calculation - who expects to be overtaken tends towards a preventive strike or towards flight into merger; treaties: trade, non-aggression, research, investment, defence, union; the player sees behaviour, estimates with uncertainty and trade data, and an analysis window shows the assessment derived from them, never the truth; merger needs a union treaty, mutual shareholdings above a threshold and cultural proximity, and creates a new community; subjugated communities no longer count as sovereign; if one sovereign remains the ending is Metamorphosis when the larger part of the absorbed population came by merger, otherwise Extinction; if the galaxy falls below a survival threshold everybody loses; if the player is eliminated the game is lost for the player; depleting deposits provide end pressure; a year limit exists only in sweeps, where hitting it is reported as a balance defect.
Design question to answer fully: data structures (relations, treaties, culture, disposition with hidden state and its dynamics, evidence and assessment), the strategic AI (economy, fleet building, war and peace decisions, treaty decisions, investment in foreign shares, reaction to the player), diplomacy commands and how proposals between player and AI work, how war aims, peace terms and subjugation work, the exact merger and ending rules with a convincing argument why every game terminates with exactly one ending within a plausible time and why coexistence is unstable without feeling scripted, how the player can infer dispositions (what evidence, what the analysis window shows), tests, sweep metrics, and the risks. The military and siege layers exist as described; the AI must use them.
`

const DESIGN_FORMAT = `
Deliver ONE design document as your final answer (markdown, no preamble, at most about 2200 words), with these sections: 1. Core idea in five sentences. 2. State (Rust-like struct sketches with field meanings and units). 3. Algorithms per step (daily/monthly), with formulas and named parameters and proposed default values. 4. Commands (SimCommand variants) and their validation. 5. AI behaviour (decision rules). 6. Feedback into economy, finance and cohesion (name the existing fields and functions you hook into, with file names). 7. What the user interface needs to show and control. 8. Tests (concrete, falsifiable). 9. Risks and how the design contains them. Be specific: name files, structs and functions that exist in the repository where you connect to them.
`

const ANGLES = {
  m5: [
    { key: 'kausalitaet', text: 'Your angle: causal linkage. Make every phase of the siege run through the physical economy and the ownership model that already exist (stocks in markets, population, capital stock, shares, cohesion, war weariness). The siege should be a consequence of those systems, not a separate mini-game.' },
    { key: 'entscheidungen', text: 'Your angle: interesting decisions for attacker and defender. Each phase should confront both sides with a real trade-off (time versus damage, bombardment versus prize, assault versus starving out, relief fleet versus home defence), with clear information for the player and few but meaningful controls.' },
    { key: 'robustheit', text: 'Your angle: robustness and implementability. Design the smallest state machine that still satisfies every sentence of the specification, guarantees that every siege ends, cannot deadlock or oscillate, is cheap per tick, easy to test and easy for an AI to play competently.' },
  ],
  m6: [
    { key: 'emergenz', text: 'Your angle: emergence. Derive AI behaviour from a small number of drives and forecasts (power trend, security, wealth, interdependence) modulated by the hidden disposition, so that preventive wars, alliances, dependency through shareholdings and mergers arise from the simulation rather than from scripts.' },
    { key: 'lesbarkeit', text: 'Your angle: the player experience of reading hidden minds. Focus on evidence, intelligence, trade data and the analysis window: what the player can observe, how dispositions leave fingerprints in behaviour, how dispositions shift under pressure in a way the player can notice, and how diplomacy with such partners stays fair and comprehensible.' },
    { key: 'termination', text: 'Your angle: guaranteed, satisfying endings. Design the dynamics so that coexistence is unstable, every game ends with exactly one of the two endings within a plausible time (give the argument), merger is a real peaceful path through shareholdings, treaties and cultural convergence, and AI-versus-AI sweeps can measure and tune all of it.' },
  ],
}

const JUDGE_SCHEMA = {
  type: 'object',
  properties: {
    scores: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          design: { type: 'string', enum: ['A', 'B', 'C'] },
          score: { type: 'number' },
          strengths: { type: 'string' },
          weaknesses: { type: 'string' },
        },
        required: ['design', 'score', 'strengths', 'weaknesses'],
      },
    },
    best_base: { type: 'string', enum: ['A', 'B', 'C'] },
    grafts: { type: 'string', description: 'Ideen aus den anderen Entwuerfen, die uebernommen werden sollten' },
    gaps: { type: 'string', description: 'Was in allen drei Entwuerfen fehlt oder falsch ist' },
  },
  required: ['scores', 'best_base', 'grafts', 'gaps'],
}

const JUDGES = [
  { key: 'spezifikation', text: 'Judge as the keeper of the specification and of the game as a whole: does the design satisfy every sentence of the specification, are the causal links to economy, finance and cohesion real, is it fun and understandable for the player, does it fit the guiding idea?' },
  { key: 'umsetzung', text: 'Judge as the engineer who has to build and test it in this code base: does it fit the existing architecture and files (check the repository), is it deterministic and money-conserving, can it deadlock or fail to terminate, is the state minimal, are the tests falsifiable, how risky is the balance, how much work is it?' },
]

async function round(milestone, spec, title) {
  const designs = await parallel(ANGLES[milestone].map(a => () =>
    agent(CONTEXT + spec + '\n' + a.text + '\n' + DESIGN_FORMAT, { label: title + ' Entwurf: ' + a.key, phase: 'Entwerfen', agentType: 'Plan' })))
  const usable = designs.map((d, i) => ({ d, i })).filter(x => x.d)
  if (usable.length === 0) {
    return { error: 'keine Entwuerfe' }
  }
  const letters = ['A', 'B', 'C']
  const bundle = usable.map(x => '===== DESIGN ' + letters[x.i] + ' (angle: ' + ANGLES[milestone][x.i].key + ') =====\n' + x.d).join('\n\n')
  const verdicts = (await parallel(JUDGES.map(j => () =>
    agent(CONTEXT + spec + `
Three engineers designed this milestone independently. ${j.text}
Score each design from 0 to 10, name concrete strengths and weaknesses, choose the best base, list the ideas from the other designs that should be grafted onto it, and name what all of them miss or get wrong. Check claims about the existing code against the repository.

${bundle}`, { label: title + ' Jury: ' + j.key, phase: 'Bewerten', schema: JUDGE_SCHEMA, agentType: 'Plan' })))).filter(Boolean)
  const final = await agent(CONTEXT + spec + `
Three engineers designed this milestone independently and two judges evaluated the designs. Write the FINAL implementation specification: start from the base the judges prefer, graft the best ideas of the others, close the gaps the judges found, and resolve every contradiction. Check every reference to existing code against the repository so that file, struct, field and function names are real. The result will be handed verbatim to the engineers who implement it, so it must be complete, unambiguous and buildable; keep it tight (at most about 3200 words).
Use these sections: 1. Core idea. 2. State (Rust-like structs, units, where they are registered). 3. Tick order and algorithms per step with formulas and parameters (names and default values for the RON files). 4. Commands with validation rules. 5. AI behaviour as decision rules. 6. Hooks into existing systems (exact files and functions). 7. Public API the user interface needs (what to show, which commands to send). 8. Tests (concrete and falsifiable) and sweep metrics. 9. Implementation order in 4-8 steps that each leave the project compiling and tests green. 10. Risks and safeguards.

JUDGES:
${JSON.stringify(verdicts, null, 1)}

${bundle}`, { label: title + ' Synthese', phase: 'Zusammenfuehren', agentType: 'Plan' })
  return { final, verdicts, designs: usable.map(x => ({ angle: ANGLES[milestone][x.i].key, text: x.d })) }
}

phase('Entwerfen')
const [m5, m6] = await parallel([
  () => round('m5', M5_SPEC, 'M5'),
  () => round('m6', M6_SPEC, 'M6'),
])
return { m5, m6 }
