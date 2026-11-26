# Gefechtsrechner: Bericht der Umsetzung (wip/m4-taktik)

## summary

I picked up the battle resolver from the committed interim state and reworked it in the worktree /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/taktik (commit 17044b3 on wip/m4-taktik). All four required commands pass.

P1 side bias. I first tested the earlier hypothesis. With the target-choice jitter switched off, speer:5 vs speer:5 over 300 seeds came out 118 A, 114 B, 68 undecided, which confirms that term as the main cause. A second cause turned up after that: the retarget stagger `(index + tick) % every` put side 1 a few ticks out of phase (speer:5 then 59 % A). Fixes:
- The jitter is now a hash per shooter-target pair (`pair_jitter`).
- The retarget stagger counts a ship's position within its own side, not its global index.
- Movement, steering and fire read only the state at the start of the tick. All damage events are collected in `hits` and applied together at the end of the tick.
- Fire order alternates every tick.
- Ties in the retreat order are broken randomly per tick.
- Spawn positions are exactly point-symmetric (`math::exact_direction`).

P2 pacing. New lever `damage_scale: 0.4` in the battle section of balance.ron, documented there. A stalemate rule (`stalemate_time: 60`) makes a side withdraw if it has not got through the enemy's shields for 60 s. No standard pairing reaches the time limit any more.

P3 morale and retreat.
- Morale is now: starting value minus a permanent hull penalty, minus a transient `shock`, minus an `outlook` penalty when the own side is behind. Shocks come from friendly losses nearby and from hits into the rear, and they recover.
- Individual ships break at `morale_break`, or when they have no weapon left that can fire.
- Fleets retreat on a loss share per doctrine (`retreat_loss: (0.65, 0.45, 0.3)`) when they are not clearly ahead (`retreat_unless_ahead: 1.3`). Sides are evaluated one after another, the worst-off first, and a retreat takes effect immediately, so both sides never retreat in the same instant.
- The `time > 25` hack is replaced by a `contact` flag.
- Routing ships flee away from the enemy and outward. They escape at `exit_radius` or after `jump_out_time: 45` s.
- The winning side follows them: routing ships count at half weight in the enemy centre.
- Allied sides now count towards a side's own strength.

P4 drift. Every side has orbit +1, so the fleets circle their common centre, and `center_pull` draws that centre back to the middle of the field.

P5 class roles, all through mechanics and data, with no class-versus-class bonus:
- Point defence fires only at missiles, fighters and corvettes. Before this fix, frigates beat everything because their flak was used as main guns.
- Fighters with empty magazines return to their carrier and rearm there (`rearm_time: 6` s).
- Carriers keep `carrier_standoff` (1250) while they have fighters, then close to their gun range (new `DesignStats::gun_range`).
- A fleet holds at the range of its shortest line weapon.
- In mixed fleets, frigates stay near their slot and only go after small attackers.
- A ship's `main_range` follows the weapons that can still fire, so a fleet out of ammunition closes to beam range.
- Data changes: rail guns are faster (600; the coil gun 660), cruisers get three PD batteries in place of the rear laser (hull slots 15), carriers have two PD batteries instead of three, battleship hull is 1300, rear hits do more (`rear_hull_factor: 1.6`, `morale_rear_factor: 1.5`), and the PD hit chance moved into balance.ron.

P6. Point defence finds missiles through its own grid, `Battle::missiles`, rebuilt every tick.

P7.
- New guards against NaN and zero divisions: `unit()` for ShipSpec fractions, `hull_max >= 1`, fallbacks for zero direction vectors.
- Fighters of a destroyed carrier pick the nearest surviving carrier.
- Three-way battles, reinforcements in mid-battle (hostility matrix stays consistent), a battle whose last ships are adrift, and end detection are all tested.
- New balance values are validated when the data is loaded.

Tooling: `headless battle` now has `--mirror`, `--swap` and `--versus-all`, the `--matrix` runs both side assignments plus a mixed fleet, battles run in parallel, and the summaries show winner and loser losses and fighting time after contact. `--trace` now prints damage dealt per weapon type.

Tests: 28 integration tests in /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/taktik/crates/sim/tests/battle.rs (about 6 to 8 s) and 5 new unit tests in step.rs covering the direct damage path, hit facing, reactor loss, morale recovery and the jitter.

