# M5 Kern: Belagerung auf neuem Kern

## summary

The siege work (wip/m5-rechner 8d0023b) is merged onto the new core as a real merge commit on wip/m5-kern, its second half is finished, and all standard checks pass on the pushed final commit b77a131.

A scripted siege in a whole game now runs through all five phases (approach, investment, bombardment, ground war, underground) to the hand-over of an ordinary world, with money drift 0 and invariants checked on every day.

AI-only play is unchanged: both sweeps (peaceful and grenzkrieg, seeds 0..8, 60 years) match the main-branch release binary in all 41 common CSV columns; only the hash differs.

Three decisions the merge forced on me that you should know:
- **Fuel in orbit.** The new fleet layer already charges every docked fleet `military.fuel_per_day` (0.004). The siege charged its own `station_fuel` on top, so besiegers burned double. The siege no longer burns fuel in the world; `siege.station_fuel` is kept for the forecast only, is now 0.004, and loading fails if the two differ.
- **Relief battles.** The new blockade rule keeps a system closed while the blockading fleet still fights. The siege is still lifted the moment the battle starts. I kept both rules and changed the relief test to expect the system closed; the two rules are not consistent with each other (see open issues).
- **Supply for landings.** On the new core the attacker's home market holds too little munition after two years (32 of 210 unit-days bought), so the storm tests failed. The tests that land troops now put the supply goods on the home market as a starting condition.

The unfinished tip (production and share-price hooks) was sound: I kept it as it was; its two unit tests pass.

Nothing in this report comes from stored memory; the handover note's branch context matched what I found.

## done

- Merge commit f442a56 (parents d0ab5bd and 8d0023b), German message. Conflicts in command.rs, state.rs and headless main.rs resolved keeping both sides. SimCommand keeps main's order; the seven siege commands follow LayUpShips at the end: BeginSiege, SetSiegeOrders, LiftSiege, SetGarrisonOrders, EmbarkTroops, LandTroops, Capitulate.
- Drift fixed in the merge: update_blockades now takes Battles (threaded through Orbit::refresh_blockades, conquer and resolve_conquests); the succession destructurings name the new world and region fields and leave them untouched; MakePeace passes the truce days after the siege's peace veto; the siege world tests rest the military AI.
- Production and share-price hooks from the tip kept unchanged: disruption idles its share of staffed capital; issuers seated on a besieged world carry a 0.25 risk discount.
- Bookkeeping half of siege_month (new crates/sim/src/siege/month.rs, system in SimSet::Aftermath at month start, after conquests): militia of a world not under siege follows population and cohesion; war damage and occupation decay and are forgotten below 1 %; every region's mirrors (occupation, garrison cover, war damage) are rewritten; running sieges re-read the stakes in the world's firms.
- New field PlanetDefense::occupied_by (serde default): an occupation ends at the next month start when the world no longer belongs to its conqueror, for example after a secession.
- War weariness and the cohesion items for occupation and war damage were already in the merged code (daily step, conquest, CohesionFactors). I added nothing there; they are now fed monthly by the step above and covered by the new world test.
- Military AI: every fleet assigned to a siege is entered in AiState::reserved from the tick of the BeginSiege command until its assignment ends. Only entries the siege made itself are released (new Sieges::held, serde default). Sync runs after command application and in the every-tick siege system.
- hand_over in crates/sim/src/siege/conquest.rs is still the single call site for the owner change (called once, from conquer); the capital branch in resolve_conquests is untouched.
- Design test 10 (bombardment_weighs_on_the_conquered_world_for_a_year): twins with and without area fire before the storm; twelve months later cohesion 32.4 against 40.3, market value of the local issuers 3823 against 4426, attacker's weariness 24.36 against 24.20.
- Design test 11: the conquest parts were already asserted in the starving test; the missing part is the new unit test a_prior_stake_thins_the_militia_and_eases_the_conquest (60 % prior stake: militia times 0.4, at least 15 points more cohesion, lighter occupation).
- Design test 15 (ground_war_in_the_center_idles_the_economy_until_it_is_over): output 1.40 against 2.68 for the twin during fighting in the centre; 3.11 against 2.01 sixty days after the lift.
- Design test 16: the existing replay test (same seed twice, every tick against skipped ticks, save and load mid-siege) passes on the new core and now spans the monthly step and the reservation.
- Five-phase run (a_siege_runs_through_all_five_phases_to_the_hand_over): station fought down, investment, precise fire, landing and storm, underground against regulars in a bunker, outcome Stormed, owner changed on day 770 of seed 3, then 30 more days; invariants and money drift 0 every day.
- AI test (the_fleet_ai_leaves_besieging_fleets_alone): under an active fleet AI the besieger stays 40 days; after LiftSiege the reservation is gone and the AI gives the fleet new orders within 60 days.
- Two unit tests for the monthly step and a rewritten mirror test that now goes through the monthly step instead of writing the region directly.
- README: a siege section, the `headless siege` tool, and the milestone table row for milestone 5.

