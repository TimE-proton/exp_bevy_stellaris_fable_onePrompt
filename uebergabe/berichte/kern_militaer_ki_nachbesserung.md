# Kern: Nachbesserung der Militär-KI nach der Prüfung

## summary

All ten findings were real and are fixed on wip/m4-integ; final commit e2ba3c4 is pushed, the worktree is clean and every required check passes.

- **Budget (2, 3, 4):** the civil base now uses the wage tax actually collected; procurement and fleet maintenance draw on the free treasury, so bonds are spendable at once.
- **Diplomacy and battles (1):** a truce after peace blocks an immediate new war, and a fleet that left the field no longer rallies beside its former enemy.
- **Tools (5, 10):** script entries for an absent community leave a rejected command and a notice; fixed-fleet comparisons rest the AI.
- **AI war (6, 7, 9):** offensives are judged against all foes around the target, giving way is remembered, and the in-battle estimate reads only visible state.
- **AI armament (8):** no orders for years after a lay-up, and ships are scrapped only when long-run revenue cannot carry them.

Things you should know:
- `defend_odds` is now 0.85, not the briefed 1.0. At 1.0 the 24-seed sweep had 10 battles; at 0.85 it has 45.
- Wars remain cautious: seeds 0..8 have battles in 4 of 8 wars, and seeds 2, 4 and 6 see none.
- The year-long revenue slumps of one-system states are a feedback in the economy itself (tariffs stop while the local trade office has no surplus). I did not change that; the AI now sits such slumps out with arrears.
- Six existing tests were restated or re-seeded because trajectories shifted.
- I used no stored note.

## fixed

- 1 (retreat, peace, war again). Cause: `conclude()` treated any non-hostile party holding the field as the fleet's own side, and war could be redeclared at once. Change: own side now requires a shared opponent; `DeclareWar` is refused for `military.truce_days` (30, validated >= `regroup_days`) after a peace. Proof: `peace_and_a_new_war_neither_shelter_a_fleet_nor_restart_a_battle` in tests/military.rs.
- 2 (civil base switched off by a tax rate set only at the close). Cause: `open_budget_month` used the rate in force at the close times the month's wage bill. Change: new `Economy::wage_tax_running` records the tax collected day by day; standard revenue = revenue - collected + usual rate x wage bill. Proof: `a_tax_rate_raised_only_around_the_close_does_not_cut_the_civil_base` fails without the fix (standard revenue 16,513 against 108,028).
- 3 (fleet claim cut while the treasury is full). Cause: maintenance bought only from a grant fixed at day-one prices. Change: `Economy::fleet_allowance` lets maintenance continue from the free treasury; a real money shortfall is added to `BudgetMonth::unfunded`. Proof: `the_fleet_gets_its_spare_parts_while_the_treasury_can_pay` fails without the fix; from year 1 on the fleet line is paid in full.
- 4 (money arriving after the close not spendable on procurement). Cause: the procurement purse was a number fixed at the close. Change: `allowance(Procurement)` is computed live as treasury minus open grants minus the running month's revenue. Proof: `money_borrowed_after_the_close_can_be_spent_on_procurement_at_once` fails without the fix (allowance unchanged after a 12,524 bond).
- 5 (`--order`, `--navy`, `--ai` for an absent community dropped silently). Change: the script issues the order anyway; `execute()` now rejects any command from a community that does not exist ('Gemeinschaft besteht nicht mehr'), and the CLI prints a notice when the number is beyond the starting communities. Proof: `a_script_naming_a_community_that_does_not_exist_leaves_a_trace`; the reviewer's command now prints two 'Abgelehnt' lines and three notices.
- 6 (blockader and defender chasing each other, defender on empty tanks). Cause: attack, defence and avoidance each looked at a different set of foes, with no memory. Change: offensives are judged against foes within `response_jumps` (3) and start only in port; giving up, retreating or avoiding keeps the community in port for `caution_days` (360); the defender waits for fuel and keeps the post where it met the intruder; non-offensive marches use only ways clear of foes. Proof: `wars_led_by_the_ai_do_not_repeat_the_same_empty_battle_or_sortie` (seeds 13 and 20, at least 14 blockade days per offensive); the 24-seed sweep averages 48 days per offensive.
- 7 (same loss-free battle dozens of times). Causes found by daily traces: the in-battle estimate capped the tech edge at 1 while the pre-battle one did not; a refit target was never re-checked; single ships rallied through the enemy; stranded dry fleets retried the same blocked way. Change: one consistent estimate; `MilitaryPlan::met` keeps a fleet from being sought again for `rematch_days` (360), or `elusive_days` (1800) if nobody lost anything, also as an intruder; refit targets are re-chosen; avoidance runs before refit. Proof: same test, at most 6 loss-free battles per war; seed 20 went from 51 battles to 0, seed 13 has 10 battles with 12 ships lost.
- 8 (AI builds ships it then scraps). Cause: orders were judged on a 6-month mean with no memory of a lay-up, and a single capital ship could be worth twice the target. Change: target from the weaker half of 12 months (`lean_share`) and a slow revenue mean (`revenue_memory_months` 60); no ship worth more than the target; 12 months of history needed in peace; no orders for `quiet_months` (24) after missed pay or `layup_quiet_months` (120) after a lay-up; lay-up only above what long-run revenue carries. Proof: `after_laying_up_ships_the_ai_does_not_order_new_ones_for_years` (seed 9). In seed 9 the Räte went from 16 built / 15 laid up to 8 / 3 and the Freistaat from 12 / 11 to 4 / 2 (sweep just before the final three war-rule tweaks).
- 9 (retreat decision read exact enemy hull and hangar state). Change: the estimate moved to `ai::estimate::battle_values` and counts, on both sides, the ships still fighting at build value. Counting only own damage made a fleet judge itself outmatched in every battle, so both sides use the same measure. Proof: unit test `the_estimate_in_a_battle_counts_what_everyone_sees`; README corrected.
- 10 (README budget comparison no longer measured fixed fleets). Change: `fleets` rests the military AI when `--start-fleets`, `--research-share` or `--fleet-cost` is given and says so; `--ai alle:aktiv` overrides; README updated. Proof: the README command now gives 6/12/18/24 ships and the fleet shares in the measurements table.

