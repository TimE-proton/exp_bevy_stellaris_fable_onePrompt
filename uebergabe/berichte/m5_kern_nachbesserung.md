# M5 Kern: Nachbesserung

## summary

All 12 review findings on wip/m5-kern were confirmed and fixed at their cause; none turned out wrong. Final commit 9b3908b is pushed, worktree clean, and all standard checks pass.

Each fix has a test that fails without it. I checked that by reverting the fix lines and seeing the tests fail for 11 of the 12; the station-building fix was not reverted (its two tests pass with it).

Three things beyond the literal findings changed, all following from the fixes:

- **Open systems.** A siege no longer closes a system whose owner is not at war with the besieger. In such a system the garrison cannot buy at the market and lives off its depot alone. The siege card's forecast uses the same rule.
- **Conquest script (`harness::conduct`).** With the system left open, the script's fleet was resupplied in place, never went home for troops, and sieges ran 1,000 to 1,700 days. The script now fetches troops once a world is ripe for takeover and besieges a target in its own home system where it lies.
- **Seeds of one whole-game test.** `arrears_occur_in_a_long_scripted_war` now uses seeds 0, 5, 6 instead of 3, 5, 6, because fleet arrears became rarer.

One side effect you should know about: in the peace run of seed 2 a community now briefly owes its fleet 0.097 of its pay; before the fixes it owed none. I believe the cause is that regulars are now fully paid before depots, which costs more, but I did not trace it.

`eroberung` sweep, seeds 0..8, 60 years: 209 sieges begun, 65 stormed, 22 starved, 0 handed over, 122 lifted, 87 worlds fallen, 5 communities ended, longest 459 days. Before the fixes: 228 / 68 / 16 / 0 / 144 / 84 / 5 / 1,168 days. Battles per run fell from 59.6 to 33.1 and ships built from 808 to 685; I did not trace which fix drives that.

## done

