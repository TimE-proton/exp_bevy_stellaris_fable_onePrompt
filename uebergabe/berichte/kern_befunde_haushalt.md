# A2 Befunde und Staatshaushalt

## summary

All 15 open confirmed findings on the strategic layer are resolved on wip/m4-integ. Final commit f053104, pushed, worktree clean; every required check passes.

Groups 1 to 3 and 5: the predecessor's commits 549ba93 to 8758cd7 are sound. I checked each finding against code and tests, mutation-checked three more of them, and reworked the two tests the join had weakened.

Group 4: I built on 8ffaae6 instead of reverting it. Its order of claims and read model were coherent and already met aim c, but it still paid crew and research salaries to all households, so aim d failed: household needs coverage was 0.801 with fleet and research and 0.804 without. Pay now goes to a separate state-service account that buys at the seat's market. With that, a community without fleet and research hands out 41.7 % of revenue instead of 19.1 %, and needs coverage is 0.804 instead of 0.765, in each of 8 seeds.

Three things need your decision:
- Population after 60 years is 5.4 % lower than before the budget (51,363 against 54,318 million). That is the now real cost of fleet and research; supply, cohesion, prices and secessions are unchanged.
- The budget has two claims the task did not list: a civil base before the fleet, and capital for Kontore before research.
- No command scraps ships, so a community in deficit can only raise taxes, borrow or lose ships.

## findings