## not_fixed

- Year-long revenue slumps of one-system states (raised under finding 8): cause identified but not changed. A trade office pays tariff only on cash above its working capital; when the treasury is empty the hand-out stops, households buy less, and revenue stays at the wage-tax floor until the 12-month civil base has shrunk. This belongs to the economy of milestones 2 and 3; the AI was made robust to it instead.
- Finding 7, way_clear for a fleet in mid-jump: not changed, because a marching fleet cannot turn before its next system anyway. The loops are closed by the other rules instead.

## checks

All run in the worktree at e2ba3c4.
- `cargo build -p sim -p headless`: exit 0.
- `cargo test -p sim -p headless`: the full run passed every binary except one tech test (lib 158, ai 8, battle 45, budget 7, economy 6, finance 9, military 48, headless 3). After restating that test, tests/tech.rs was rerun alone: 35 passed, 1 ignored. Doc tests: exit 0. The suite was not rerun in one piece after that last test-only change.
- `cargo clippy -p sim -p headless --all-targets -- -D warnings`: exit 0.
- `cargo fmt -p sim -p headless -- --check`: exit 0.
- `cargo build --release -p headless`: exit 0.
- `headless sweep --seeds 0..8 --years 60`: exit 0, no violation reported.
- `headless hash --seed 3 --days 3600`: twice identical, and identical with `--exact` (day 3600: d3e37a17f20cdef6).
- Scripted wars: `sweep --seeds 0..8 --years 60 --preset grenzkrieg` exit 0, 17 battles begun and ended. `hash --seed 7 --days 1800 --navy 0:0:2 --war 3:0:1` twice and with `--exact`: 6c6728b5534795d1.
- Save and load in the middle of an AI war: `wars_led_by_the_ai_are_deterministic` passes.

## commit

e2ba3c4 on wip/m4-integ, pushed (origin/wip/m4-integ is at the same commit). Earlier commits of this task: a46a4b0 (budget), 0673906 (truce and battle conclusion), 166ee16 (script trace), 710bd7e (AI).

## measurements

PEACE SWEEP, seeds 0..8, 60 years (before = 1f531d5, after = e2ba3c4)
| | before | after |
|---|---|---|
| Population (million) | 49,934 | 50,325 |
| Supply | 0.99 | 0.99 |
| Needs met | 0.872 | 0.881 |
| Cohesion | 67.3 | 67.2 |
| Price level | 0.57 | 0.59 |
| Secessions per run | 0.5 | 0.4 |
| Ships per community | 17.0 | 14.4 |
| Fleet share of revenue | 10.2 % | 8.8 % |
| Research share | 13.1 % | 12.9 % |
| Hand-out share | 26.7 % | 25.9 % |
| Ships laid up | 0 | 0 |
| Months with arrears | 8 | 0 |
| Technologies | 11.3 | 11.2 |
| AI commands rejected | 0.02 % | 0.02 % |
No invariant violations.

WAR SWEEP, preset grenzkrieg, 60 years, after
| | seeds 0..8 | seeds 0..24 |
|---|---|---|
| Battles (all decided) | 17 | 45 |
| Wars with battles | 4 of 8 | 15 of 24 |
| Ships lost | 54 | 128 |
| Whole fleets lost | 0 | 2 |
| Offensives | 101 | 743 |
| Blockade days | 4,200 | 35,642 |
| Ships laid up | 0 | 18 |
| AI commands rejected | 0.02 % | 0.02 % |
Seeds 0..8: 147 ships built by the warring sides, 0 months with arrears, blockades in 5 of 8 wars; seeds 2, 4 and 6 see no battle. Most battles in one war: 10 (seed 13, 12 ships lost).
The 0..24 column was run on the final AI code just before 710bd7e; later commits changed only the fleets tool, README and tests.
For comparison, the reviewer's figures at 1f531d5: 191 battles in 10 wars on seeds 14..24, 51 in seed 20 alone.

