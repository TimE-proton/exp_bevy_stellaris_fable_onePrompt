# Gefechtsrechner: Bericht der Nachbesserung (wip/m4-taktik, 35801ce)

## summary

I fixed all 18 confirmed findings in the battle resolver at their root, added 14 tests and strengthened existing ones, then rebalanced the data and measured again. All four required commands pass: cargo build -p sim, cargo test -p sim (battle integration tests 45 of 45), clippy with -D warnings, and the release build of headless. Commit 35801ce on wip/m4-taktik in /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/taktik.

Main changes:
- Range keeping (finding 1): a fleet now measures its distance from the centroid of the ships that set its range, not from the anchor, and the anchor no longer runs ahead of its ships. Carriers without fighters move into the line.
- Stalemates (13): if neither side can get through the other's shields, both sides disengage together and nobody wins. A side that cannot hurt the enemy still withdraws alone if the enemy is hurting it.
- Mirror bias (9): near-equal distances in nearest-target searches are now broken by a random value per shooter-target pair instead of grid order. Over 2000 seeds per fleet no side bias remains.

Choices beyond the findings:
- Commit trailer: I used "<Ausweichmodell>" because the session attribution instruction outranks the script text, which asked for "<Hauptmodell>".
- Carrier value: carriers counted their fighters twice. BattleShip.value now leaves the fighters out, so initial_value equals build value.
- Retreat-by-losses counts ships only, not fighters; Balanced retreats after losing just under a third of ship value.
- Carrier kiting is a cliff: carriers beat battleships only because their fallback distance (standoff 2100 x 0.8 x 0.6, about 1008) exceeds heavy-gun range 900. At standoff 1900 the result flips to about 7 %. Documented in balance.ron.

## fixed

- 1 Carrier range: distance measured from the centroid of range-setting ships (Combatant.range_center/range_offset), anchor tethered to its ships, carriers move into the line once no fighters remain (Combatant.carriers_in_line, assign_slots parameter). Test carriers_without_fighters_close_to_their_gun_range and fleets_keep_their_range_from_their_own_ships.
- 2 PD-only ships: point defence counts as armed only if an enemy has small ships, launchable fighters, missile weapons or missiles in flight; unarmed ships rout only while an enemy is still fighting. main_range falls back to PD range. Test a_ship_left_with_point_defence_only_breaks_off_against_large_ships.
- 3 Hangars: has_fighters/can_launch require an Active carrier with an intact hangar module; used in fleet profile, armed and fighting_value. Test carriers_with_wrecked_hangars_count_as_carriers_without_fighters.
- 4 Timeout fate: one shared rule (adrift_lost) in check_end and results(); drifting ships of a side not fighting while an enemy fights are lost at the time limit too. Test ships_adrift_at_the_time_limit_count_as_lost_in_the_tally_and_the_results.
- 5 Leftover fighters: settle_fighters at battle end counts fighters without hangar room as lost (and subtracts them from escaped). Test fighters_are_accounted_for_after_every_battle (invariant initial = lost + returned).
- 6/17 Breakaway counts down by dt; first_launch, launch_interval and breakaway_time are in RON. Test the_breakaway_of_fighters_lasts_the_same_time_at_any_tick_length (dt 0.1 and 0.05).
- 7 morale_per_weariness wired up through CombatantSetup::with_war_weariness(data, weariness 0..100). Test war_weariness_lowers_the_starting_morale.
- 8/12 Mirror test: 400 seeds per fleet, 800 for speer:5, plus pooled; 3-sigma bounds on win share and on loss difference; undecided only as stalemate. Verified: re-introducing the old index stagger makes the test fail.
- 9 Side bias: tie_break (pair_jitter) in pick_aim fallback, PD missile and ship search, guide_missile. 2000 seeds: pooled 49.6 % for A.
- 10/15a Save/load: saves every 41 ticks wherever shots are in flight; at least 8 checkpoints, 2 with missiles, 4 with fighters, each continued via postcard and RON and compared byte for byte.
- 11 Counter test at equal value: the favoured side is at most 3 % more valuable (dolch 13 vs horst 2, ratio 1.02), 24 seeds per assignment. Rebalanced so corvette beats carrier at equal value (about 95 %).
- 13 Stalemate: mutual disengagement without a winner (Combatant.stalemate, Outcome.stalemate); stalemate_pressure lever. The carrier mirror now ends as a stalemate (1985 of 2000), not by tie-break. Tests carrier_mirrors_end_in_stalemate_not_in_a_coin_toss, a_stalemate_ends_without_a_winner, a_side_that_cannot_hurt_its_enemy_withdraws_alone.
- 14 Mixed-fleet test: both assignments, comment matches what was measured, the known weakness against battleships is stated openly.
- 15b Retreat test: at most one side sets retreat_time and there is no stalemate. 15c PD test: corvettes and torpedoes in the scenario, at least 20 hits on corvettes and on missiles, never on a cruiser.
- 16 Balance levers in RON and validated at load: module_hits, magazine_blast, tank_leak, damaged_weapon_below/cooldown, dry_speed, loss_shock, point_defense_signature_weight, small/tiny_signature, slow_tracking, point_defense_size_bias, targeting, range_bands, flanking, frigate_leash, fighter_min_speed, value_hull_floor. The class lists for point defence, escorts and missiles are now signature thresholds.
- 18 New tests for fighter rearming, fuel, firing arcs, range keeping, stalemate, the per-tick budget (large battle under 20 ms per tick) and small even battles (speer:3 and bastion:2: the loser loses everything in fewer than half). Also fixed: lost_engines test previously moved only 7 units because the battle ended right away without an enemy (cruise helper).