## not_done

- Procurement half of siege_month (armament budget, recruiting, building levels, upkeep, procure_day) and SetDefensePlan: not built, as the task asked only for the bookkeeping half. Colonies therefore still have no stores, regulars or installations unless a test places them.
- The AI does not start or conduct sieges, as instructed.
- Capital hand-over is not replaced, as instructed: a capital's verdict stays pending and its siege rests.
- SAVE_FORMAT unchanged, as instructed, although the state now contains Sieges and two new fields.
- crates/game was not built or changed.
- Grievances (Sieges::grievances) are still only recorded; nothing reads them. I read 'grievance hooks into cohesion' as the occupation and war-damage items of the cohesion; if the grievance ledger itself was meant to feed cohesion, that is not done.
- Test 10 does not follow the design to the letter: the prelude is 8 days, not 20, and the attacker's weariness is set to 20 as a starting condition. Reasons: a colony without stores falls under area fire before day 20; a siege of 30 days or more adds 0.3 weariness per month, which outweighs the fire victims of a small colony; and the conquest bonus of -3 clamps a low weariness to 0 in both twins.
- No mutation probe was run for the new hooks; the AI test contains its own counter-check (the freed fleet is moved), the others compare twins.
- The scripted nine-war sweep from the old siege report was not rerun; sweeps still contain no sieges because nothing but tests issues siege commands.
- RunSummary and the sweep CSV have no siege figures.
- The headless `siege` tool was built and merged but not run in this session.

## api_notes

All in crates/sim/src/siege unless noted; worktree /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/m5.

**Schedule (plugin.rs)**
- SimSet::Siege, chained: siege_watch (every tick; now also takes ResMut<AiState>), siege_day (day start).
- SimSet::Aftermath, chained: conquests (day start), siege_month (month start).

**New**
- month.rs: `run_month(data, galaxy, polities, finance, logistics, sieges)` and `stakes_in(galaxy, polities, finance, logistics, planet, owner) -> BTreeMap<PolityId, f64>` (also used when a record forms). Both re-exported from sim::siege.
- mod.rs: `reserve_besiegers(sieges: &mut Sieges, ai: &mut AiState)`; `Sieges::held: BTreeSet<FleetId>`; `PlanetDefense::occupied_by: Option<PolityId>`.

**Changed signatures**
- `Orbit::refresh_blockades(galaxy, diplomacy, battles: &Battles, defenses)`.
- `resolve_conquests(data, galaxy, polities, diplomacy, battles: &Battles, economy, finance, logistics, sieges, orbit, chronicle, tick)`; `conquer` likewise (private).
- `Orbit::apply` no longer changes ship fuel; `DayEffects::fuel` is used by the forecast only.

**Data**
- `siege.station_fuel` is 0.004 and must equal `military.fuel_per_day`; `GameData` loading rejects a difference.

**For the next task (capital hand-over)**
- The one call site is `hand_over(...)` inside `conquer` in conquest.rs. It bypasses `Succession::transfer_planet`, so `Endgame` tags, capital move, elite flight and the end of a community do not happen for conquests yet.
- `transfer_planet` needs a `Succession` (borrows of nearly every resource including AiState), while `conquer` holds `&mut Sieges` and an `Orbit` over `&mut Fleets`; the conquest system will have to take `SuccessionParams` instead of its separate resources and build the Orbit from the borrowed fleets.
- After replacing it: delete the capital branch in `resolve_conquests`; set `occupied_by`/`occupied_from` as today; `tidy` already lifts sieges whose defender no longer owns the planet or whose attacker is dead.
- The succession merge guard fired as designed: `PlanetEconomy.disruption`, the four Region mirrors and the three CohesionFactors fields are now named in polity/succession.rs and deliberately left untouched at transfer and at a community's end. `Sieges` itself is not part of `Succession`; its per-community maps (budget, plans, grievances) are not cleaned when a community ends.