- determinismus 1, korrektheit 4, anforderungen 2 (forced retreat, regroup protection): present after the join, fixed in 37c41d9. A regrouping fleet now rejects MoveFleet and finishes its withdrawal, a split keeps route and period, merging needs both fleets in the same state, yards deliver no ship into it, and an existing blockade holds while the blockader fights. Tests: unit the_regroup_period_can_neither_be_shed_nor_passed_on, a_withdrawal_cannot_be_called_off_and_the_regroup_period_stays_with_the_fleet, turning_up_does_not_lift_a_blockade_but_winning_does.
- korrektheit 1 (undecided battles restart): fixed by the join, closed in 37c41d9. Everyone not at home withdraws and regroups, opponents never withdraw into the same system (over several hops if needed), and the same opponents can meet again only after regroup_days and only if one marches on the other. Tests: a_stalemate_has_no_winner_and_does_not_start_over, unit opponents_never_fall_back_into_the_same_system.
- korrektheit 2 (fleet released only when the whole battle ends): fixed in 37c41d9. muster_battles releases each morning a party with no opponent or no own ship left on the field. Tests: peace_frees_a_fleet_while_the_others_fight_on, a_fleet_that_leaves_the_field_is_free_at_once_and_shares_the_fate_of_its_side.
- korrektheit 3 (field held per fleet): fixed in 37c41d9. Holding is decided per side; a fleet off the field regroups in the system while its side holds or still fights and leaves only when an opponent holds; craft alone hold nothing. Tests: unit whose_side_holds_the_field_need_not_leave_the_system, craft_in_the_air_do_not_hold_the_field_for_a_fleet_without_ships.
- korrektheit 5 (order hangs when the yard disappears): fixed in 10213a2. An order without a yard rests with BuildStatus::NoYard, keeps progress and money, resumes if the yard returns within yard_grace_days (360) and otherwise lapses without refund, with the written-off amount in the chronicle. Test: unit an_order_rests_while_its_yard_is_gone_and_lapses_after_the_grace_period (fails under my mutation).
- korrektheit 6 (pay, counts and strength include wrecks): fixed in 37c41d9. Destroyed ships are written back to the fleets every morning of a running battle. Test: unit what_a_running_battle_has_cost_shows_in_the_fleet_each_morning (fails under my mutation).
- determinismus 2, korrektheit 7 (headless panic on unknown community): fixed in 549ba93. Names are looked up with a fallback, and the script sends no fleet orders when a side does not exist. Test: headless a_script_naming_a_community_that_does_not_exist_is_reported_not_fatal.
- anforderungen 1 (colony yards flicker): fixed in 10213a2. A yard opens at arms capital 1.0 and closes below 0.5; leftovers make none. Tests: unit a_colony_yard_needs_real_arms_capital_and_does_not_flicker (fails under my mutation), colony_yards_follow_real_arms_industry_and_do_not_flicker over 20 years.
- anforderungen 3 (holding station costs nothing): fixed in 0d5e48b. A fleet burns fuel_per_day (0.004 of its tank) in orbit, and without fuel it neither blockades nor raids nor escorts. Test: a_blockade_far_from_home_runs_dry_without_supply.
- anforderungen 6 (loss scaled by what is left): fixed in 0d5e48b. loss_share measures against the navy before the battle, at least one starting fleet. Tests: losses_are_weighed_against_a_stable_measure_not_against_what_is_left, unit the_last_ship_of_a_worn_down_navy_weighs_what_it_is_worth.
- anforderungen 7 (yard in a system another community holds): fixed in 10213a2. One notion of own territory (supply_base: any system with an own world, unless its holder is at war with you) serves yards, resupply and repair. Tests: a_fleet_is_supplied_wherever_its_community_owns_a_world, unit a_yard_in_a_system_held_by_others_builds_and_its_ships_are_supplied_there.
- anforderungen 8 (tools cannot combine orders with wars): fixed in 8758cd7. run, sweep, hash and fleets take one script with --war, --peace, --order and --navy. Test: harness a_script_orders_ships_and_a_programme_replaces_what_a_war_costs; the required war sweep uses it.
- anforderungen 4 (fleet costs leave no trace): still present at 8ffaae6, which had the claims but paid salaries to all households. Fixed in febb2fe to f053104, see budget. Tests: four game-level tests in tests/budget.rs (two fail when pay is routed back to households), seven unit tests in economy/budget.rs, the rewritten pay and arrears tests in military.rs and tech.rs.
- anforderungen 10 (tests on fabricated states): joined by real play. From 8758cd7: an_order_in_an_untouched_economy_is_built_from_the_market_and_moves_it, a_war_wears_on_cohesion_and_makes_borrowing_dearer, commands_given_during_a_battle_replay_to_the_same_war; from me: tests/budget.rs. The 2000-cruiser arrears test stays as the exact arithmetic of one month.
- Tests the join changed: larger_economies_research_faster_in_a_running_game now measures research points over six games (35 clear pairs; at least 90 % ahead, pooled points at least 1.2 times, log-log correlation at least 0.75). Over twelve seeds the larger economy led in 70 of 70 pairs with correlation 0.90, while the technology count reversed in 13. The finance colony test now counts stages over the last ten year-ends, so the original seeds 4, 14, 24 are back.
- Residuals. Regroup protection: a regrouping fleet is out of play (no march, blockade, escort, orbit defence or new ships), so the protection covers only the withdrawal. Three or more hostile parties: the route continues over several hops to the nearest system no opponent chose; only if the galaxy has none does a fleet regroup in place. Sister fleet holds the field: the fleet regroups behind the lines in the system and leaves if its side loses the field after all.

## budget

Decision: built on 8ffaae6, not reverted. It compiled, its claims, grants and read model matched aims a, c and e, and a sweep at that commit was healthy (population 54,110, cohesion 66.9). Missing was the cause of the finding itself: pay still went through pay_households.