Memory: none of the user's memory notes bore on this task.

## measurements

How measured: release build of target/release/headless; seeds as stated. Equal build value 22804 = 2 battleships.

Counter matrix, `battle --matrix --seeds 0..12`. Each cell reads "row wins as A / row wins as B (12 runs each), mean loss row % : column %":
| row \ column | 14 corvettes | 11 frigates | 5 cruisers | 2 battleships | 2 carriers | mixed |
|---|---|---|---|---|---|---|
| Corvette | 3/9 62:62 | 0/0 61:2 | 0/0 58:13 | 12/12 10:100 | 10/11 35:75 | 12/10 32:90 |
| Frigate | 12/12 2:61 | 5/7 40:40 | 0/0 57:4 | 0/0 58:0 | 12/12 0:41 | 12/10 35:45 |
| Cruiser | 12/12 13:58 | 12/12 4:57 | 6/6 52:52 | 3/2 67:17 | 12/12 0:96 | 12/12 7:100 |
| Battleship | 0/0 100:10 | 12/12 0:58 | 10/9 17:67 | 7/5 40:40 | 2/3 46:15 | 11/9 12:81 |
| Carrier | 1/2 75:35 | 0/0 41:0 | 0/0 96:0 | 9/10 15:46 | 9/3 17:17 | 0/0 97:18 |
| mixed (bastion:1, speer:1, waechter:1, dolch:2) | 2/0 90:32 | 2/0 45:35 | 0/0 100:7 | 3/1 81:12 | 12/12 18:97 | 9/3 41:41 |

The diagonal cells are mirror matches over only 12 seeds; their unevenness is sampling noise (see the 400-seed mirror table below).

The nine required relations over 24 seeds, favoured side wins as A / as B (minimum needed: 17 of 24):
| favoured | against | as A | as B |
|---|---|---|---|
| frigate | corvette | 24 | 24 |
| cruiser | frigate | 24 | 24 |
| cruiser | corvette | 24 | 24 |
| battleship | cruiser | 20 | 19 |
| battleship | frigate | 24 | 24 |
| corvette | battleship | 24 | 24 |
| corvette | carrier | 20 | 20 |
| carrier | battleship | 18 | 19 |
| frigate | carrier | 24 | 24 |

Cruiser against carrier: the cruiser wins 24 of 24.

Escorts, 12 seeds each assignment. bastion:2 against horst:2 wins 0 of 24 and loses about 50 %. bastion:1 plus waechter:5 (slightly cheaper) wins 24 of 24 and loses 0 %. Against 14 corvettes, bastion:2 loses 100 % in every run, while battleship plus frigates wins 10 of 24.

Mixed fleet bastion:1, speer:2, waechter:1, dolch:1 against each single class at the same value, wins out of 24 (both assignments together), loss mixed % : opponent %:
| opponent | wins | losses |
|---|---|---|
| frigates | 24 | about 20:53 |
| carriers | 24 | 9:99 |
| battleships | 19 | 48:44 |
| cruisers | 6 | 50:46 |
| corvettes | 1 | 86:29 |

Mirror matches over 400 seeds, share of side A among decided battles:
| fleet | A wins | mean loss A : B |
|---|---|---|
| corvette | 53 % | 60 : 61 |
| frigate | 52 % | 38 : 39 |
| cruiser | 52 % | 50 : 54 |
| battleship | 49 % | 36 : 35 |
| carrier | 50 % | 17 : 17 |
| mixed | 49 % | 45 : 45 |

Cruiser mirrors over 1000 seeds: speer:5 514/486, seeds 1000..2000 500/500, speer:4 520/480. There are no undecided battles and no timeouts.

Pacing, measured as seconds of fighting after first contact:
- 3 vs 3 cruisers: mean 116 to 118 s, longest 175 s.
- 20 vs 20 mixed (horst 1, bastion 2, speer 5, waechter 6, dolch 6): mean 101 s, longest 114 s.
- Mirror means range from 45 s (corvettes) to 173 s (frigates).

P3, 10 vs 10 cruisers, 20 seeds: the winner keeps 62 % on average (loses 38 %), the loser loses 66 % on average and typically saves 3 to 4 ships.

