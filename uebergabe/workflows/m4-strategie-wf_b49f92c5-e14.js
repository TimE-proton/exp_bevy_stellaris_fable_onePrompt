export const meta = {
  name: 'm4-strategie',
  description: 'M4: Schiffe in Flotten, Werften, Nachschub, Unterhalt und Gefechte in der Partie; danach gegenpruefen und nachbessern',
  phases: [
    { title: 'Flotten und Werften', detail: 'Schiffe, Bauauftraege aus Marktguetern, Nachschub, Reparatur, Unterhalt' },
    { title: 'Gefechte in der Partie', detail: 'Beginn, Verstaerkung, Schritt je Takt, Rueckschreiben, Rueckzug, Kriegsfolgen' },
    { title: 'Pruefen', detail: 'drei Blickwinkel, jeder Befund wird gegengeprueft' },
    { title: 'Nachbessern', detail: 'bestaetigte Befunde beheben und committen' },
  ],
}

const DIR = '/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim'
const SCRATCH = '/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim'
const BASE = 'd883061'

const COMMON = `
You are a senior Rust engineer working on KONFLUENZ, an original single-player 4X strategy game (Rust 1.97.1, Bevy 0.20.1). It is an abstract strategy game about how economy, financial markets and conflict between communities are causally linked.
Workspace: crates/sim = deterministic simulation core without any rendering (Bevy plugins on FixedUpdate, 20 ticks per in-game day, 30 days per month, 360 days per year); crates/game = Bevy app (map, UI); crates/headless = command line over sim::harness. Game data lives in assets/data/*.ron.
Milestones 1-3 (galaxy, economy with local prices and freighters, stock market and cohesion with secession) are finished. Milestone 4 (fleets and space battles) is in progress: ship data and a tactical battle calculator exist, but the campaign still treats a fleet as a single strength number.

YOUR WORKING DIRECTORY (a git worktree on branch wip/m4-sim): ${DIR}
- Start every Bash command with: cd ${DIR} &&
- With the file tools use absolute paths below ${DIR}.
- The parent checkout /home/user/exp_bevy_stellaris_fable_onePrompt contains files with the same names and belongs to OTHER agents working in parallel. Never read, search or edit anything outside ${DIR} (except your scratch directory and the read-only cargo registry sources).
- Scratch directory for logs and temporary files: ${SCRATCH}
- Read ${DIR}/README.md first. It explains the architecture, the determinism rules, the economy and the ownership/cohesion model.

BINDING CONVENTIONS
1. Language: everything a human reads is GERMAN - code comments, doc comments, UI and chronicle strings, error messages, CLI help, commit messages. Identifiers are English as in the existing code. Comments are short and explain why. Use real umlauts. Match the style of the surrounding code.
2. Determinism (crates/sim): no iteration over HashMap/HashSet (use Vec or BTreeMap), nothing time- or thread-dependent, randomness only through crate::rng::Rng::stream(seed, Domain::X, a, b), sorting with total_cmp plus an id tie-break, loops in index order. All simulation state is plain serde data in Resources listed in the sim_state! macro in crates/sim/src/state.rs (that list defines save games and the state hash). The headless harness skips idle ticks: Headless::run_days and run_days_exact must give the same hash (see needs_fine_ticks in crates/sim/src/lib.rs).
3. Money is i64 milli-credits (type Money) and is conserved exactly: it is only moved, never created or destroyed (the only exception is explicitly booked 'minted' money). sim::state::money_drift(world) must stay 0; sim::harness::check_invariants checks it. Goods are physical: they are bought from the markets of the star systems and taken out of their stocks. Reuse the existing helpers for payments and purchases and study how other buyers are booked before you add a new one.
4. Player and AI act on the simulation only through SimCommand (crates/sim/src/command.rs).
5. Balance levers belong in assets/data/*.ron with serde structs in crates/sim/src/data/, validated when loaded - not as magic numbers in code.
6. Tests must be honest and meaningful. Never weaken an assertion just to make it pass; find the cause. No tautological tests.
7. Wording: neutral, abstract game-mechanics language everywhere (Gefecht, Verluste, Rückzug, ausgefallen). No graphic descriptions.
8. Git: you may commit inside your worktree only. Never push, never create pull requests, never run git stash / git reset / git clean / git checkout of other branches. Commit messages are German and end with exactly these two trailer lines:
   Co-Authored-By: <Modellname laut Sitzung> <noreply@anthropic.com>
   Claude-Session: https://claude.ai/code/session_013vNX3wKo7yPWrSdoFZ2sRA
9. Cargo: never judge success from output piped through tail/grep - redirect to a log file in the scratch directory and check the exit code ($?). Use generous timeouts (up to 10 minutes).
10. Format with: cargo fmt -p sim -p headless
11. Work autonomously to the end. Do not ask questions; decide, and record noteworthy decisions in your final report. Do not create planning or analysis documents in the repository.

OFF LIMITS: crates/sim/src/battle/** (the tactical battle calculator), assets/data/hulls.ron, modules.ron, designs.ron, techs.ron, the 'battle' and 'research' sections of assets/data/balance.ron, and crates/game/**. Use the battle calculator exactly as it is through its public API; do not change, tune or 'fix' it, even if you think you see a weakness - mention it in your report instead. The game crate is not built here (it needs a full engine build): keep every existing public name, field and signature that other crates may use and only add to the public API; in particular the fields Fleet.strength (f32) and Fleet.in_battle (bool) are read by the game crate and must stay.

REQUIRED FINAL STATE (all must succeed):
  cargo build -p sim
  cargo test -p sim
  cargo clippy -p sim -p headless --all-targets -- -D warnings
  cargo build --release -p headless
  target/release/headless sweep --seeds 0..8 --years 60     (no invariant violations)
  target/release/headless hash --seed 3 --days 3600          (run twice: identical output; and identical with --exact)
`