MECHANICS
- Monthly close per community (finance::plugin::monthly_books calls economy::settle_budget). Revenue is the closed month's wage tax, Kontor tariffs and dividends on state holdings.
- Order of claims (economy::Claim): 1 debt service; 2 civil base (transfer to households: civil_share of the 12-month mean revenue at the standard tax rate); 3 fleet (crew pay monthly, spare parts daily); 4 procurement (yards, restocking, repair, paid as bought; the month's estimate is set aside at the close); 5 capital for Kontore without cash; 6 research; 7 reserve; 8 hand-out (handout_share of what lies above the reserve).
- Cuts come from the back. Claims 1 to 5 may use the reserve; research gets only what the month leaves after them plus what already lay above the reserve.
- Consequences of a cut: fleet claim not granted in full gives arrears (lower crew morale); research earns fewer points; yards, restocking and repair wait (BuildStatus::MissingMoney).
- Real cost: crew pay and research salaries go to Economy::staff, which buys the household basket at the seat's market (spend_rate a day); spare parts and fuel are bought on markets and used up. Only civil base and hand-out reach households.
- Raising the wage tax creates room, because the civil base is measured at the standard rate; cutting the tax does not shrink the base.

LEVERS (balance.ron)
- economy: civil_share 0.5 (new), handout_share 0.5 (was a fixed half), state_reserve_per_million 1.5, income_tax 0.15 (the standard rate), spend_rate 0.08 (also the staff's).
- military: upkeep_per_month 0.03 (was 0.012), maintenance_per_month 0.04 (new), fuel_per_day 0.004, fuel_goods 0.2 (was 0.03), arrears_smoothing 0.5, morale_per_arrears 30.
- research: budget_share 0.125 (was 0.20), max_budget_share 0.1875 (was 0.30), efficiency 0.0416 (was 0.026, so points per unit of revenue are unchanged), template shares 0.10 to 0.175.
- Code constants, unchanged: Kontor floor 600 and refill 1500; BUDGET_HISTORY 12.

AIMS C AND D: headless fleets --seeds 0..8 --years 10, all communities pooled, % of current state revenue
variant | fleet | procurement | research | remainder | hand-out | months below reserve | highest arrears | needs covered | household cash per million
as shipped | 8.7 | 1.5 | 13.0 | 23.1 | 19.1 | 15 % | 0 | 0.765 | 9.93
no fleet cost, no research | 0 | 0 | 0 | 45.7 | 41.7 | 2 % | 0 | 0.804 | 10.79
no fleet cost | 0 | 0 | 13.3 | 33.1 | 29.1 | 6 % | 0 | 0.782 | 10.29
no research | 9.0 | 1.5 | 0 | 35.2 | 31.1 | 6 % | 0 | 0.784 | 10.34
2 starting fleets | 17.8 | 3.0 | 12.0 | 13.6 | 9.8 | 37 % | 0 | 0.748 | 9.56
3 starting fleets | 27.1 | 4.5 | 9.9 | 4.7 | 3.1 | 78 % | 43 % | 0.737 | 9.27
4 starting fleets | 35.8 | 6.1 | 7.4 | -1.0 | 1.1 | 97 % | 58 % | 0.735 | 9.06
research at maximum | 8.7 | 1.5 | 18.0 | 18.4 | 14.4 | 22 % | 0 | 0.758 | 9.76
2 fleets, research at maximum | 17.7 | 2.9 | 16.3 (cut) | 9.4 | 6.1 | 53 % | 0 | 0.741 | 9.41
4 fleets, wage tax 25 % | 30.9 | 5.0 | 11.0 | 8.3 | 5.1 | 57 % | 1.3 % | 0.687 | 8.05
Civil base 48.6 to 48.8 % everywhere, 40.5 % with the tax raised; Kontore 3.0 to 5.7 %.
- Per seed (tests/budget.rs, 5 years): fleet 8.8 to 9.3 %, research 12.6 to 13.7 %. Doubled fleet: hand-out 9.8 to 11.0 % against 19.1 to 21.6 %, needs covered lower by 0.017 to 0.024. No fleet and research: hand-out 42.5 to 44.7 %, needs covered higher by 0.038 to 0.050.
- Four fleets, years 3 to 5, seeds 3 and 5: every community below its reserve in 36 of 36 months, hand-out 0, claims above revenue, arrears peak 3 to 67 %.
- Scripted war (twelve years of yearly offensives both ways, both sides arming to four fleets): one belligerent reaches arrears of 48 to 50 % in each of seeds 1, 4 and 5. With two or three fleets belligerents rarely do, because research is cut first.
- Aim b in prices, seed 3, year 30, capital market: hull segments 95.9 (before 35.0, the floor), weapons 134 (113), drives 139 (80).

AIM F: sweep --seeds 0..8 --years 60, before = 8758cd7, after = f053104
seed | population (millions) | cohesion | price level | secessions
0 | 54249 to 50965 | 65.8 to 66.8 | 0.58 to 0.56 | 1 to 1
1 | 52641 to 49965 | 65.4 to 66.2 | 0.64 to 0.57 | 0 to 1
2 | 54857 to 52568 | 66.6 to 67.6 | 0.66 to 0.54 | 0 to 0
3 | 52696 to 50079 | 66.3 to 66.9 | 0.59 to 0.59 | 0 to 0
4 | 56189 to 52765 | 67.3 to 68.5 | 0.61 to 0.58 | 2 to 2
5 | 53219 to 50193 | 67.7 to 68.2 | 0.54 to 0.53 | 0 to 0
6 | 54339 to 51112 | 66.9 to 66.9 | 0.62 to 0.62 | 0 to 0
7 | 56355 to 53254 | 66.6 to 66.4 | 0.58 to 0.67 | 0 to 0
Means: population 54,318 to 51,363 (-5.4 %), supply 1.00 to 1.00 (0.99 in two seeds), cohesion 66.6 to 67.2, price level 0.60 to 0.58, secessions 0.4 to 0.5, utilisation 0.69 both, jobs filled 0.71 both, freighters 586 to 520, technologies 11.9 to 11.6. No invariant violation, money_drift 0, arrears 0 %.
At 8ffaae6 (budget, pay still to households) population was 54,110, so the 5.4 % are the cost of fleet and research. At year 60 revenue is 19,777 a month, the starting fleet costs 3.6 %, research 13.1 %, hand-out 31.7 %.

## api_notes

BUDGET READ MODEL (sim::economy)
- Economy::budget(polity) -> Option<&Budget>. Budget { current, history (at most 12 closed months, newest last), grants, tracked, opened }, with last() and year() (sum of history).
- BudgetMonth { day, revenue, wage_bill, standard_revenue, other, lines: BTreeMap<Claim, Line { wanted, paid }>, unfunded, treasury, reserve_target }, with line(claim), spending(), claimed(), unpaid(), remainder(), handout().
- Claim::ALL is the rank order; Claim::label() is the German name. `current` is the running month: its fleet and procurement lines grow daily, so show last() for a complete month.
- `other` is the balance of everything outside the budget (central-bank money, bonds, colony founding, share trades).

ADDING A CLAIM (planetary defence)
- Add a Claim variant at its rank (ALL, label, may_use_reserve), insert its demand into `Wants` in finance::plugin::monthly_books, and spend the grant with Economy::allowance + Economy::spend (purchases) or Economy::pay_staff (pay).

NEW IN sim::economy
- Economy.staff (per community), staff_money(polity), pay_staff(polity, claim, amount) -> Money, staff_consume(galaxy, economy, data, seats); Economy.budgets, Economy.wage_bill_running.
- Economy::{allowance, want, book_paid, spend, open_budget_month}, settle_budget, market_capital_needs. settle_public_accounts now only collects tariffs.
- Polity::seat(&Galaxy) -> Option<PlanetId>: capital if still owned, else the first world.

FLEET AND RESEARCH
- fleet_claim, procurement_estimate, maintenance_parts, maintain_fleets, research_claim. pay_upkeep pays the staff account; arrears are the share of the whole fleet claim (pay plus parts) the budget did not grant. Chronicle text is now "kann seine Flotte nicht mehr bezahlen".
- From the predecessor's commits, which had no report: update_blockades takes &Battles; yard_slots takes &Fleets; update_yards and Fleets.yards; BuildOrder.status: BuildStatus with stalled() and status_text(data) (the field `stalled` is gone); Battles.departed, Departed, Departure, Battles::is_fighting, muster_battles, withdrawal_route, loss_share; SideReport.left_early; Fleet::has_fuel; supply_base.
- Commands: MoveFleet is rejected while a fleet regroups; MergeFleets is rejected between a regrouping and a ready fleet.

HARNESS AND TOOLS
- EconomyStats.needs_met and staff_money; PolitySummary.budget and budget_months. WarScript::new(Vec<ScriptedEvent>), ScriptedAct::{War, Peace, Order, Navy}.
- headless fleets: --seeds, --start-fleets, --research-share, --tax (issued as SetTaxRate on day 0), --fleet-cost, --months. run and sweep print the budget of the last twelve months; the sweep CSV has needs_met, revenue, fleet_paid, research_paid, handout.

FOR THE PARALLEL PACKAGES
- Succession: when a community ends, Economy.staff[polity] and Economy.budgets[polity] stay as they are; staff money of a community without a seat is never spent and should be handed over with the treasury.
- balance.ron: I changed only lines inside economy, military and research, nothing at the start or end of the file. SAVE_FORMAT is unchanged; new state fields have serde defaults.

## game_api_changes

crates/game was not built. This rests on grep over crates/game/src in this worktree, which is the game before the join.

No public sim item the game uses was removed, renamed or changed in type, neither by my commits (8ffaae6..f053104) nor by the predecessor's (5ac904d..8758cd7). The game uses Economy::{treasury, market, planet, treasury_targets, tax_rates}, Fleets::{of, list, docked_in, is_blockaded, blockaded}, Fleet fields and is_docked, SimCommand, unit_economics, expected_return, to_credits, balance.fleets.speed and items outside my work. Economy gained the fields budgets, staff and wage_bill_running; the game builds no Economy literal.

Since 8758cd7 the sim API is purely additive. I use that commit as reference because, according to the handover note in memory, the game lane merges it.

Changes in meaning the game has to follow:
1. MoveFleet is rejected while a fleet regroups. crates/game/src/ui/fleets.rs line 70 should disable "Marsch zum Ziel" when fleet.is_regrouping().
2. Fleet.strength and Fleet.ships fall each morning of a running battle, no longer only at its end.
3. A system stays blockaded while the blockader fights; arriving does not open it.
4. The treasury follows the budget. The finance panel should show Economy::budget(player): last() and year(), with Claim::label.

For a fleet or yard window built on the newer API: BuildOrder.stalled (field) is replaced by BuildOrder::stalled() and status / status_text(data); yard_slots takes &Fleets as fifth argument; update_blockades takes &Battles.

## files_changed

- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/README.md
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/assets/data/balance.ron
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/headless/src/fleets.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/headless/src/main.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/economy/budget.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/economy/capital.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/economy/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/economy/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/economy/production.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/economy/setup.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/fleet/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/fleet/supply.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/harness.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/lib.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/polity/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/tech/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/tech/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/src/tech/step.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/tests/budget.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/tests/finance.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/tests/military.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ/crates/sim/tests/tech.rs

## checks

All run in /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/integ on commit f053104, output to log files, exit codes read directly. Logs: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/kern/z/final

1. cargo build -p sim -p headless: exit 0.
2. cargo test -p sim: exit 0. 301 passed, 0 failed, 1 ignored (lib 156, battle 45, budget 4, economy 6, finance 9, military 46, tech 35).
3. cargo clippy -p sim -p headless --all-targets -- -D warnings: exit 0.
4. cargo fmt -p sim -p headless -- --check: exit 0.
5. cargo build --release -p headless: exit 0.
6. target/release/headless sweep --seeds 0..8 --years 60: exit 0, stderr empty, no invariant violation.
7. target/release/headless hash --seed 3 --days 3600: twice identical and identical with --exact (day 3600: 813fc97a18bbea83).
8. Scripted wars: `run --seed 3 --years 30 --quiet` with nine --war and one --peace: exit 0, stderr empty, 5 battles begun, 5 ended, 16 ships lost. The same script as `sweep --seeds 0..8 --years 40` with --navy 6:alle:2 and --order 12:alle:speer:1,dolch:2@kolonie: exit 0, stderr empty, 18.1 battles and 34 lost ships per run. Its hash over 5400 days is identical with --exact.

Additional
- cargo test -p headless: 3 passed.
- cargo doc -p sim --no-deps: no warnings.
- The ignored 150-year research pace test passed (248 s) on ac46ee7; later commits change no trajectory of games without a human player (sweep hashes identical).
- Mutation checks, sources restored afterwards: pay routed back to households fails 2 of the game-level budget tests; no daily write-back, any arms capital makes a yard, and an order without a yard just waits each fail their unit test.

Not run: any build or test of crates/game.

## commit

f05310404678ac612ffbc4a144061bf24b11f4f9 on wip/m4-integ, pushed; origin/wip/m4-integ points to the same commit and the worktree is clean. My commits after 8ffaae6: febb2fe (pay goes to the state service), 2e7d966 (tests restated for the budget rules), 258b311 (game-level budget tests), ac46ee7 (tools, README, maximum research share), 2230480 (Kontor claim test), b98fdd5, 1425c0f (colony test counts stages), 3d1fd2d, f053104 (civil base at tax rate zero).

## deviations

1. Built on 8ffaae6 instead of reverting it (reason under budget).
2. Two claims beyond the list in the task. The civil base ranks before the fleet: without a fixed claim of about half the revenue, four fleets at 9 % each plus research cannot produce a deficit. Capital for Kontore existed before as an unconditional payment; it now ranks after procurement. I tried it before the fleet and took that back: months in which several Kontore need capital then left the fleet unpaid, with arrears already at two fleets.
3. Pay goes to a new money holder, Economy::staff, counted in the money sum. This is my reading of "the cause, not the symptom": as long as pay lands in the households, any fleet cost is a relabelled transfer.
4. Maximum research share is 0.1875, not the predecessor's 0.25. It keeps the ratio of 1.5 to the usual share that the technology tree and its 150-year test are built on.
5. `fleets --tax` now issues SetTaxRate instead of changing the standard rate in the data; otherwise the civil base moved with it and a tax increase showed no effect.
6. Fixed on the way: the taxable wage bill was not recorded at a tax rate of exactly zero, so cutting the tax to zero escaped part of the civil base.
7. Tests restated, never loosened to pass: ten in military.rs and one in tech.rs now state the new money flow (pay to crews and researchers, hand-out reduced by what the budget sets aside). Two scenarios had lost their premise because fleet maintenance keeps an arms industry alive: the blockaded yard now explicitly has no local part works, and the order in an untouched economy is measured against a live market (each part at least 1.1 times dearer at its peak, 1.5 times on average; measured minimum 1.24) and by arms capital averaged over two years instead of on the last day.
8. The arrears-in-war test needs both sides arming to four fleets and passes if two of three seeds show arrears; with smaller programmes belligerents rarely fall into arrears.
9. I once ran `git checkout -- <five files>` to discard my own uncommitted edits. It restores files and changes no branch.

Memory: I read two notes. The handover note says the game lane merges core commit 8758cd7; I used that only to choose the reference for API changes. The lean-review note did not apply to this task. Worth remembering, all measured in this session: (a) the economy is demand-limited, so state purchases flow back as wages and only where state money lands decides household welfare; (b) ranking Kontor capital before the fleet causes pay defaults; (c) cargo test -p sim takes about four minutes here, the ignored 150-year test another four.

## open_issues

- No command scraps ships or disbands a fleet. With real upkeep a community in deficit can only raise taxes, borrow or lose ships; a player needs this before the budget is playable.
- No military AI, and the AI does not read its budget: it borrows only at war, never sizes its fleet, and its war chest still follows the old treasury-growth estimate.
- Spare parts missing on the market, as opposed to missing money, have no effect on the ships; only an unfunded fleet claim has (arrears). Fuel is the only supply that runs dry far from home.
- Population after 60 years is 5.4 % lower than before the budget. If that is unwanted, the levers are the research share (13 % of revenue for the whole game) and upkeep; the civil and hand-out shares do not change it.
- The starting fleet's share of revenue falls from 8.7 % in the first ten years to 3.6 % at year 60 because the economy grows and the fleet does not. Aim c holds for the first ten years as specified.
- Economy.staff and Economy.budgets of a community that ends are not handed over; the succession package has to move the staff account with the treasury.
- The Kontor floor (600) and refill (1500) are constants in economy/capital.rs, as they were before, not levers in balance.ron.
- Half of the starting treasury above the reserve is handed out on day 0, as before the budget; the AI's colony fund protects only one founding.
- Several new and restated tests are pinned to seeds (war arrears, the blockaded yard, the order in an untouched economy). Merging the succession and siege branches will shift trajectories; I left margins, but they may need re-pinning.
- headless fleets --order still takes designs only, no option for BuildClass.
- crates/game was not built and needs the follow-up listed under game_api_changes, plus a budget view.