- F1 LandTroops near-zero strength: a strength with no troops aboard is rejected ('an Bord von … sind keine Truppen'), so there is no 0/0. Supply no longer stays aboard without troops: when a fleet's last units leave (LandTroops, occupation landing in the day step, Capitulate), their supply goes with them to the landing force or, on an own world, into the depot up to the cap (the excess is dropped). Test: siege::orders::tests::supply_never_stays_aboard_without_troops_and_no_landing_conjures_stores.
- F2 and F6 stale reservation: new system siege_release runs reserve_besiegers at the end of every day-start tick (after siege_day, conquests and the month books). Test: tests/siege.rs a_fleet_wrecked_over_a_world_is_released_in_the_same_tick. The conquest-script whole-game test now checks invariants every day instead of every 30. Skip and exact mode agree: hash --seed 6 --days 9240 --every 9240 --preset eroberung gives f3eb46f3fd7626ea both ways (the script itself changed, so this is not the reviewer's exact game).
- F3 depots before pay: on the first day of a month the depot purchase runs after pay, upkeep, recruiting and works (siege::open_arms_month, called from siege_month); on all other days procure_day runs where it did. Test: siege::arms::tests::pay_comes_before_depots_when_the_grant_covers_only_one.
- F4 unfunded counted twice: run_arms_month no longer books the unpaid defence pay a second time. The existing test now asserts unfunded == claim (a_cut_defence_buys_nothing_and_its_regulars_drift_away).
- F5 ships wrecked by a besieged world: Orbit::apply records ships and whole fleets lost per community; siege_day books them through the new Battles::count_wrecked, so they appear in ships_lost and fleets_lost of the run summary. The chronicle gets a War entry: 'Belagerung von X: Y verliert N Schiffe an die Verteidigung der Welt. Die Flotte Z ist verloren.' Test: tests/siege.rs ships_wrecked_over_a_world_are_counted_and_told.
- F7 blockade days counted again on every conquest: fleet::update_blockades is split; conquer uses the new fleet::recompute_blockades, which sets the closed systems without counting a day. Test: siege::day::tests::recomputing_blockades_after_a_conquest_counts_no_second_day.
- F8 retaken world tagged Own: PlanetDefense::former_tag (serde default) remembers path, population and gap the world had under the community it was taken from; a liberation restores it, and it is cleared when the occupation ends. Test: extended siege::conquest::tests::taking_a_world_back_is_a_liberation.
- F9 siege closes the besieger's own system: held_systems closes a system only if its owner is at war with the besieger (new orbit::closes). In an open system the garrison gets no market forage. Test: siege::day::tests::a_siege_closes_only_systems_of_the_enemy.
- F10 siege card while the siege rests: siege_view and siege_card use the new orbit::presence, which still lists assigned fleets that lie over the planet in battle or regrouping, with their fuel, firepower, magazines and troops aboard; control stays false. The three forecasts then report Limit::NoControl instead of Fuel. Tests: siege::view::tests::a_resting_siege_still_shows_its_fleet_and_names_the_contested_orbit, plus assertions added to the relief test in tests/siege.rs.
- F11 stations never built: works are no longer bought all or nothing. Each month the world buys what the market yields (buy_share) towards the next level; PlanetDefense::delivered (serde default) holds the sets already delivered, and the level stands once n sets for level n are in. Claim, daily demand and DefenseCard::build_cost count only what is still missing. Tests: siege::arms::tests::works_grow_from_what_the_market_yields_month_by_month and the whole-game test a_planned_station_is_built_from_the_market_in_play (seed 3, SetDefensePlan stations 1 on a capital, built within five years).
- F12 'Phase 5 von 5, Untergrund' on worlds without underground: Siege::phase returns Underground only with regulars below ground (deep > 0), otherwise Ground; and the chronicle names no new phase on the day a verdict falls. Test: siege::day::tests::a_world_without_underground_falls_without_a_fifth_phase.
- Conquest script harness::conduct: a fleet without troops goes home to fetch them once the garrison has dissolved, nobody is ashore or aboard and regulars wait at home; a fleet under way with empty berths fetches troops first; a target in the attacker's own home system is besieged in place.
- README section 'Belagerung' updated for the blockade rule, supply, works, losses in orbit, liberation tag, fifth phase and the resting siege card.

## not_done

- crates/game was neither built nor changed, as instructed.
- The peace-time fleet arrears now seen in seed 2 (0.097 in one month of the 21-year budget test run) were not traced to a specific fix.
- The drop in battles and ships built in the eroberung sweep was not traced to a specific fix.
- F11 was not checked by reverting the fix; the reviewer's observation that stations are never built stands as the evidence for the old behaviour. Guns, shields and bunkers were not tested in a whole game, only stations.
- The two consequences of F1 the reviewer read from code (NaN depot on a world with garrison 0, stranded supply) are covered by the unit test only, not by a whole-game replay of the reviewer's probe.
- Saves from before this branch were not load-tested; the two new PlanetDefense fields have serde defaults.
- tests/siege.rs still uses the strict orbit::squadron in its own Campaign::view helper; only the read model switched to presence.
- The sack test (landing_sack_and_conquest_only_move_money) was loosened: firms' cash is rounded down per firm, so their sum may fall short by up to one thousandth per firm. The old exact assertion had passed by luck of rounding.

## api_notes

Final public API of the sim core for sieges (crate sim, module siege unless noted).

SIEGE READ MODEL (siege::view)
- SiegeRefs::of_world(&World) / of_state(&SimState, &GameData).
- siege_cards(&refs, day) -> Vec<SiegeCard>; siege_card(&refs, planet, day) -> Option<SiegeCard>; siege_view(&refs, planet, day) -> Option<SiegeView>. Headless also has siege_cards() and command_block(polity, &command).
- SiegeCard fields: planet, planet_name, system, attacker, defender, phase, phase_number (1..5), phase_label, days, control, blockaded, handover: Option<Outcome>, progress, stores_days, army_supply_days, defense: DefenseStrength {stations, guns, guns_active, shield, charge, bunkers}, troops: TroopStrength {army, embarked, militia, reserve, deep, garrison_start, occupation_force}, fleets, firepower, ammo, fuel, forecasts: [(Course, Forecast); 3], view.
- Phase labels: Anmarsch, Einschließung, Beschuss, Bodenkrieg, Untergrund. Underground now only with regulars below ground.
- While the siege rests (control == false, fleet in battle or regrouping) the card still lists fleets, fuel, firepower, ammo and troops.embarked; all three forecasts have falls == false and limited_by == Some(Limit::NoControl).
- blockaded is false when the besieged world lies in a system not owned by an enemy of the besieger.
- Forecast.limited_by: Limit::{NoControl, Stations, Fleet, Fuel, Ammunition, Shield, NoTroops, Repelled, Time}.
- SiegeReport.blocked (BlockedSet of Blocked::{NoControl, Stations, NoFirepower, NoAmmo, ShieldHolds, Underground, NoArmy, Handover}) says which standing orders could not act that day.

COMMANDS (SimCommand, unchanged)
- BeginSiege {fleet, planet}; SetSiegeOrders {planet, orders: SiegeOrders {fire, assault, focus, sack}}; LiftSiege {planet}; SetGarrisonOrders {planet, posture, hold_fire}; EmbarkTroops {fleet, planet, strength, supply_days}; LandTroops {fleet, planet, strength}; Capitulate {planet}; SetDefensePlan {planet, plan}.

REASONS FOR DISABLED COMMANDS
- siege::command_block(&refs, polity, &command, tick) -> Option<String>: the core's own German rejection text, None if it would be accepted or the command is not a siege command. It runs the command on a copy, so call it per selection or per day, not per frame.
- New text for LandTroops: 'an Bord von <Flotte> sind keine Truppen' when nothing is aboard (previously a tiny strength was accepted).

DEFENCE PLAN
- DefensePlan {stations, guns, shield, bunkers: u8, regulars: f32, depot_days: u16}.
- defense_card(&refs, planet) -> Option<DefenseCard> {planet, owner, besieged, plan, defense: PlanetDefense, level_caps: [u8; 4], regulars_cap, depot_days_cap, upkeep, plan_upkeep, build_cost, hold_need}. Order of the four works is Works::ALL = Stations, Guns, Shield, Bunkers; Works::index() gives the position.
- New: PlanetDefense::delivered: [f64; 4] = sets of goods already delivered towards the next level of each work; level n needs n sets. Progress of the next level = delivered[i] / next_level. siege::sets_missing(&defense, works, next) gives what is still missing. build_cost already subtracts what is delivered.
- New: PlanetDefense::former_tag: Option<(endgame::Path, f64, f32)>, internal bookkeeping for liberation.

CLAIM VARIANT
- economy::Claim::Defense, ranked after Fleet and before Procurement. Budget lines: wanted, paid. BudgetMonth::unfunded now counts unpaid defence pay once.

CHRONICLE
- All siege entries are ChronicleKind::War: blockade begins/ends, siege begins, phase change ('Belagerung von X: Phase n von 5, <Name>.'), siege lifted, world falls ('ist gestürmt/ausgehungert/übergeben und wird von Y besetzt' or 'kehrt zu Y zurück'), sack.
- New: 'Belagerung von X: Y verliert N Schiffe an die Verteidigung der Welt. Die Flotte Z ist verloren.'
- No phase entry is written on the day a world falls.
- Loss of a capital and the end of a community come from the succession path (not changed here).

WHEN THE PLAYER'S COMMUNITY ENDS (not changed in this step; read from polity/succession.rs)
- Conquest of the last world ends the community through the common succession path: SimConfig::player becomes None and Endgame::player becomes PlayerFate::Out (label 'ausgeschieden'). Its fleets, sieges, assignments and embarked troops are dissolved in the same step. The game continues as an observer game unless an ending is reached.

OTHER NEW PUBLIC ITEMS
- siege::open_arms_month (month-start books: pay, then depots).
- siege::orbit::{presence, closes}; Orbit::take_wrecks; Toll::fleets_lost.
- Embarked::{disembark, leave}.
- fleet::recompute_blockades; Battles::count_wrecked.

## game_api_changes

Changes crates/game has to follow (it was not built against this commit):

1. PlanetDefense has two new fields, delivered: [f64; 4] and former_tag: Option<(Path, f64, f32)>. Any struct literal of PlanetDefense in the game needs them or ..Default::default().
2. Toll has a new field fleets_lost: Vec<String>.
3. siege::run_day and siege::tidy signatures are unchanged. The plugin's systems changed (siege_procure skips the first day of a month, siege_month now also does the day's depot purchase, new siege_release); the game is affected only if it orders its own systems relative to these.
4. Siege panel: with control == false the card now still carries fleets, fuel, firepower, ammo and embarked troops; show 'ruht, Orbit umstritten' from control and Limit::NoControl rather than treating empty fleets as 'no fleet'.
5. Siege panel: blockaded can be false during an investment (world in the besieger's own or a third party's system). Do not derive 'eingeschlossen' from blockaded; use the phase.
6. Phase display: a world without regulars below ground never shows phase 5; it stays in 'Bodenkrieg' until it falls.
7. Defence panel: show build progress per work from defense.delivered[i] against the next level (level n needs n sets). Levels now grow over several months; build_cost shrinks as goods are delivered.
8. LandTroops button: command_block can now return 'an Bord von … sind keine Truppen'.
9. Supply display: after the last units of a fleet land, the fleet's Embarked entry is gone (supply went ashore with them). Landing on an own world drops supply beyond the depot cap when no troops stay aboard.
10. Fleet statistics: ships_lost and fleets_lost now include losses to stations and guns of besieged worlds; a new chronicle line of kind War reports them.
11. Blockade-day statistics are lower than before on days with conquests (no double counting).
12. Budget panel: the 'unfunded' figure of a month no longer doubles unpaid defence pay.
13. No SimCommand variant was added or changed, SAVE_FORMAT is unchanged, Claim::Defense and ChronicleKind are unchanged.