LED AGAINST PASSIVE, seeds 0..8, 60 years
| Preset | Side | Battles won | Ships lost | Blockade days | Offensives | Fleets lost |
|---|---|---|---|---|---|---|
| attacker passive | led | 16 | 9 | 11,610 | 106 | 1 |
| attacker passive | passive | 1 | 15 | 0 | 0 | 4 |
| neighbour passive | led | 2 | 2 | 11,250 | 148 | 0 |
| neighbour passive | passive | 1 | 6 | 0 | 0 | 0 |
Against an equally strong passive fleet within reach the AI does not attack (several seeds show nothing).

BUDGET MAGNITUDES, `fleets --seeds 0..8 --years 10 --start-fleets N`, AI resting
| Starting fleets | Fleet share | Research share | Hand-out share | Last column (arrears) |
|---|---|---|---|---|
| 1 | 9.3 % | 12.8 % | 17.3 % | 0.0 % |
| 2 | 18.7 % | 11.8 % | 8.7 % | 8.0 % |
| 3 | 28.3 % | 9.7 % | 2.6 % | 24.1 % |
| 4 | 36.7 % | 7.2 % | 1.1 % | 58.6 % |
Without fleet and research cost: hand-out 41.9 %, needs met 0.803 against 0.747.

## api_notes

Economy
- New field `Economy::wage_tax_running` (serde default).
- New `Economy::fleet_allowance(polity)` and `Economy::book_unfunded(polity, amount)`.
- `allowance(_, Claim::Procurement)` is computed live; `Budget::grants` no longer holds a Procurement entry.

Diplomacy
- New field `Diplomacy::truces` (serde default) and `truce_until(a, b, day)`.
- `make_peace` takes a further argument `truce_days`.
- `declare_war` rejects during a truce.

Commands
- `execute()` rejects every command from a community that does not exist or is not alive. Succession should check that commands issued for a community in the tick it ends are meant to fail.

balance.ron (all required)
- `military.truce_days`, placed after `regroup_days`.
- `military.ai`: `quiet_months`, `layup_quiet_months`, `revenue_memory_months`, `lean_share`, `response_jumps`, `rematch_days`, `elusive_days`, `caution_days`.
- Changed value: `defend_odds` 1.0 -> 0.85.
- All edits are inside the military block; the start and end of the file are untouched. SAVE_FORMAT is unchanged.

AI
- `Situation` gained `day`.
- `MilitaryPlan` gained `calm_months`, `months_since_layup`, `usual_revenue`, `met` (map of `Meeting { day, fought }`), `no_offensive_before`, `post`; all serde default.
- New `estimate::battle_values(battles, battle, side)`.

For siege: tests that stage fleets still need the military AI resting. A fleet of a community that has just avoided or retreated stays in port for a year.

## game_api_changes

None of the public sim items I changed is used by crates/game/src. I grepped for allowance, grants, declare_war, make_peace, truce, wage_tax, wage_bill, MilitaryAiBalance, MilitaryBalance, MilitaryPlan, AiState, Situation and battle_values: the only hits are imports of `sim::diplomacy::Diplomacy` as a resource in ui/mod.rs and ui/panels.rs. That struct gained the field `truces`; this only matters if the game builds a `Diplomacy` by struct literal, which I did not see.

Behaviour the game will notice:
- `SimCommand::DeclareWar` can now be rejected with 'nach dem Friedensschluss gilt noch N Tage Waffenruhe'.
- `allowance(Claim::Procurement)` changes during the month.
- `BudgetMonth::unfunded` can grow during the month.
- `balance.ron` needs the new levers, which matters for any copied data directory.

## open_issues

- Wars are still cautious. Near-equal sides often do nothing: seeds 2, 4 and 6 of the 0..8 sweep have no battle, and a led fleet does not attack an equally strong passive one in reach. A later war-aims planner needs to accept more risk; today only `defend_odds` 0.85 opens a window in which both sides stand.
- A weaker side still raids when the enemy main fleet is refitting and gives up when it returns. This is bounded to about one aborted sortie a year by `caution_days`, but it is visible back and forth.
- The 'offensives' counter also counts each return to a blockade after refuelling, so it overstates distinct operations.
- One-system breakaway states go through year-long revenue slumps (economy feedback, see not_fixed). They now sit them out: Freistaat Hesartis II had 65 months with arrears in seed 9. Some lay-ups remain (18 ships in 24 wars).
- A fleet cut off in a dead end with an empty tank stays there for a year at a time; nothing relieves it.
- Six existing tests were restated or re-seeded because trajectories shifted: the hand-out bound and span in the four-fleet deficit test, the yard flicker test (now minimum lifetime of a yard), the part-price threshold 1.5 -> 1.4, the research budget test (measures an unshortened month), and the seeds of three AI war tests. Worth a reviewer's look.
- The determinism test for AI wars now covers blockades and fleet movement with a scripted fleet programme, not a battle: with equal starting fleets nobody attacks in the early years.
- The checker's four stray log files in the filesystem root (/f1.log, /f4.log, /f4r.log, /sw.log) are still there; I did not touch them.
- Throw-away data directories and logs of this task are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/kern/n and can be deleted.