const S1 = COMMON + `
YOUR TASK (step 1 of 2): fleets made of ships, shipyards, supply and upkeep on the campaign level.

Read first: crates/sim/src/fleet/mod.rs and plugin.rs (today: Fleet with a strength number, movement along lanes, stances Guard/Blockade/Raid/Escort, blockade rule, a coarse daily skirmish rule), crates/sim/src/data/ships.rs and ship_loader.rs (HullDef, ModuleDef, DesignDef with DesignStats: value, parts, build_days, ammo, fuel, fighters, techs; MilitaryBalance), assets/data/designs.ron, hulls.ron, modules.ron and the 'military' section of assets/data/balance.ron (starting_fleet, upkeep_per_month, yard_capital_per_slot, repair_per_day, repair_parts, fuel_per_distance, dry_speed, ammo_goods, fuel_goods, hull_good, ammo_good - all defined but not used yet), crates/sim/src/ids.rs (ShipId, BattleId exist), crates/sim/src/economy/ (Market with stock, cash and prices per system; PlanetEconomy with capital per recipe and four sector firms including the arms sector; treasuries per community; households; how purchases and payments are booked), crates/sim/src/logistics/mod.rs (how freighters buy and sell on markets and how raids and escorts use fleet strength), crates/sim/src/finance/ (PolityFinance, how the state's monthly books work), crates/sim/src/command.rs, state.rs, harness.rs, ai/.

Design (the public names are fixed; later steps and the user interface rely on them):
  pub struct Ship { pub id: ShipId, pub design: u16, pub hull: f32, pub modules: Vec<f32>, pub ammo: f32, pub fuel: f32, pub fighters: u32 }
      hull, ammo and fuel are fractions 0..1 of the design's maximum; modules holds the condition 0..1 of each module of the design in design order, an empty vector means all intact; fighters is the number of carrier-borne craft on board.
  Fleet gains: pub ships: Vec<Ship>, pub formation: crate::battle::Formation, pub doctrine: crate::battle::Doctrine. Fleet.strength stays a public f32 field and becomes a cached combat value derived from the ships (build value times condition, scaled so that a starting fleet is of the order of 100); refresh it whenever ships change. Fleet.in_battle stays.
  pub struct BuildOrder { pub id: u32, pub polity: PolityId, pub planet: PlanetId, pub design: u16, pub progress: f32, pub spent: Money, pub stalled: bool, pub started: u64 }
  Fleets gains: pub orders: Vec<BuildOrder> plus counters for ship and order ids (and whatever else you need, all serializable).
Rules:
1. Start. Every community starts with the fleet from balance.military.starting_fleet (designs and counts) at its capital, fully supplied.
2. Shipyards. A planet can build ships if its arms sector has capital; the number of building slots is that capital divided by yard_capital_per_slot (at least one slot on a capital world). SimCommand::BuildShip { planet, design } queues an order (validation: own planet with a yard, design exists, is not a carrier-borne craft, and all technologies it requires are known - write a helper that takes the list of known technologies as a slice; until the research system is merged from another branch pass an empty slice, so only designs without requirements can be built; a sensible limit on the queue per planet). Every day the orders of a planet advance in id order, at most as many at once as there are slots: for the day's share of the design's parts (DesignStats.parts over build_days) the state buys the goods on the market of the planet's star system at the local price and pays from its treasury; in addition a yard fee (a share of the parts value, parameter in balance.ron) goes to the planet's arms-sector firm as revenue, so shipyards earn from state orders and their shareholders with them. If a good is not in stock or the treasury is empty, the order advances only as far as the scarcest input allows and is marked stalled: a blockaded or badly supplied yard stands still. A finished ship joins a fleet of the same community docked in that system (lowest id, not in a battle), otherwise a new fleet is created there. SimCommand::CancelBuild { order } removes an order without refund.
3. Supply. A fleet docked in a system that belongs to its own community and is not blockaded restocks ammunition and fuel a little every day (rate in balance.ron) by buying the goods (ammo_good and the fuel good; ammo_goods and fuel_goods give units of the good per point of on-board capacity) from the local market, paid by the treasury; if the market is short it gets less. Elsewhere there is no resupply. Travelling consumes fuel (fuel_per_distance per unit of lane length); a fleet without fuel moves at dry_speed of its normal speed.
4. Repair. Docked at an own system that has a yard, damaged ships regain repair_per_day of hull and module condition per day and replace lost carrier-borne craft; the hull good needed (repair_parts times the hull's segments per full repair, proportionally) is bought from the market; without the goods nothing is repaired.
5. Upkeep. Once a month every community pays upkeep_per_month of the build value of its ships as crew pay from the treasury to the households of its planets (reuse the way the state already pays households; exact conservation). If the treasury cannot cover it, it pays what it has and the unpaid share is recorded per community as arrears (0..1, smoothed, serializable, public) with a chronicle entry when arrears begin; a later step uses arrears for crew morale.
6. Commands (append the new variants at the END of SimCommand and of the match in execute, to keep merges with other branches clean): BuildShip, CancelBuild, MergeFleets { from, into } (same owner, both docked in the same system, neither in a battle), SplitFleet { fleet, ships: Vec<ShipId> } (docked, not in a battle, cannot take every ship; the new fleet appears in the same system), SetFleetOrders { fleet, formation, doctrine } (standing orders for future battles). All with validation and German error messages like the existing commands.
7. The coarse daily skirmish rule (resolve_skirmishes) will be replaced entirely in step 2. For now keep it working provisionally on ships (spread its damage over the hulls, remove ships at zero hull, a fleet without ships is gone) and adapt the existing unit tests honestly.
8. Raids, escorts and blockades keep using Fleet.strength.
9. Invariants and tooling: extend sim::harness::check_invariants (ship fractions finite and within 0..1, living fleets have at least one ship, unique ship ids, strength finite) and the run summary (fleets and ships per community); add a 'fleets' subcommand to the headless program that lists fleets, ships, supply state and build orders for a seed after N years (put it in a new file crates/headless/src/fleets.rs and touch main.rs minimally). Upkeep and state orders change public finances: run the sweep before and after, compare treasuries, bond yields and cohesion, and adjust the parameters of the 'military' section of balance.ron if the starting fleets would ruin the states; report both tables.

TESTS (new file crates/sim/tests/military.rs, plus unit tests next to the code):
  1. Starting fleets match the data; strength follows the ships' value and condition.
  2. Building: an order makes goods leave the local market and money move from the treasury to the market and to the yard's firm, exactly conserved (money_drift == 0); the ship appears no earlier than build_days; more slots build more ships in parallel.
  3. A yard stands still when a required good is missing from the market or the treasury is empty, the order is marked stalled, and it resumes when supply returns. Also show it through a blockade if you can set one up with the existing commands.
  4. Supply: a fleet with empty magazines and tanks restocks at home from the market (stock falls, treasury pays) and does not restock in a foreign or blockaded system; travelling consumes fuel; a fleet without fuel is slower.
  5. Repair needs a yard and goods.
  6. Upkeep moves exactly the stated amount from the treasury to households once a month; an empty treasury produces arrears.
  7. Merge, split, standing orders and cancel commands, each with the rejected cases.
  8. Determinism: a scripted game with build orders gives the same hash twice; run_days equals run_days_exact; a save in the middle of a build loads and continues identically.

When everything is green, commit in the worktree (git add -A; git commit) with a German message.

FINAL REPORT (structured output): summary; measurements (sweep comparison before/after, typical upkeep against state revenue); api_notes (exact public structs, fields, functions and commands with their semantics, for the engineer who does step 2 and for the user interface); results of the required commands; deviations with reasons; open issues.
`