## not_fixed

- bench 2000 (about 1000 ships per side) runs into the time limit at 901 s. Mean 937 µs per tick, but the worst tick took 70 ms, above the 50 ms budget. Below that everything is fine (20 vs 20 mixed: 75 s, no time-outs). Big fleets apparently cannot finish within 900 s; not investigated further.
- Frigate mirror: about 11 % stalemates (frigates only have lasers and no longer get through shields once they have losses). Honest and allowed in the test, but frigate-only battles are drawn out (mean 170 s).
- The mixed test fleet (bastion:1, speer:2, waechter:1, dolch:1) loses to 2 battleships about 75 % of the time. Stated openly as a known weakness, not a target.
- Carrier versus battleship is a cliff with respect to carrier_standoff (see summary). Range technology on the battleship side could flip it.

## checks

cargo fmt -p sim -p headless: ok. cargo build -p sim: exit 0. cargo test -p sim: exit 0 (lib 46, battle 45, other test files 6 and 9, all green). cargo clippy -p sim -p headless --all-targets -- -D warnings: exit 0. cargo build --release -p headless: exit 0. Regression check: the old stagger by index makes mirror_matches_are_fair fail (speer loss difference -0.054 ± 0.015); reverted afterwards. Temporary probe file crates/sim/tests/zz_debug.rs removed.

## measurements

Matrix (budget 22804, 24 seeds per assignment, cell = wins of the row as A / as B): frigate vs corvette 24/24; cruiser vs frigate 24/24; cruiser vs corvette 24/24; battleship vs cruiser 21/23; battleship vs frigate 24/24; corvette vs battleship 20/18 (200 seeds: 82 %/84 %); corvette vs carrier 24/22 (dolch:13 vs horst:2 at equal value, 200 seeds: 94/96 %); carrier vs battleship 24/23 (100 seeds 100/98 %); frigate vs carrier 24/24. Mirror over 2000 seeds, A share of decided battles: corvette 48 %, frigate 50 % (228 stalemates), cruiser 51 %, battleship 49 %, mixed 50 %, carrier 1985 stalemates; pooled 49.6 %, no time-outs. Pacing: 3 vs 3 cruisers 96 s after contact on average (longest 243 s, 100 seeds); 20 vs 20 mixed 75 s, no time-outs; loser wiped out in small even battles: speer:3 3 %, bastion:2 2 %; winner loses 4 %, loser 42 %. Bench 400: 416 µs per tick on average, 9.2 ms worst. Bench 2000: 937 µs on average, 70 ms worst, time limit reached.

## api_notes

Public API stable apart from additions; changes in meaning are noted. New: Combatant.stalemate, range_center, range_offset, carriers_in_line (all serde default); Outcome.stalemate; Battle::has_fighters(data, side); CombatantSetup::with_war_weariness(data, f64). Changed meaning: BattleShip.value excludes on-board fighters for carriers (initial_value now equals build value); fighting_value counts hangar fighters only if they can launch; the retreat loss share ignores fighters; BattleShip.armed counts point defence only against small or missile-carrying enemies; unarmed ships rout only while an enemy is fighting; Disabled ships of a side not fighting while an enemy fights are destroyed at the time limit too (results Lost); fighters left over at the end are Destroyed and counted in lost; a stalemate sets retreat_time on both sides and ends with no winner. Renderer: carriers move from the rear slot into the line once no fighters remain. Data changes: dolch carries flak instead of impulslaser; traeger speed 32; schwere_kanone 74; torpedo 135; leichter_torpedo 45; bordkanone 1.85; damage_scale 0.3; retreat_loss (0.6, 0.32, 0.22); jump_out_time 30; carrier_standoff 2100; many new levers in the battle section of balance.ron. headless: --versus-all needs only --a; summaries list stalemates (Patt).

## commit

35801ce