**Armament spending** still goes through `orders::armament_allowance` (the whole treasury). It is not a claim in the state budget; purchases appear there as other movements.

## game_api_changes

Found by grep over crates/game/src; the game was not built.

**Likely compile breaks: none found.** The game does not use sim::siege, Sieges, CommandContext, update_blockades or risk_discount, builds no PlanetEconomy, Region, CohesionFactors or SimState by literal, and its one match over SimCommand (ui/fleets/mod.rs) has a catch-all arm.

**Added or changed public items**
- SimCommand has seven new variants at the end: BeginSiege { fleet, planet }, SetSiegeOrders { planet, orders }, LiftSiege { planet }, SetGarrisonOrders { planet, posture, hold_fire }, EmbarkTroops { fleet, planet, strength, supply_days }, LandTroops { fleet, planet, strength }, Capitulate { planet }.
- SimState has the new field `sieges`; new resource sim::siege::Sieges (defenses per planet, active sieges, assign, embarked, pending, stats).
- PlanetEconomy has `disruption`; finance::Region has `besieged`, `occupation`, `garrison_cover`, `war_damage`; CohesionFactors has `occupation`, `garrison_cover`, `war_damage`; Balance has `siege`, CohesionBalance has `occupation`, `occupation_hold`, `war_damage` (all required in balance.ron).
- CommandContext has the new field `sieges`.
- For a siege view: sim::siege::{SiegeView, forecast, Course, Forecast, Phase, Siege::phase, orbit::squadron, planet_facts, firm_value}; `Campaign::view` in crates/sim/tests/siege.rs shows how a SiegeView is assembled.