const S2_HEAD = COMMON + `
YOUR TASK (step 2 of 2): battles inside the campaign.

Step 1 has just been done by another engineer in this worktree (see 'git log -1' and their notes below): fleets consist of ships, shipyards build them from market goods, fleets are supplied, repaired and paid. The tactical battle calculator exists in crates/sim/src/battle/ and so far is only used for practice battles (crates/sim/src/battle/matchup.rs shows how to set up and run one). Your job is to make it the way battles are fought in the campaign and to replace the coarse daily skirmish rule completely.

Read first: crates/sim/src/battle/mod.rs (Battle, BattleShip with source/status/hull/modules/ammo/fuel/hangar/carrier, Combatant with its bookkeeping initial/lost/escaped/initial_value/lost_value/retreating, ShipSpec, CombatantSetup, CombatModifiers, Battle::new(id, system, seed), add_combatant(data, setup, ships) which also works in the middle of a battle, set_hostile, set_orders, order_retreat, finished, timed_out, victors(), is_fighting, active_by_class, fighting_value) and step.rs (Battle::step(data) advances 0.1 s of battle time - call it once per simulation tick), matchup.rs, the 'battle' section of assets/data/balance.ron (morale_start, morale_per_weariness ...), then the fleet module as it is now, diplomacy/mod.rs (at_war), finance/ (PolityFinance.war_weariness 0..100 and how it already feeds cohesion and bond yields; finance::shares::shock_polity), chronicle.rs, lib.rs (SimSet order, needs_fine_ticks), harness.rs, state.rs, command.rs.

Design (public names fixed; the user interface will rely on them):
  In the fleet module (for example crates/sim/src/fleet/battles.rs):
  #[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)] pub struct Battles { pub active: Vec<Battle>, pub reports: Vec<BattleReport>, pub debris: Vec<Debris>, .. }   registered in sim_state! directly after 'fleets: Fleets,' and created in SimState::generate
  pub struct BattleReport { pub id: BattleId, pub system: SystemId, pub started: u64, pub ended: u64, pub timed_out: bool, pub sides: Vec<SideReport> }
  pub struct SideReport { pub owner: PolityId, pub fleet: Option<FleetId>, pub fleet_name: String, pub initial: [u32; 6], pub lost: [u32; 6], pub escaped: [u32; 6], pub initial_value: f32, pub lost_value: f32, pub held_field: bool, pub retreated: bool }
  pub struct Debris { pub system: SystemId, pub amount: f32, pub tick: u64 }
  impl Battles { pub fn in_system(&self, system: SystemId) -> Option<&Battle>; pub fn of_fleet(&self, fleet: FleetId) -> Option<(&Battle, usize /* side index */)>; pub fn get(&self, id: BattleId) -> Option<&Battle> }
  Fleet gains pub battle: Option<BattleId> (keep in_battle in sync) and whatever it needs to remember where it came from and until when it regroups.
Rules:
1. Beginning. At the start of a day, in every system where docked fleets of communities that are at war with each other stand (Diplomacy::at_war) and that are not already in a battle or regrouping, a battle begins with all fleets present whose owner is at war with at least one other owner present. One combatant per fleet, in a deterministic order; hostility between combatants follows the war matrix; the fleet's standing formation and doctrine apply; crew morale starts at balance.battle.morale_start reduced by the owner's war weariness (morale_per_weariness) and by pay arrears; every ship goes in with its current condition, supplies and craft (ShipSpec with source = its ShipId). The battle's seed derives from the game seed and the battle id through Rng::stream. Technology modifiers stay neutral for now - leave one clearly marked place where CombatModifiers will later be filled from the research system.
2. Reinforcements. A fleet that is docked in a system with a running battle, is not regrouping and is at war with a participant joins at the next start of a day through add_combatant.
3. Progress. Every simulation tick each running battle advances by one step, in index order. While any battle runs, needs_fine_ticks must be true so that headless runs do not skip ticks; prove with a test that run_days and run_days_exact give the same hash during a war.
4. Orders. SetFleetOrders also changes a running battle (Battle::set_orders); a new SimCommand::Retreat { fleet } orders the fleet's side to leave the field (Battle::order_retreat); MoveFleet is rejected for a fleet in a battle with a German message that says to order retreat first. Append new command variants at the END of SimCommand.
5. End. When a battle is finished, write the result back from the battle's public state: ships that were lost are removed (lost = destroyed, or adrift at the end on a side that does not hold the field); every other ship keeps its hull, module condition, ammunition and fuel as fractions of the design's maximum; surviving carrier-borne craft return to carriers of their own side up to each design's capacity, the rest are lost. A fleet without ships ceases to exist. Fleets of a side that retreated or left the field move out: to the system they came from if it is adjacent, otherwise to an adjacent system of their own community, otherwise to any adjacent system without hostile fleets, otherwise to the adjacent system with the lowest id; they travel there normally and regroup for a number of days (parameter) during which they neither begin nor join battles and do not count for blockade, raid or escort. Fleets that hold the field stay. If the battle ran into its time limit nobody leaves, and that system gets a pause (parameter, in days) before a new battle may begin. Make sure there is no endless loop of instant re-battles.
6. Aftermath. Record a BattleReport (keep the newest N) and a debris entry for the system (value of the lost ships; it fades over the years; for now it is information for the map). Losses raise the owner's war weariness in proportion to the share of its fleet value lost (parameters in the 'military' section of balance.ron; bounded 0..100; check that weariness also decays in peace and add that if it is missing), a heavy defeat shocks the owner's share prices through the existing finance function, and the chronicle gets German entries for the beginning (who, where), reinforcements, and the end (who holds the field, ships lost per side). Fleets in a battle or regrouping do not blockade, raid or escort.
7. Remove the coarse daily skirmish rule and every parameter that is unused afterwards (RON and structs consistent).
8. Tooling: the run summary and the sweep table get the number of battles and ships lost; give 'headless run' an option to script wars for testing, for example --war YEAR:A:B (repeatable), implemented with ordinary DeclareWar commands; with it, look at a few games with wars and check that the chronicle reads sensibly.

TESTS (extend crates/sim/tests/military.rs):
  1. No battle without war. When fleets of communities at war meet in a system, a battle begins at the next start of a day, runs to its end, and the result is written back: lost ships are gone, survivors carry their damage and used supplies, craft are back on board only up to capacity, a fleet without ships is gone, the chronicle has entries for beginning and end.
  2. A clearly superior fleet holds the system; the inferior side's survivors leave to a neighbouring system and regroup; no immediate re-battle.
  3. Reinforcements that arrive in the middle join the battle and help their side (compare with the same battle without them over several seeds).
  4. Costs feed back: losses raise war weariness; upkeep and resupply draw on the treasury; money is conserved exactly through a whole war (money_drift == 0); a heavy defeat moves share prices.
  5. Orders: SetFleetOrders reaches the running battle; Retreat makes the fleet leave with its survivors; MoveFleet during a battle is rejected.
  6. Determinism: the same scripted war twice gives the same hash; run_days equals run_days_exact while a battle is running; a save in the middle of a battle, loaded and continued, ends in exactly the same state as the uninterrupted game.
  7. A fleet in a battle or regrouping does not blockade; a battle that reaches the time limit ends without anyone leaving and does not restart at once.
  8. Three communities at war in one system and a battle whose participants include a community that is extinguished or changes through secession during the battle do not crash and end properly.

When everything is green, commit in the worktree (git add -A; git commit) with a German message.

FINAL REPORT (structured output): summary; measurements (typical length of campaign battles in days, what a scripted war does to treasuries, weariness and cohesion); api_notes (exact public API of Battles, reports, debris, fleet fields and commands, and what a user interface needs to show a running battle and to send orders; where the research system's combat modifiers and the fleet speed bonus have to be plugged in); results of the required commands; deviations with reasons; open issues - including anything you noticed about the battle calculator that you were not allowed to change.
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
You are reviewing code in the game project KONFLUENZ (Rust, deterministic simulation core of an abstract strategy game). You do NOT edit any file in the repository; you only read, run existing commands and report.
Directory (git worktree): ${DIR} - start every Bash command with 'cd ${DIR} &&' and use absolute paths below it. Never touch the parent checkout.
Scratch directory for your own logs: ${SCRATCH}
Subject: the campaign-level military layer added on this branch: fleets made of ships, shipyards and build orders, supply, repair, upkeep, and battles inside the campaign (begin, reinforcements, one step per tick, writing results back, retreat and regrouping, aftermath). See 'git diff ${BASE} --stat' and 'git diff ${BASE}' (exclude nothing; the tactical battle calculator in crates/sim/src/battle/ is NOT under review and is used as it is). Read crates/sim/src/fleet/ completely, every hook in other files, crates/sim/tests/military.rs, the new headless subcommands and the 'military' section of assets/data/balance.ron. README.md explains the project.
You may run: cargo test -p sim; cargo build --release -p headless; target/release/headless run --seed 3 --years 30 --war ... (see --help); target/release/headless fleets ...; target/release/headless sweep --seeds 0..8 --years 60; target/release/headless hash --seed 3 --days 3600 [--exact]. Redirect cargo output to a log file and check the exit code.
Project rules that matter: determinism (no HashMap iteration, randomness only via Rng::stream, state only in Resources listed in sim_state!, idle-tick skipping must not change results, a save in the middle of a battle must continue identically); money is conserved exactly (money_drift == 0) and goods are physical (bought from market stocks); player and AI act only through SimCommand; balance levers live in RON files; existing public names used by the game crate (Fleet.strength, Fleet.in_battle and everything that existed before) must still exist; comments and strings are German.
Specification for this part: ships and weapons are the end of the production chains; a blockade lets prices rise and factories (and yards) stand still; supply with ammunition and fuel matters; war costs feed back into economy and cohesion; simulation deterministic with the same seed.
Report only real, concrete problems (file, line, failure scenario). No style nitpicks, no speculation without a scenario.
`