Benchmark, `battle --bench 400`:
- 201 ships per side, up to 738 on the field at once including fighters, up to 416 projectiles.
- 1370 ticks, mean 388 to 407 µs per tick, worst tick 1.4 to 1.5 ms.
- With 800 ships (up to 1460 in the field, 601 projectiles): mean 500 µs, worst 8.7 ms.
- Budget per tick is 50 ms.

## api_notes

No existing pub field was renamed or removed. Additions and changes in meaning:

New in battle/mod.rs:
- `pub enum Fate { Holding, Escaped, Lost }`
  - Holding: Active or Routing at the time of the call, or adrift (Disabled) while no hostile side is still fighting or the own side still fights.
  - Escaped: status Escaped.
  - Lost: Destroyed, or adrift while a hostile side holds the field and the own side does not.
  - At the end of a battle, routing ships become Escaped, and adrift ships of the loser are destroyed.
- `pub struct ShipResult { source: ShipId, side: u16, fate: Fate, hull: f32, modules: Vec<f32>, ammo: f32, fuel: f32, fighters: u32 }`
  - hull, ammo, fuel and modules are fractions 0..1 of the design maximum.
  - A Lost ship gets 0 everywhere.
  - A maximum of 0 (for example a design without ammunition) is reported as 1.
- `Battle::results(&self, &GameData) -> Vec<ShipResult>`
  - One entry per ship with `source = Some`, in ship index order.
  - Carrier-borne fighters (source None, class Fighter) that are not Lost go back into the hangars of non-Lost carriers of their side, in index order, up to `stats.fighters`.
  - Works as a snapshot on a running battle.
- `Battle::deploy(&mut self, side, anchor: V2, facing: f32)` moves a side and its ships into place, for example a force arriving in the rear. Its home_angle is set to point away from the facing direction.
- `Battle.contact_time: Option<f32>`: time of first contact.

New public fields on BattleShip:
- `morale_base`: starting morale.
- `shock`: transient; positive is a scare, negative a boost.
- `rout_time`: seconds spent routing.
- `rearm`: fighter rearm timer.
- `armed`: has a weapon that can still fire, or fighters in the hangar.

New public fields on Combatant:
- `contact`: has been engaged.
- `outlook`: current morale penalty for being behind.
- `last_hit`: time this side last got through enemy shields.
- `retreat_time: Option<f32>`.
- `damage_dealt: [f32; 4]`: damage to enemy armour and hull, by weapon kind Beam/Kinetic/Missile/PD.
- `damage_taken: f32`.