## checks

All run on commit 9b3908b with a clean worktree; logs in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/m5 (final_*.log, final_*.out).

- cargo build -p sim -p headless: exit 0.
- cargo test -p sim -p headless (whole suite, one run): exit 0; 460 passed, 0 failed, 1 ignored.
- cargo clippy -p sim -p headless --all-targets -- -D warnings: exit 0.
- cargo fmt -p sim -p headless -- --check: exit 0.
- cargo build --release -p headless: exit 0.
- release sweep --seeds 0..8 --years 60: exit 0, stderr empty.
- release sweep --seeds 0..8 --years 60 --preset grenzkrieg: exit 0, stderr empty.
- release sweep --seeds 0..8 --years 60 --preset eroberung: exit 0, stderr empty. Sieges: 209 begun; 65 stormed, 22 starved, 0 handed over, 122 lifted; 87 worlds fallen, 5 communities ended (seeds 0, 1, 3, 6, 7); longest 459 days. Per seed (begun/stormed/starved/lifted/fallen/ended/longest): 0: 21/14/5/2/19/1/128; 1: 19/11/5/3/16/1/136; 2: 9/2/2/5/4/0/459; 3: 19/14/2/3/16/1/107; 4: 8/2/4/2/6/0/130; 5: 68/1/0/67/1/0/253; 6: 16/9/3/4/12/1/133; 7: 49/12/1/36/13/1/274. Same sweep: 33.1 battles per run, 685 ships built, 318 lost, 11,503 blockade days.
- release hash --seed 3 --days 3600, run twice and once with --exact: all three outputs identical; day 3600 = 8c349f6a54008276.
- Extra: hash --seed 6 --days 9240 --every 9240 --preset eroberung = f3eb46f3fd7626ea, same with --exact.
- Reverting the fix lines made these tests fail, as intended: 8 unit tests in sim::siege, the relief test, and the two 'wrecked' tests in tests/siege.rs.