const LENSES = [
  {
    key: 'korrektheit',
    prompt: REVIEW_COMMON + `
YOUR LENS: correctness, money and goods. Follow every payment and every purchase line by line: is the sum of money conserved in all branches (rounding of milli-credits, partial purchases when stock or treasury is short, yard fee, upkeep with arrears, repair, resupply, cancelled orders, orders of a community that loses the planet or ceases to exist)? Do goods really leave the market stock, never go negative, never get bought twice? Then the lifecycle: fleets and ships with stale ids after merge, split, loss of the last ship or a finished battle; a fleet that is moving, regrouping or in a battle receiving commands; build orders finishing while the system is contested; results written back from a battle (which ships count as lost, fractions within 0..1, carrier-borne craft returned only up to capacity, modules mapped to the right design slots); retreat with no adjacent system that qualifies; three-sided battles; reinforcement handling; battles in systems whose owner changes; communities founded by secession while a battle runs. Look for index-out-of-bounds, division by zero and endless battle loops.`,
  },
  {
    key: 'determinismus',
    prompt: REVIEW_COMMON + `
YOUR LENS: determinism, state and integration. Is every piece of state that influences the future serializable and inside a Resource registered in sim_state! (and created in SimState::generate)? Is the tick order sensible and unambiguous (which systems run every tick, which only at day or month starts, and in which SimSet)? Does needs_fine_ticks cover running battles, and do run_days and run_days_exact agree during a war (run the hash command and the tests; build a scripted war with --war and compare)? Does a save in the middle of a battle or a build order continue identically? Are battle seeds derived deterministically and independent of unrelated events? Are the new commands validated, logged and replayable like the existing ones? Does anything iterate in an unstable order? Could the changes break the game crate, which was not built here (compare the public API before and after: removed or renamed public items, changed field types, new required fields in structs the game constructs)? Are new RON parameters validated at load with clear German errors?`,
  },
  {
    key: 'anforderungen',
    prompt: REVIEW_COMMON + `
YOUR LENS: requirements, feedback loops and honesty of the tests. Check that the causal links exist and are measurable: ships need goods from the markets, so shortages and blockades stop yards; fleets need ammunition and fuel, so long campaigns far from home run dry; upkeep, orders and losses weigh on the treasury, war weariness, share prices and through them cohesion. Run a few scripted wars with the headless program and report what actually happens to treasuries, weariness, cohesion and prices; is any link missing, negligible or absurdly strong? Is the economy still healthy in the sweep? Then read crates/sim/tests/military.rs critically: is each test meaningful, could it pass while the property is false, which required property has no test? Are balance levers hard-coded that belong in balance.ron? Is the public API (Battles, BattleReport, Debris, Fleet fields, the commands) sufficient for a user interface that lists fleets and ships, shows build queues and supply, marks running battles on the map, shows a running battle and sends orders?`,
  },
]