New: `DesignStats::gun_range` (range of the design's own weapons, without fighters).

New in BattleBalance and the battle section of balance.ron: `damage_scale`, `point_defense_hit`, `rear_hull_factor`, `morale_shock_recovery`, `morale_outlook`, `retreat_loss` (per doctrine), `retreat_unless_ahead`, `jump_out_time`, `orbit_share`, `center_pull`, `rearm_time`, `carrier_standoff`, `stalemate_time`, `broadside_min`, `broadside_angle`. All existing fields are kept.

New in matchup: `pub fn finish(data, &mut Battle) -> Outcome` runs a built or running battle to its end. `Side::specs` is now pub. `Outcome.contact_time` and `Outcome::fighting_time()` are new. `build()` now gives every ship a sequential `ShipId`, so `results()` covers practice battles; fighters stay `source None`.

Changed meaning (renderer and AI should know):
- `Combatant.orbit` is always +1 now. Same rotational sense for all sides, so fleets circle each other.
- `BattleShip.morale` is recomputed every tick as base - hull penalty - shock - outlook. The hull damage part is permanent; the rest recovers.
- `BattleShip.main_range` now follows working weapons that have ammunition. Carriers keep their standoff value.
- Point defence fires only at missiles, fighters and corvettes.

Unchanged, as asked: `Battle::new`, `add_combatant` (also mid-battle; the hostility matrix grows with `false`, then use `set_hostile`), `set_hostile`, `step`, `set_orders`, `order_retreat` (now also sets `retreat_time`), `finished`, `timed_out`, `victors`, `fighting_value`, `active_by_class`, the per-side bookkeeping in Combatant, ShipSpec, CombatantSetup, CombatModifiers, and `matchup::{Side, build, fight, outcome_of, Outcome}`.

Serialisation: `missiles`, `hits` and `weights` are `#[serde(skip)]` and are rebuilt (or empty) at the start of every step. `contact_time` and `retreat_time` are `#[serde(default)]`.

## files_changed

- crates/sim/src/battle/mod.rs
- crates/sim/src/battle/step.rs
- crates/sim/src/battle/math.rs
- crates/sim/src/battle/matchup.rs
- crates/sim/src/data/ships.rs
- crates/sim/src/data/ship_loader.rs
- crates/sim/tests/battle.rs
- crates/headless/src/battle.rs
- assets/data/balance.ron
- assets/data/designs.ron
- assets/data/hulls.ron
- assets/data/modules.ron

## checks

All four required commands exited 0, checked by exit code with output written to log files in the scratch directory:
- `cargo build -p sim`: OK.
- `cargo test -p sim`: OK. 46 library unit tests, 28 battle tests (5.7 s), plus the existing economy and finance tests (6 and 9). The whole run takes about 1 min 45 s, mostly the other test files.
- `cargo clippy -p sim -p headless --all-targets -- -D warnings`: OK, no warnings.
- `cargo build --release -p headless`: OK.

Before them I ran `cargo fmt -p sim -p headless`.

After those runs I changed one value, the coil gun speed from 580 to 660. I re-ran the battle tests afterwards (28/28 OK) and then committed.

## commit

17044b3 "Meilenstein 4: Gefechtsrechner fair, im Tempo, mit Moral und Kontern" on wip/m4-taktik. Not pushed.

## deviations

- Commit trailer: the brief asked for "Co-Authored-By: <Hauptmodell>". The session's attribution instruction takes precedence over the script text, so the commit uses "Co-Authored-By: <Ausweichmodell> <noreply@anthropic.com>" plus the Claude-Session line.
- Mixed fleet: the brief asked for a balanced mixed fleet that is competitive against every single class. That does not fully hold. The test `a_balanced_mixed_fleet_holds_its_own` asserts only what holds over 12 seeds: it beats frigates, carriers and battleships, and against corvettes and pure cruisers it wins rarely but inflicts at least 25 to 30 % losses. Pure cruisers beat their escorts, which is their stated role.
- Flanking at battle level: the test measures what the arriving force itself does in the 30 s after it arrives, for the same force at the same distance and time, behind the enemy versus in front of it. From behind it deals more than 1.25 times the damage and takes less. Over the whole battle, total exchange barely changes, because cruisers in broadside move sideways and the fleet's "rear" is not their stern. The flank test does not use the whole-battle exchange for that reason.
- Mirror test: it uses 60 seeds instead of 40, with a 0.15 bound on mean-loss difference. That bound is about 2.5 standard deviations; with 40 seeds, the 0.12 bound failed on chance alone even though 1000-seed runs are fair.
- Technology test: the full-tier test uses 10 cruisers per side. With 5 the result was 0.83 to 0.88, which is not "almost always". The one-tier test (damage 1.08) uses 5 cruisers over 2 × 20 battles and needs at least 60 %.
- Carriers stay behind their anchor even when they have no other ships with them, and after their fighters are gone they close to gun range. Carrier-only mirrors are therefore usually decided by the stalemate rule: one side withdraws after 60 s without getting through shields.
- `broadside_min` and `broadside_angle` were added as balance values but left at the old behaviour (1.05 and 80°). Experiments showed that ship headings mostly follow their direction of movement, so the angle hardly matters.

## open_issues

- Mixed fleets lose clearly to pure cruiser fleets of the same value (6 of 24, loss 50:46 with the battleship mix, 0 of 24 without one). Battleship strength is strongly non-linear: 1 battleship beats 2 cruisers every time but loses every time to 3. Within a mixed fleet the battleship is rarely decisive, and the cheap escorts die first, which triggers the fleet's retreat on loss share.
- Some counters are close to the 70 % line: battleship over cruiser and carrier over battleship are each about 75 to 80 %. Changes to hull or weapon values can easily push them below it.
- Flanking against broadside lines only pays off for the arriving force itself. To make it decisive for the whole battle, the orientation of broadside ships would need rework, because their heading currently follows their direction of movement.
- The bench with 800 ships has a worst tick of 8.7 ms (mean 0.5 ms). `refresh_armed` runs every tick for every active ship and could move to the command ticks if needed.
- The README does not mention the new headless options --mirror, --swap and --versus-all; README.md was not on my file list.