## commit

9b3908b on wip/m5-kern, pushed to origin/wip/m5-kern (three commits on top of b0fa146: 99ce7b0 conquest script, 74ce0dd siege fixes and tests, 9b3908b README). Worktree /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/m5 is clean.

## open_issues

- Peace-time fleet arrears: in the peace run of seed 2 (tests/budget.rs setup, 21 years) a community owes its fleet 0.097 of its pay in one month after the fixes; before them it owed none. Probably because regulars are now fully paid before depots, but not traced. Fleet arrears in the long scripted war also became rarer (seeds 0, 5, 8 of 0..9 instead of two of 3, 5, 6).
- Balance shift in the eroberung sweep not traced: battles per run 59.6 -> 33.1, ships built 808 -> 685, AI offensives 163 -> 119. Rebuilding damaged works now draws on the defence claim every month, which may take money from procurement.
- In a system that stays open (owned by the besieger or a third party) the besieging fleet is resupplied in place and the world's economy is not cut off; only the garrison's market access is. Whether an invested world in an open system should also lose its trade is a design question the design text does not answer.
- Seed 5 of the eroberung sweep still shows 67 of 68 sieges lifted and one world fallen; this was similar before the fixes (57 of 58) and was not investigated.
- Capitulation is still never triggered by the script (0 handed over in every sweep); sack is not triggered either. Both are covered by tests only.
- Wrecks from station and gun fire leave no debris and cause no war-weariness or share-price shock beyond what the siege already books; only the tallies and the chronicle were added.
- Memory: nothing in this report comes from stored memory. One item worth keeping, learned in this session from the test runs: several whole-game tests (tests/budget.rs arrears test, tests/siege.rs conquest-script test) depend on chosen seeds and need re-choosing when budget or siege rules shift.