phase('Flotten und Werften')
const s1 = await agent(S1, { label: 'Schiffe, Werften, Nachschub', phase: 'Flotten und Werften', schema: IMPL_SCHEMA })
if (!s1) {
  return { error: 'Schritt 1 lieferte kein Ergebnis' }
}
log('Schritt 1 fertig: ' + s1.summary.slice(0, 300))

phase('Gefechte in der Partie')
const s2 = await agent(S2_HEAD + `
NOTES FROM STEP 1 (the public API you build on; check it against the code):
${s1.api_notes}
Open issues named in step 1: ${JSON.stringify(s1.open_issues)}
`, { label: 'Gefechte in der Partie', phase: 'Gefechte in der Partie', schema: IMPL_SCHEMA })
if (!s2) {
  return { s1, error: 'Schritt 2 lieferte kein Ergebnis' }
}
log('Schritt 2 fertig: ' + s2.summary.slice(0, 300))

const reviewed = await pipeline(
  LENSES,
  lens => agent(lens.prompt + `

What the implementers reported (claims to check, not facts):
Step 1: ${s1.summary}
Step 2: ${s2.summary}
API notes step 1: ${s1.api_notes}
API notes step 2: ${s2.api_notes}
Open issues named: ${JSON.stringify((s1.open_issues || []).concat(s2.open_issues || []))}`, { label: 'Pruefung: ' + lens.key, phase: 'Pruefen', schema: FINDINGS_SCHEMA }),
  (review, lens) => {
    const findings = (review && review.findings) || []
    return parallel(findings.map((f, i) => () =>
      agent(REVIEW_COMMON + `
YOUR JOB: check one reported finding independently. Try to REFUTE it: read the code yourself, and where it is cheap run an existing test or the headless tool to see whether the claimed failure really happens. Answer real=true only if you are convinced the problem exists and matters (wrong results, lost or created money or goods, nondeterminism, a crash or endless loop, a requirement not met, a broken public API, a missing or meaningless required test). Answer real=false if it is wrong, purely cosmetic, or already handled elsewhere.

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
  return { s1, s2, findings: all, confirmed, fix: null }
}
const fix = await agent(COMMON + `
YOUR TASK: the campaign-level military layer (crates/sim/src/fleet/** and its hooks, see 'git diff ${BASE}' and 'git log -3') was implemented in two steps and then reviewed. Independent checkers confirmed the findings below. Fix every one of them properly (cause, not symptom) and add or strengthen tests where a finding shows a gap. Keep the public API stable unless a finding requires a change; report any change.

API notes of the implementers:
Step 1: ${s1.api_notes}
Step 2: ${s2.api_notes}

CONFIRMED FINDINGS:
${confirmed.map((f, i) => (i + 1) + '. [' + f.severity + '] ' + f.file + (f.line ? ':' + f.line : '') + ' - ' + f.title + '\n   ' + f.description + '\n   Scenario: ' + f.failure_scenario + '\n   Suggested fix: ' + (f.suggested_fix || '-') + '\n   Checker: ' + (f.verdict.reasoning || '')).join('\n')}

Finish with all required commands green and a commit in the worktree (German message with the two trailer lines). Report what you fixed, what you could not fix and why, and the final API notes for the user interface and for the engineers who will add the military AI and plug in the research system.`, { label: 'Nachbesserung', phase: 'Nachbessern', schema: FIX_SCHEMA })

return { s1, s2, findings: all, confirmed, fix }