**Behaviour the game must follow**
- MakePeace can be rejected: "{Gemeinschaft} belagert {Planet}: Frieden kann nur der Belagerer anbieten".
- A planet can change its owner by conquest on any day, not only by secession at a month start.
- Fleets.blockaded no longer contains a system whose owner has a station there; and a blockade now holds through a relief battle while the siege itself is lifted.
- MergeFleets and SplitFleet carry embarked troops and the siege target along.
- New chronicle entries of kind War: start, lifting, conquest and sack of a siege.
- The cohesion panel will show "Besatzung" and "Kriegsschäden" by itself if it iterates the contributions; "Aufbruchstimmung" is scaled down under occupation.
- Share prices of issuers on a besieged world drop (risk discount 0.25); production on a world with ground fighting drops.
- Home worlds have guns, regulars and stores from day one; every owned world has a militia figure from the first month start.
- AiState.reserved now holds real entries (besieging fleets, the player's included).
- Scheduling: siege_watch takes ResMut<AiState> and ResMut<Fleets> every tick; siege systems write Galaxy, Economy, Finance, Sieges and Chronicle at day start, and siege_month writes Finance and Sieges at month start. Any game system inside the sim schedule touching these needs an order.
- Saves: SAVE_FORMAT is still 5; a save written before this merge will not load into a valid state.

## checks

All on the final commit b77a131 with a clean worktree, output to log files under /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/m5, judged by exit code. CARGO_TARGET_DIR was the integ target directory.

- cargo build -p sim -p headless: exit 0, no warnings.
- cargo test -p sim -p headless (whole suite, one run): exit 0. 441 passed, 0 failed, 1 ignored (the existing tech test): 3 (headless), 247 (sim lib), 8, 45, 7, 6, 10, 48 (military), 15 (siege), 17 (succession), 35 + 1 ignored.
- cargo clippy -p sim -p headless --all-targets -- -D warnings: exit 0.
- cargo fmt -p sim -p headless -- --check: exit 0.
- cargo build --release -p headless: exit 0.
- headless sweep --seeds 0..8 --years 60: exit 0, stderr empty. No game ended in the run time.
- headless sweep --seeds 0..8 --years 60 --preset grenzkrieg: exit 0, stderr empty. 38 ships lost, 366 attacks, 10251 blockade days over all runs.
- headless hash --seed 3 --days 3600, twice, and once with --exact: all exit 0, outputs byte-identical. Day 3600: 212cc1125aec6317.

Scripted siege run in a whole game: the test a_siege_runs_through_all_five_phases_to_the_hand_over in crates/sim/tests/siege.rs (seed 3, part of the suite above) passes: phases in order Anmarsch, Einschließung, Beschuss, Bodenkrieg, Untergrund; hand-over on day 770; money drift 0 and check_invariants on every day.

Extra:
- Comparison with the release binary in the main checkout (run with the main checkout's data directory): both sweeps identical in all 41 common CSV columns except hash; 48 and 82 data rows.
- The sweeps and hashes exercise no siege: nothing outside the tests issues siege commands.

## commit

b77a13186b0f79fa96d42951cf3dd0b2ae78b875 on wip/m5-kern, pushed (origin/wip/m5-kern is the same commit), worktree clean. Two commits on top of d0ab5bd: the merge f442a56 and b77a131 (monthly step, AI reservation, tests, README). Both messages are German and end with the two required trailer lines.

## open_issues

- Siege and blockade disagree during a relief battle: the siege is lifted when the battle starts, the system stays closed until the battle is decided. When the besieger wins, a new siege forms and the militia is recomputed from the formula, so a failed relief attempt refills the garrison for free. A siege that rests during the battle instead of ending would match the fleet rule; that is a change to the lifecycle in day.rs.
- A reserved fleet is out of the fleet AI's hands entirely: it gets no retreat order in a battle, and a besieger whose tanks run dry keeps its assignment (it is still docked and at war) and so stays reserved and stranded. Harmless while only scripts start sieges; the siege AI task must release or manage such fleets.
- An AI command issued the tick before a BeginSiege can still move the fleet away in the same command batch; the assignment then drops. Not seen in tests, where the AI rests during staging.
- SAVE_FORMAT is still 5 although the state gained Sieges, Sieges::held and PlanetDefense::occupied_by; with postcard the serde defaults do not make old saves loadable.
- Conquest bypasses the succession path: no Endgame tag for the taken world, and the conqueror's foreign-share and capital logic of transfer_planet does not run. This is the seam for the next task.
- Armament purchases (supply at embarking, the garrison's forage) are paid straight from the treasury, outside the state budget's claims.
- Munition is scarce on home markets after two years on the new core: EmbarkTroops bought 32 of 210 requested unit-days on seed 3. A landing force supplied only by commands runs dry within days until procurement exists.
- With these levers a longer siege wears the attacker more through the monthly 0.3 than area fire on a small colony does through its victims (about 0.16 here); and the conquest bonus of -3 wipes out a low weariness completely. Worth a look when the levers are tuned.
- The many-wars test showed a siege of 1012 days on seed 14: the 500-day bound holds only with an occupation force aboard. Under an active AI most scripted sieges in that test are still lifted (16 of 18 on seed 4, 43 of 44 on seed 10), mostly because the opponent's AI fleets arrive.
- Standing militia is maintained monthly but a new siege record still recomputes it from the formula; the standing figure is for display only.
- The monthly mirrors lag the cohesion by one month: cohesion runs before the siege step in the same tick. Conquest writes its mirrors at once.
- Sieges::budget, plans and grievances keep entries of a community that has ended; subordination and wards were not examined for sieges beyond the head-resolving war check.
- Carried over from the siege reports and unchanged: on the day the last station falls the system is still open until the next morning; PlanetDefense stays with an abandoned colony; the relief test depends on the timing of its scenario; the many-wars test is pinned to seeds 4, 10 and 14.
- Carried over from the previous task and unchanged: fleet rules that are not bloc-aware, owner changes by claiming and abandoning outside succession.rs, the heir's budget showing an inherited treasury as other movements.
- Worth remembering for later sessions (learned by running it here): the fleet layer charges fuel_per_day to every docked fleet, so any new per-day fuel cost for fleets must not be added a second time; and the main checkout's release headless binary still serves as the baseline for AI-only behaviour.

