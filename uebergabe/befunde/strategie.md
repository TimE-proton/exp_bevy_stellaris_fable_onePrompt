# Prüfbefunde: strategische Ebene (wip/m4-sim, 4b64d65)

Drei Prüfer, jeder Befund einzeln gegengeprüft. Die Nachbesserung ist nicht gelaufen: alle bestätigten Befunde sind offen (Paket A).

Zählung: 17 bestätigt, 3 widerlegt, 0 ungeprüft.

## Blickwinkel: determinismus

### determinismus 1 · bestätigt · major · Forced retreat after a lost battle can be cancelled: MoveFleet to the fleet's own system clears the retreat route, and the fleet then stays in the battle system, unattackable for regroup_days

Ort: `crates/sim/src/fleet/mod.rs:608`

**Beschreibung.** Found by code reading, not reproduced by a run (no CLI path issues a MoveFleet at a chosen tick).

conclude() (crates/sim/src/fleet/battles.rs:816-822) gives a beaten fleet `route = vec![target]`, `progress = 0.0` and `regroup_until = Some(day + regroup_days)`. Until the next day-start movement the fleet still counts as docked, because is_docked() is true for progress <= 0. order_move() only refuses fleets with `in_battle`; in its docked branch, `if fleet.system == to { fleet.route.clear(); return Ok(()) }` (mod.rs:608-611) deletes the retreat hop. Commands run in SimSet::Commands before movement, so this works even when the battle ends on the last tick of a day.

The fleet keeps `regroup_until`, so is_ready() is false. open_battles() builds `present` only from ready fleets (battles.rs:434-437), so nobody can start or join a battle against it for 30 days.

Two more ways to the same result:
- split_fleet() (mod.rs:717-738) copies came_from and regroup_until to the detached part but not the pending route, so that part stays behind.
- merge_fleets() lets ready ships join a regrouping fleet and inherit its immunity.

The existing game UI already offers the first route: crates/game/src/ui/fleets.rs:70 enables "Marsch zum Ziel" when `system != fleet.system || !fleet.route.is_empty()`, which is exactly the state of a fleet with a pending retreat when its own system is selected.

This contradicts the README rule "Wer das Feld nicht hält, weicht in ein Nachbarsystem aus". Only a human player benefits; nothing in the simulation issues such a command.

**Fehlerszenario.** The player's fleet loses in the enemy home system (or the player's home fleet loses to a blockader). The battle ends at tick T; the fleet has route=[neighbour], progress=0, regroup_until=day+30.

The player clicks the fleet name (which selects the fleet's system) and then "Marsch zum Ziel", sending SimCommand::MoveFleet { fleet, to: fleet.system }. order_move clears the route and returns Applied.

The fleet now lies in the contested system for 30 days. The victor in the same orbit cannot engage it. If it is the fleet's own system with a yard, repair_fleets restores 1 % hull per day, because repair has no blockade check. On day+30 it is ready and a new battle begins.

**Vorschlag.** Treat a pending retreat hop like a march already under way: in order_move, if `fleet.is_regrouping() && !fleet.route.is_empty() && fleet.progress <= 0.0`, keep route[0] as the first hop (the same handling as the in-transit branch) instead of clearing or replacing it. In split_fleet, either copy the parent's route when progress == 0 or reject the split while a retreat hop is pending. Alternatively, stop exempting a regrouping fleet from attack while it is still docked in the system of the battle it lost.

**Gegenprüfung.** The finding is correct, and I reproduced it in a live run with the existing headless tool. I would rate it medium rather than major.

**Mechanism (read in the code)**
- `conclude()` (`crates/sim/src/fleet/battles.rs:814-822`) models the forced retreat as an ordinary pending march: `regroup_until = Some(day + regroup_days)`, `route = vec![target]`, `progress = 0.0`.
- With `progress == 0.0`, `Fleet::is_docked()` is true and `in_battle` is already false, so `order_move()` takes its docked branch.
- That branch (`crates/sim/src/fleet/mod.rs:608-611`) is the old "halt" semantics: `if fleet.system == to { fleet.route.clear(); return Ok(()) }`. It predates this branch and has no check for a pending retreat.
- `SimSet::Commands` runs before `SimSet::Fleets`, so a command issued any time up to and including the next day-start tick is applied before the first movement step.
- Nothing restores the hop afterwards. `conclude()` is the only place that sets it, and `move_fleets` only follows `route`.
- The fleet keeps `regroup_until`, so `is_ready()` is false and `open_battles()` (`battles.rs:432-438`) never puts it into `present`. No battle can start against it until the period ends.

**Not handled elsewhere**
- The game UI enables "Marsch zum Ziel" in exactly this state (`crates/game/src/ui/fleets.rs:70`: `system != fleet.system || !fleet.route.is_empty()`); clicking the fleet name selects its own system.
- No test covers `to == fleet.system` for a beaten fleet. `cargo test -p sim` passes (exit 0), and `orders_reach_a_running_battle_and_a_fleet_can_break_off` even asserts that a beaten fleet with a pending hop accepts a new march order.

**Why it matters**
- README "Gefechte / Ende" and the `regroup_days` comment in `balance.ron` state that whoever does not hold the field moves to a neighbouring system and regroups there. The protection from attack is tied to that retreat. Here the fleet gets the protection without leaving.
- The same order gives a different result depending on a one-day timing difference: before the first movement step the fleet stays, after it the fleet goes out and comes back.
- Staying saves the round trip (time and fuel that cannot be replaced abroad or under blockade).
- In an own system with a yard, `repair_fleets` (`supply.rs:153-166`) keeps repairing, since it checks neither regrouping nor blockade.
- `advance_yards` (`yard.rs:302-313`) picks any own fleet with an empty route that is not in battle as host, so new ships join the protected fleet.

**Caveats on severity**
- Protection from attack while regrouping is intended, and a regrouping fleet can legally march out and back into the same system. The incremental gain is the saved round trip plus uninterrupted repair and yard pickup.
- The window closes at the first day-start after the battle ends (at most 20 ticks). A human must pause in time; commands queue while paused. A script or a future AI that re-issues march orders daily has no such limit.
- Nothing in the simulation issues such a command today. Money conservation, determinism and invariants are unaffected (the runs exit 0 with no invariant violation).

**The two side routes**
- Split: confirmed by reading. `ready_to_reorganize` passes because `is_docked()` is true at progress 0, and `new_fleet` creates the detached part with an empty route while `came_from` and `regroup_until` are copied (`mod.rs:717-738`). That part stays behind.
- Merge: the "longer regroup period wins" rule is deliberate and unit-tested (`the_regroup_period_survives_splitting_and_merging`). I would treat it as a design trade-off, not a separate defect. Merging a beaten fleet into a route-less own fleet does drop the hop as well.

The suggested fix (keep `route[0]` as the first hop while a retreat hop is pending) is small and compatible with the existing test, which only checks `route.last()`.

**Belege der Gegenprüfung.** **How I reproduced it**

The finding says no CLI path exists; there is one. `WarScript::issue` (`crates/sim/src/harness.rs:478-509`) re-issues `MoveFleet { fleet, to: <target home system> }` for all of A's fleets at each `--war` event. To make a lost battle end on the last day of a year, I used `--data-dir` with a scratch copy of `assets/data` in which only `fleets.speed` is changed (22.0 to 7.4067; `diff` shows that one line). The repository was not touched (`git status` clean).

**Main run**

`target/release/headless --data-dir <scratch>/data_s7.4067 run --seed 0 --years 2 --war 0:2:0 --war 1:2:0` (exit 0):
- `25.10.2400 Bei Doretha beginnt ein Gefecht: 1. Flotte (Räte) mit 6 Schiffen gegen 1. Flotte (Liga) mit 6 Schiffen.`
- `30.12.2400 Gefecht bei Doretha nach 66 Tagen entschieden: 1. Flotte (Räte) hält das Feld. ... 1. Flotte (Liga) verliert 1 Kreuzer, 1 Fregatte und 3 Korvetten und weicht nach Peltor aus.` This is day 359.
- `Abgelehnt (01.01.2401, ...): DeclareWar ...: es herrscht bereits Krieg` is the only rejected command, so the `MoveFleet` to Doretha at the start of day 360 was applied.
- `30.01.2401 Bei Doretha beginnt ein Gefecht: 1. Flotte (Räte) mit einem Schiff gegen 1. Flotte (Liga) mit einem Schiff.` This is day 389, exactly `regroup_until`. Both fleets lay in Doretha at war from day 360 to 388 with no battle.

**State just before the re-issued order**

Same data, `--years 1 --war 0:2:0 --save`, tick 7200:
- `1. Flotte (Liga)`: system Doretha (26), `route:[25]` (Peltor), `progress:0.0`, `in_battle:false`, `came_from:Some(25)`, `regroup_until:Some(389)`, stance Blockade.
- Its one remaining ship has fuel 0.0. The lane Peltor–Doretha is 71.95 long, which at dry speed (7.4067 × 0.4 per day) is 25 days each way.
- A round trip could not end before about day 409, so a battle at Doretha on day 389 means the fleet never left.

**Controls**

| Variant | First battle ends | Effect of the day-360 order | Second battle at Doretha |
|---|---|---|---|
| Speed 7.4067, with `--war 1:2:0` | 30.12.2400 (day 359) | applied while docked with pending hop; hop cleared | 30.01.2401 (day 389) |
| Speed 7.4067, without `--war 1:2:0` | 30.12.2400 (day 359) | no order | none; only one battle in two years |
| Speed 7.44, with `--war 1:2:0` | 29.12.2400 (day 358) | fleet already under way; first hop kept, goes out and back | 19.02.2401 (day 408) |
| Speed 7.38, with `--war 1:2:0` | 02.01.2401 (day 361) | `MoveFleet` rejected: `steht im Gefecht; erst den Rückzug befehlen` | none |

**Existing tests and build**
- `cargo test -p sim`: exit 0 (83 unit, 6 economy, 9 finance, 29 military tests pass).
- `the_inferior_fleet_falls_back_where_it_came_from_and_regroups` asserts the precondition (`route == vec![rear]`, regrouping, not in battle).
- `cargo build --release -p headless`: exit 0, binary up to date.

**Code references** (under `/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/`)
- `crates/sim/src/fleet/mod.rs:235-237` (`is_docked`), `578-618` (`order_move`, cancel at 608-611), `646-657`, `693-741` (split)
- `crates/sim/src/fleet/battles.rs:432-438`, `814-822`
- `crates/sim/src/fleet/supply.rs:153-166`
- `crates/sim/src/fleet/yard.rs:302-313`
- `crates/game/src/ui/fleets.rs:49-51`, `70-74`

Logs, the save and the tuned data copies are in `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/v_retreat/` (`runA.txt`, `runB.txt`, `run_s7.44.txt`, `run_s7.38.txt`, `save_y1.ron`, `test.log`, `build.log`).

### determinismus 2 · bestätigt · minor · `headless run --war/--peace` panics after the whole run when A is not an existing community

Ort: `crates/headless/src/war.rs:117`

**Beschreibung.** Reproduced.

parse_event() only checks A != B. WarScript::issue() queues the commands for any PolityId, and the simulation rejects them cleanly ("Gemeinschaft besteht nicht mehr") and logs them under that polity. print_aftermath() then formats every rejected entry with `sim.polities().get(entry.polity).name`, which indexes the list directly.

Because print_aftermath runs before print_summary and before `--save`, the run's summary and save file are lost. `hash` and `sweep` with the same arguments are unaffected, since they do not call print_aftermath.

The same happens for a free-state number that does not exist yet at the scripted year and never comes into being. If such a free state does appear later, the rejection is attributed to the wrong community instead.

**Fehlerszenario.** `target/release/headless run --seed 3 --years 4 --war 3:99:0 --quiet` simulates four years, then aborts with "index out of bounds: the len is 6 but the index is 99" in headless::war::print_aftermath, exit code 101, no summary.

`--war 3:0:99` (bad B) works and prints the rejection.

**Vorschlag.** In print_aftermath, look the name up with `sim.polities().list.get(entry.polity.index())` and fall back to the raw number. Optionally have WarScript::issue skip events whose A or B is not an existing community at the event day, with a German message.

**Gegenprüfung.** Confirmed by reading the code and by running the release binary; I could not refute it. Severity is minor (developer CLI, not the simulation core), but it is a real crash on plausible input, not a cosmetic issue.

Code path:
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/headless/src/war.rs:13-32 parse_event only checks A != B; nothing validates the numbers against the actual communities (main.rs does not either after loading the data).
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/sim/src/harness.rs:478-509 WarScript::issue queues DeclareWar/MakePeace for any PolityId via Headless::issue (CommandSource::Player, so the entry is logged).
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/sim/src/diplomacy/mod.rs:98-112 check_pair rejects it cleanly with "Gemeinschaft besteht nicht mehr" (uses list.get), so the sim core is fine.
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/headless/src/war.rs:117 print_aftermath formats every rejected entry with sim.polities().get(entry.polity).name, and /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/sim/src/polity/mod.rs:34-36 Polities::get is `&self.list[id.index()]` -> out-of-bounds panic.
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/headless/src/main.rs:155-170: print_aftermath runs before summarize/print_summary and before the --save write, so both are lost.

Why it matters rather than being a nitpick: the crashing path is exactly the one written to report rejected script commands (doc comments in harness.rs:459-461 and war.rs:109-110 say rejected commands are shown), and it works for a bad B but aborts for a bad A. The trigger is an ordinary operator mistake: six communities are numbered 0..5, so `6` (off by one) crashes, as does naming a free state by number in a run where it never secedes. The panic comes only after the whole simulation has run, and in a mixed script (one valid war plus one mistyped event) the battle reports, summary and save of the valid war are all lost. Nothing else handles it. No simulation state, determinism or money is affected; hash and sweep with the same arguments run normally.

One sub-claim is overstated and not verified by a run: "attributed to the wrong community" when a free state with that number appears later. From the code (finance/cohesion.rs:520 appends free states to polities.list) the printed name would be the community that now holds that id, which merely did not exist yet at the logged date; that part is cosmetic and does not carry the verdict.

The suggested fix is sound: Polities.list is public and PolityId::index() exists, so `sim.polities().list.get(entry.polity.index())` with a fallback to the raw number removes the panic.

Nothing from user memory was used (the memory index was empty).

**Belege der Gegenprüfung.** Release binary rebuilt first (cargo build --release -p headless, exit 0, already up to date with the sources). Logs are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim (verify_*.out / verify_*.err).

1) Claimed scenario: `target/release/headless run --seed 3 --years 4 --war 3:99:0 --quiet` -> exit 101, stdout empty, stderr:
   thread 'main' panicked at crates/sim/src/polity/mod.rs:35:19: index out of bounds: the len is 6 but the index is 99
   backtrace: core::panicking::panic_bounds_check <- headless::war::print_aftermath <- headless::main

2) Same with `--save <file>`: exit 101, stdout 0 bytes, save file not created.

3) Off by one: `--war 3:6:0 --quiet` -> exit 101, "the len is 6 but the index is 6". `--peace 3:6:0 --quiet` -> same panic.

4) Mixed script `--war 1:0:1 --war 3:6:0` (not quiet): chronicle is printed, then the same panic, exit 101; no aftermath, no summary.

5) Bad B works: `--war 3:0:99 --quiet` -> exit 0, prints "Abgelehnt (01.01.2403, Karawane von Meridian): DeclareWar { target: PolityId(99) }: Gemeinschaft besteht nicht mehr", then the aftermath line and the full summary.

6) Unaffected commands: `headless hash --seed 3 --days 1440 --war 3:99:0` -> exit 0, five checksums printed; `headless sweep --seeds 3..4 --years 4 --war 3:99:0` -> exit 0, table printed.

### determinismus 3 · widerlegt · minor · Battle seed and combatant lot depend on the global battle counter, so an unrelated battle elsewhere changes every later battle

Ort: `crates/sim/src/fleet/battles.rs:485`

**Beschreibung.** Found by code reading. Determinism itself is intact: same seed and same commands give the same run, and `hash` with and without `--exact` agreed in every war script I tried.

open_battles() derives the seed as `Rng::stream(seed, Domain::Battle, id.0, system.0)` with `id = battles.next_id`, the number of battles begun so far anywhere in the galaxy. The same stream also draws the combatant order (`rng.shuffle(&mut order)`). By the implementers' own numbers that order largely decides the winner: the first party won 55 of 76 decided campaign battles.

So any additional battle anywhere shifts the id of all later battles and re-rolls both their seed and their first-party lot. That includes a battle in a lower-numbered system on the same morning, since ids are handed out in BTreeMap system order. This runs against the stream rule stated in rng.rs: "Ein zusätzlicher Zufallszug in einem Teilsystem verschiebt so nie die Zahlen eines anderen."

**Fehlerszenario.** Two unrelated wars, A–B and C–D. In run 1 the decisive A–B battle at system Y on day 400 is battle id 0. In run 2, identical except that C and D meet in some system on day 399, the same A–B battle (same fleets, same ship states, same day, same system) gets id 1. It therefore gets a different seed and a different first-party lot, and typically a different winner.

The same coupling makes A/B comparisons in sweeps noisier: changing one war re-rolls all others.

**Vorschlag.** Key the stream on where and when instead of on the counter, e.g. `Rng::stream(seed, Domain::Battle, system.0 as u64, day)`. This is unique because at most one battle runs per system and battles begin only at day start. Keep BattleId purely as a label.

**Gegenprüfung.** The mechanism is described correctly and I could reproduce it, but it is not a defect under the project's rules; it is a design preference, and the suggested fix would not even remove the coupling.

What is true: open_battles() takes id = battles.next_id (battles.rs:485-486) and derives both the battle seed and the first-party lot from Rng::stream(seed, Domain::Battle, id, system) (battles.rs:487-498). With the headless tool I saw the same battle (same system, same start day, same fleets) end with the opposite winner once an unrelated battle elsewhere had started two or three days earlier and pushed it from B0 to B1.

Why it is not a real problem:
1. Determinism is intact, as the finding itself concedes. next_id is part of Battles, which is listed in sim_state! (state.rs:77), so it is saved and hashed. Repeated hash runs and hash --exact agree for a two-war script, and the tests a_scripted_war_is_deterministic and a_save_in_the_middle_of_a_battle_continues_identically pass. "Gleicher Seed, gleiche Befehle und gleiches Programm ergeben denselben Verlauf" (README) holds; the scenario in the finding compares two runs with different commands (an additional war), for which nothing is promised.
2. The cited rule is misread. rng.rs:6-7 says an extra draw in one Teilsystem never shifts the numbers "eines anderen" (of another subsystem, i.e. another Domain). Both battles are in Domain::Battle, and no other domain is affected. The doc of Rng::stream (rng.rs:59) names "ID und Tag" as the intended keys; the battle id is exactly such an ID, and ids.rs:73 (unchanged since the baseline d883061) defines BattleId as "Gefecht, fortlaufend über die ganze Partie".
3. The dependence on the running battle number is part of the tactical calculator's own contract, which is out of scope and "used as it is": step.rs:55 draws every step from Rng::stream(self.seed, Domain::Battle, self.id.0, self.tick) ("Strom je Gefecht und Takt", step.rs:5-6). So BattleId cannot be "purely a label". With the suggested fix (seed and lot from system and day) a battle that becomes B1 instead of B0 would still get different dice on every tick; only the lot would be stable.
4. The same convention already exists in accepted code: logistics keys raid dice on freighter.id (logistics/mod.rs:226), a running index assigned from freighters.len() (logistics/mod.rs:112, 555), so an extra freighter elsewhere shifts later freighters' dice in the same way.
5. The remaining argument (noisier A/B sweeps) is a tooling preference, not a requirement. Keys on (system, day) re-roll just as much whenever a change moves a fleet's arrival by one day, which almost any balance change does. Players cannot observe the difference; no money, goods, save, replay or public API is affected.

The finding also overstates the effect ("typically a different winner"): with two parties the lot changes in only about half of the cases.

So the observation is accurate but describes history sensitivity of a random draw, not wrong results, nondeterminism or an unmet requirement. If the team wants per-battle locality as an optional improvement, it has to be done together with the calculator's stream key in step.rs, not in battles.rs alone.

Memory: one stored note exists (about visually checking the game window in a cloud container); it does not bear on this task and was not used.

**Belege der Gegenprüfung.** Code read (all under /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim):
- crates/sim/src/fleet/battles.rs:485-498: `let id = BattleId(battles.next_id); battles.next_id += 1; let mut rng = Rng::stream(seed, Domain::Battle, id.0 as u64, system.0 as u64); let battle_seed = rng.next_u64(); ... rng.shuffle(&mut order);`
- crates/sim/src/battle/step.rs:55 (calculator, not under review, unchanged on this branch): `let mut rng = Rng::stream(self.seed, Domain::Battle, self.id.0 as u64, self.tick);` and this rng feeds fire_weapons and advance_shots.
- crates/sim/src/rng.rs:6-7 and :59: rule is about separation between Teilsysteme; stream keys "z. B. ID und Tag".
- crates/sim/src/ids.rs:73-74: "Gefecht, fortlaufend über die ganze Partie" (identical at d883061).
- crates/sim/src/logistics/mod.rs:112, 226, 555: raid stream keyed on freighter.id, which is `FreighterId::from_index(logistics.freighters.len())`.
- crates/sim/src/state.rs:77: `battles: Battles` inside sim_state!.
- Baseline d883061 fleet/mod.rs:245 keyed the old coarse rule on fleet list indices and day, also running indices.

Runs (logs in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_battleseed/):
- `target/release/headless run --seed 3 --years 14 --quiet --war 10:0:1`: B0 Zeloth 21.01.2410 to 18.02.2410 (28 days), Karawane holds the field (5 of 6 lost), Liga loses 6 of 6.
- Same plus one unrelated war `--war 10:X:Y`, twelve variants. In the eight variants where the other battle starts later (4:5, 5:4, 2:4, 4:2, 3:5, 5:3, 2:5, 5:2) Zeloth stays B0 with the identical result. In the four variants where the other battle starts on 18.01. or 19.01.2410 in another system (2:3, 3:2, 3:4, 4:3) Zeloth becomes B1, still starting 21.01.2410 with 6 against 6 ships, and ends reversed: 27 days, Karawane loses 6 of 6 and retreats, Liga holds the field with 4 of 6 lost. The B1 result is identical in all four variants, so the flip comes from the battle number alone. This reproduces the mechanism the finding describes.
- `target/release/headless hash --seed 3 --days 3700 --war 10:0:1 --war 10:2:3`, run twice and once with `--exact`: all checksums identical (Tag 3700: 9dc54adfe8013acc), exit code 0 each.
- `cargo test -p sim --test military -- a_scripted_war_is_deterministic a_save_in_the_middle_of_a_battle_continues_identically three_communities_fight_in_one_system`: 3 passed, exit code 0.
- `cargo build --release -p headless`: exit code 0. No file in the repository was changed.

## Blickwinkel: korrektheit

### korrektheit 1 · bestätigt · major · Undecided battles restart forever: the campaign has no way out of a standoff, and with real designs standoffs are common

Ort: `crates/sim/src/fleet/battles.rs:807`

**Beschreibung.** At the time limit conclude() lets every fleet stay (`held || battle.timed_out`, line 807), sets a pause (lines 879-893), and open_battles() opens the next battle between the same ready fleets as soon as the pause is over (lines 472-474). Nothing changes between rounds, so a pairing that the tactical calculator cannot decide is fought again and again: 450 days of battle, 20 days of pause, indefinitely. The implementers' note that a time-out 'never occurred with the real data' only holds for the default starting fleet (I counted 180 of 180 battles decided over 60 seeds with unmodified data). With other compositions of the real, tech-free designs it is the normal outcome: corvettes only and carrier groups without cruiser/battleship. While the loop runs both fleets are `in_battle` about 96 % of the time: MoveFleet is rejected, no resupply or repair, the attacker never blockades, the home yard keeps finished ships in the dock (host in battle + contested), and needs_fine_ticks stays true. No invariant is violated, money_drift stays 0.

**Fehlerszenario.** Copy assets/data, change only `military.starting_fleet` to `[("dolch", 6)]`, then `headless --data-dir <copy> run --seed 1 --years 14 --quiet --war 1:2:3`: at Lumorara battles B0..B9 each end after 451 days 'ohne Entscheidung abgebrochen', from B1 on with 0 losses on both sides (one corvette each), B9 still running at the end. Same lever, 8 seeds x 3 wars, 3 years: `[("dolch", 1)]` 24 of 24 battles undecided; `[("dolch", 3)]` 23 undecided, 11 'Niemand hält das Feld', 1 decided; `[("horst", 2), ("dolch", 2)]` every battle undecided (20-year run: 15 consecutive 451-day battles at Okhendor, the home side re-buying 11-30 fighters in each pause and losing them again); `[("bastion", 1), ("horst", 1)]` 2 of 24. In a normal game the same happens as soon as a player or the coming military AI builds corvette squadrons or carriers, or when two remnants are single corvettes.

**Vorschlag.** Give the time-out a strategic result instead of repeating it: e.g. the side that does not own the system (or the later arrival, or the side with the lower remaining fighting value) withdraws and regroups like a beaten fleet; or do not reopen a battle between an unchanged set of fleets until something changed (new fleet, new orders, resupply). Add a campaign test with corvette-only and carrier-only fleets.

**Gegenprüfung.** I could not refute this. The failure reproduces exactly as claimed with existing tools, and the cause is in the reviewed campaign code, not only in the tactical calculator.

**Mechanism (confirmed by reading):**
- `conclude()` keeps every fleet in the system at a time-out (`held || battle.timed_out`, battles.rs:807-813) and only sets a `BattlePause` (879-893).
- `open_battles()` drops expired pauses (line 410) and skips a system only while it is paused (472-474). Otherwise it opens a new battle between the same ready fleets.
- Nothing in between changes the pairing. The campaign test even asserts the restart ("Danach beginnt es von vorn", tests/military.rs, around line 3156).
- So any pairing the calculator cannot decide repeats: 450 days of battle (time_limit 900 s at 2 s per day), 20 days of pause, with no end condition in the rules.

**Why it matters:**
- **Not an edge case.** With the real, tech-free designs the calculator times out 8/8 for corvette-only and carrier-only pairings (the calculator is given, so the campaign has to cope with this).
- **The war never produces a result.** In the reproduction an attacker's single corvette with an empty tank holds the enemy capital system for 13 years. The attacker never blockades and the defender never clears its home.
- **Both fleets are pinned about 96 % of the time.** `MoveFleet` is rejected in battle, resupply and repair skip fleets in battle, and the home yard holds finished ships in the dock while its host fleet fights.
- **The intended attrition does not break it.** In the carrier case the defender re-buys fighters in every pause and loses them again, round after round.

This runs against "supply with ammunition and fuel matters" and the blockade requirement, and it is a livelock at campaign level.

**Corrections to the finding (they lower severity, not validity):**
- "No way out" is true for the rules and for passive parties, not for an active commander. `SimCommand::Retreat` works during the battle, `MoveFleet` works in the 20-day pause, and reinforcements join the next morning. Today every AI polity is passive, and the game UI has no retreat button.
- With the unmodified starting fleet I could not trigger it: 0 of 179 battles undecided over 40 seeds with multi-phase scripted wars, consistent with the reviewer's 180/180. Against the default fleet every build I tried is decided. It needs corvette-only or carrier-only forces on both sides: a changed `starting_fleet` lever, `BuildShip` on both sides, or corvette remnants.
- The fine-tick cost is negligible (same runtime with and without `--exact`).

Severity is arguably medium rather than major, but the problem is real and the suggested fix is sound.

The one memory note listed (cloud render check) does not bear on this task and was not used.

**Belege der Gegenprüfung.** All runs used the existing release binary (`cargo build --release -p headless` exit 0, already up to date) and data copies in the scratch directory; the repository was not touched.

**1. Exact claimed scenario** — copy of `assets/data` with only `military.starting_fleet: [("dolch", 6)]`, then `headless --data-dir <copy> run --seed 1 --years 14 --quiet --war 1:2:3`:
- "Gefechte: 10 begonnen, 1 laufen noch, 10 Schiffe verloren."
- B0 at Lumorara, 23.01.2401 to 23.04.2402 (451 days), "ohne Entscheidung", 5 of 6 corvettes lost on each side.
- B1 to B8: each 451 days, "ohne Entscheidung", "0 von 1 Schiffen verloren" on both sides, each starting 20 days after the previous one ended (e.g. 23.04.2402 to 13.05.2402).
- B9 "läuft seit 428 Tagen" at the end.
- Save at the end (`--save`): attacker's corvette has ammo 0.667 and fuel 0.0, defender's has ammo 1.0 and fuel 1.0, both `in_battle: true` in system 34.

**2. Carrier group** — `[("horst", 2), ("dolch", 2)]`, seed 1, 20 years, `--war 1:2:3`:
- 15 battles begun, 14 concluded, all 451 days "ohne Entscheidung".
- From B2 on the attacker loses 0 of 2 ships; the home side loses 4 to 26 fighters per round and re-buys them in each pause.

**3. Sweep, 8 seeds × 3 wars × 3 years** (`--war 1:0:1 --war 1:2:3 --war 1:4:5`):

| starting_fleet | undecided | decided | nobody holds | still running |
|---|---|---|---|---|
| dolch:1 | 24 | 0 | 0 | 24 |
| dolch:3 | 23 | 1 | 11 | 12 |
| horst:2, dolch:2 | 24 | 0 | 0 | 24 |
| bastion:1, horst:1 | 2 | 22 | 0 | 2 |

These are the reviewer's numbers.

**4. Tactical tool** (`headless battle --a .. --b .. --seeds 0..8`): "Zeit um" at 901 s in 8/8 for dolch 1v1, 2v1, 3v1, 3v3, 6v6, horst 1v1, horst 2v2, and horst:2,dolch:2 mirrored. The default fleet mirrored is decided 8/8 in about 57 s.

**5. Unmodified data** — 40 seeds × 9 years with 13 scripted wars in three phases: 179 battles, 0 "ohne Entscheidung", including 16 one-ship-against-one-ship remnant battles.

**6. Other checks:**
- `headless hash --seed 1 --days 2160 --war 1:2:3` with the dolch:6 data gives identical output with and without `--exact`, each in about 0.52 s.
- `cargo test -p sim --test military a_battle_without_decision` passes (exit 0); that test asserts the restart after the pause.

**Code references** in `/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim`:
- `crates/sim/src/fleet/battles.rs`: 410, 446-474, 807-813, 879-893, 979-999 (`order_retreat`)
- `crates/sim/src/fleet/mod.rs`: 589-594 (`MoveFleet` rejected in battle)
- `crates/sim/src/fleet/supply.rs`: 54-55 and 153-154 (no resupply or repair in battle)
- `crates/sim/src/fleet/yard.rs`: 303-320 (finished ship stays in dock)
- `crates/sim/src/fleet/movement.rs`: 85-96 (blockade rule)
- `crates/sim/tests/military.rs`: 3089-3162
- `crates/game/src/ui/fleets.rs`: only `SetStance` and `MoveFleet` are issued

**Logs:** `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/vstandoff/` — `run_d6.txt`, `run_h2d2.txt`, `save_d6.ron`, `sw_*.txt`, `def/a_*.txt`, `hash_d6.txt`, `hash_d6_exact.txt`, `test_timeout.log`.

### korrektheit 2 · bestätigt · minor · A fleet is released only when the whole battle ends, even if it has no enemy left or all its ships have left the field

Ort: `crates/sim/src/fleet/battles.rs:541`

**Beschreibung.** advance_battles() concludes a battle only as a whole (lines 541-553); there is no release of a single combatant. A fleet stays `in_battle` (no MoveFleet, resupply, repair, merge, split, ship delivery, no blockade/escort) as long as any other pair in the same battle still fights. This also applies after peace: open_battles() only switches hostility off (lines 422-430). Retreat does not help either: the ships reach the edge, but the fleet is still bound until conclude(), and then, not having held the field, it is sent to a neighbouring system and must regroup for 30 days.

**Fehlerszenario.** A and C are at war with B but not with each other; all three fleets fight in B's home system. On day 5 A makes peace with B. The battle C-B goes on (in a standoff up to the 450-day limit). A's fleet has no enemy, but MoveFleet is rejected with '... steht im Gefecht; erst den Rückzug befehlen'. After Retreat its ships are Escaped, the fleet remains in_battle until C-B ends, then 'weicht nach ... aus' and regroups 30 days. Same for a routed first fleet A1 while reinforcement A2 fights on for weeks.

**Vorschlag.** At day start release combatants that have no hostile side left on the field or no ship in play: write their result back immediately (keep the combatant entry for the report) and clear `battle`/`in_battle`.

**Gegenprüfung.** I could not refute it. The finding is accurate and is a real functional gap of minor severity; only its 450-day figure is a theoretical upper bound.

**Mechanism (code reading)**
- A surviving fleet's `battle`/`in_battle` is cleared in exactly one place: `conclude()` (crates/sim/src/fleet/battles.rs:799-800).
- `advance_battles()` calls `conclude()` only for battles whose `finished` flag is set (battles.rs:538-553).
- The calculator sets `finished` only when no hostile pair is left with one side fighting and the other present, or at the time limit (crates/sim/src/battle/step.rs:1098-1122).
- `open_battles()` only rewrites the hostility matrix after peace (battles.rs:422-430).
- `Fleets::refresh` clears the flags only for fleets without ships (fleet/mod.rs:433-449).
- So a fleet with no enemy left, or with every ship off the field, stays bound while any other pair fights.

**Consequences while bound (all confirmed in code)**
- MoveFleet is rejected (fleet/mod.rs:589-594) and the fleet does not march (movement.rs:43).
- No resupply or repair (supply.rs:54-55, 153-154), no merge or split (mod.rs:646-657), no ship delivery from the yard (yard.rs:303-313).
- No blockade, raid or escort, because `is_ready()` is false (mod.rs:246-248, 464; movement.rs:88-92).

**Retreat branch (code reading only; the war script cannot issue Retreat)**
- `order_retreat` turns Active ships into Routing; they become Escaped at the exit radius, and only Active carriers launch fighters.
- At `conclude()` the side therefore does not hold the field. Unless the battle timed out, the fleet is routed to a neighbouring system with `regroup_until = day + 30` (battles.rs:807-831; `regroup_days: 30`).
- So following the rejection message's own advice ("erst den Rückzug befehlen") is strictly worse than waiting.
- The routed-first-fleet variant (A1 bound while A2 fights on) follows from the same single release point; I did not run it.

**Reproduction with the existing headless tool**
- With real battle balance, a fleet that made peace on 01.01.2402 stayed a combatant for 36 more days in one run and 19 in another.
- In the two-party control the battle was called off on the day of the peace.
- With the battle clock slowed, the same fleet's MoveFleet was rejected 360 and 720 days after its peace; the two-party control accepted the order.
- Both setups used scratch copies of the data through `--data-dir`; the repository is untouched.

**Why it matters**
- It contradicts README.md:215-216 and the commit message of 4b64d65 ("Ein Friedensschluss beendet das Gefecht").
- It contradicts the Retreat command's documentation (command.rs:112-113) and the test comment "kein Marschbefehl, bis das Feld geräumt ist" (tests/military.rs:2681).
- Two-party battles release the fleet at once in the same situation, so the behaviour is inconsistent.
- The scenario needs only SimCommands (MakePeace is unilateral and immediate), and multi-party battles are common: in 26 of 40 scripted seeds a reinforcement joined a running battle.
- No existing test covers peace or retreat by one of three parties; the three nearest tests pass.

**Limits of the claim**
- No money, goods or determinism impact.
- The 450-day standoff did not occur: 0 of 102 scripted battles hit the time limit (durations 10 to 58 days, median 31), and beam weapons need no ammunition. The realistic lock is days to weeks.

**Note for a fix**
- `Battles::find_anomaly` (battles.rs:207-220) and the `attached` branch in `conclude()` (battles.rs:769-782) assume every living combatant fleet still points at the battle. A per-combatant release has to adjust both.

**Belege der Gegenprüfung.** All runs use target/release/headless (rebuilt, exit 0). Logs are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/.

**1. Real battle balance, only `fleets.speed` lowered to 2.20 in a scratch data copy so the year-granular script hits the battle**
Command: `headless --data-dir <copy> run --seed 21 --years 3 --war 1:0:1 --war 1:2:1 --peace 2:2:1` (log: verify_sp_2.20.log)
- 09.12.2401 Bei Nyanax beginnt ein Gefecht: 1. Flotte (Räte) ... gegen 1. Flotte (Karawane) ...
- 18.12.2401 1. Flotte (Syndikat) greift mit 6 Schiffen in das Gefecht bei Nyanax ein.
- 01.01.2402 Karawane von Meridian und Aschgraue Räte schließen Frieden.
- 06.02.2402 Gefecht bei Nyanax nach 58 Tagen entschieden: 1. Flotte (Karawane) und 1. Flotte (Syndikat) halten das Feld. ...

Karawane stayed a combatant 36 days after its peace. With speed 2.15 (verify_sp_2.15.log) the same pattern gives 19 days (peace 01.01.2402, end 20.01.2402).

Two-party control, speed 2.10 (verify_sp_2.10.log):
- 01.01.2402 peace
- 01.01.2402 Gefecht bei Nyanax nach 8 Tagen eingestellt: Zwischen den Beteiligten herrscht kein Krieg mehr.

**2. Battle clock slowed (`battle.dt` 0.1 → 0.002 in a scratch copy) so later script commands fall inside the battle**
Command: `headless --data-dir <copy> run --seed 4 --years 8 --war 1:0:1 --war 1:2:1 --peace 2:0:1 --war 3:0:3 --war 4:0:3 --peace 4:0:3` (log: verify_dt_three_long.log)
- 22.01.2401 Bei Rhamar beginnt ein Gefecht: 1. Flotte (Liga) ... gegen 1. Flotte (Kommune) ...
- 25.01.2401 1. Flotte (Orden) greift ... ein.
- 01.01.2402 Thalassische Liga und Sonnwend-Kommune schließen Frieden.
- Abgelehnt (01.01.2403, Thalassische Liga): MoveFleet { fleet: FleetId(0), to: SystemId(19) }: 1. Flotte (Liga) steht im Gefecht; erst den Rückzug befehlen
- Abgelehnt (01.01.2404, Thalassische Liga): MoveFleet { ... }: 1. Flotte (Liga) steht im Gefecht; erst den Rückzug befehlen
- 16.04.2405 Gefecht bei Rhamar nach 1525 Tagen entschieden: 1. Flotte (Liga) und 1. Flotte (Orden) halten das Feld. 1. Flotte (Liga) bleibt ohne Verluste. ...

Two-party control without `--war 1:2:1` (verify_dt_two.log):
- 11.01.2402 Gefecht bei Rhamar nach 350 Tagen eingestellt: Zwischen den Beteiligten herrscht kein Krieg mehr.
- The MoveFleet of 01.01.2403 is accepted; 16.03.2403 a new battle begins at Soluntis.

**3. Unmodified data, 40 seeds**
Command: `run --seed 0..39 --years 5/6` with several `--war` events (logs: verify_run_s*.log)
- 102 battles ended, 0 "ohne Entscheidung".
- Durations 10 / 31 / 58 days (min / median / max).
- In 26 of 40 seeds a reinforcement joined a running battle.

**4. Existing tests**
Command: `cargo test -p sim --test military -- peace_ends_a_running_battle three_communities_fight_in_one_system orders_reach_a_running_battle` (log: verify_tests.log)
- 3 passed. None covers peace or retreat by one of three parties.

**5. Code references**
- crates/sim/src/fleet/battles.rs:422-430, 538-553, 799-800, 807-831
- crates/sim/src/battle/step.rs:1098-1122
- crates/sim/src/fleet/mod.rs:246-248, 433-449, 589-594, 646-657
- crates/sim/src/fleet/movement.rs:43, 88-92
- crates/sim/src/fleet/supply.rs:54-55, 153-154
- crates/sim/src/fleet/yard.rs:303-313
- crates/sim/src/command.rs:112-113
- README.md:214-216

`git status` in the worktree is clean. Nothing from memory was used; the memory index was empty.

### korrektheit 3 · bestätigt · minor · 'Holds the field' is decided per fleet and by any active craft: drifting ships are lost and survivors must withdraw although the own community holds the field

Ort: `crates/sim/src/fleet/battles.rs:605`

**Beschreibung.** write_back() uses `held = battle.is_fighting(side)` for exactly one fleet (line 605) and drops Disabled ships when that fleet has no Active ship (lines 606-611); conclude() sends the fleet away on the same per-fleet value (line 807). The calculator itself keeps such ships, because it abandons Disabled ships only when a hostile side still fights. The owner-level value exists (OwnerTally.held) but is used only for the price shock. In addition is_fighting() counts launched fighters, so a side can 'hold' with nothing but craft in the air.

**Fehlerszenario.** (a) First fleet A1 and reinforcement A2 of the same community beat B. A1 has no Active ship left, one cruiser drifts (reactor destroyed without breach), two ships escaped. The calculator leaves the cruiser Disabled (no hostile victor); write_back books it as lost ('unbooked'), and A1 'weicht aus' and regroups 30 days although A2 holds the field in the same system. (b) A lone carrier is destroyed after launching, one fighter stays Active, the enemy corvette routs: the report says the carrier fleet 'hält das Feld' and 'besteht nicht mehr' (the fighter has no home), the routed enemy must withdraw and takes the heavy-defeat shock, the annihilated owner gets none because owner.held is true.

**Vorschlag.** Use the owner (or the set of mutually non-hostile sides) for recovering drifting ships and for the decision to withdraw, and count only ships with `source.is_some()` when asking whether a fleet holds the field.

**Gegenprüfung.** Verdict: real, but minor, and narrower than written. I could not refute the mechanics; I could refute parts of the two example scenarios.

**Confirmed by reading**
- `write_back` decides whether a drifting ship survives only on `battle.is_fighting(side)` of that one fleet. `conclude` sends the fleet to a neighbour system and sets `regroup_until` on the same per-fleet value.
- Neither looks at other fleets of the same owner. `OwnerTally.held` is written once and read only for the share-price shock.
- The calculator keeps exactly these ships: `check_end` abandons Disabled ships only when a hostile side still fights and the battle did not time out. Fleets of one owner are never hostile to each other (`at_war` requires `a != b`).
- So when A1 and A2 beat B, a drifting ship of A1 stays Disabled in the calculator and is then dropped by `write_back` as `unbooked`. It counts toward lost value, debris, `ships_lost` and war weariness while A2 holds the field in the same system.
- `is_fighting` counts any Active ship, launched craft included. If every ship of a fleet is destroyed but a fighter is still Active, `conclude` reports `held_field = true` and "hält das Feld", the fate reads "besteht nicht mehr", and the wiped-out owner is exempt from the defeat shock.
- Nothing elsewhere corrects this, and no test covers a friendly co-side or a craft-only holder.

**Checked with the headless tool**
- A craft-only holder is reachable: `horst:1` against `dolch:8` ends in 12 of 108 runs with the carrier side as sole holder after losing the carrier.
- A fleet is sent away while a non-hostile partner holds the field: seen in a real run (seed 10) and in 5 of 113 three-party battles over 200 seeds. That is the cross-owner case; for one owner the code path is identical. I did not reproduce the same-owner case at campaign level, because the tool never gives a community two fleets.

**Corrections to the finding**
- **Drifting ships:** with the current calculator and data they practically never exist at battle end: 0 in about 2,100 practice battles. The "drifting cruiser lost" half of (a) is a latent logic error today, not something players will see.
- **Withdrawal:** per-fleet withdrawal is documented in the README ("jede Flotte eine Partei … Wer das Feld nicht hält, weicht … aus"). The suggested owner-level rule would cancel an ordered `Retreat` whenever a sister fleet wins. Treat that part as a design decision, not a bug.
- **Scenario (b) as written does not happen:** a lone corvette never kills the carrier (it is destroyed in 23 s, 6 of 6 seeds). A routed side with no losses gets neither weariness nor shock, because the loop skips owners with `lost <= 0`. The mechanism needs a swarm, and in the runs I saw both fleets were wiped out.

**Related, same root cause:** at the time limit nobody leaves and the calculator abandons nothing, yet `write_back` still drops the drifting ships of a fleet without Active ships.

Determinism, money conservation and the public API are not affected. Nothing in this report came from memory.

**Belege der Gegenprüfung.** **Code** (worktree `/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim`)
- `crates/sim/src/fleet/battles.rs:605-611`: `let held = battle.is_fighting(side);` and `ShipStatus::Disabled => held` (comment at 608: "Ein treibendes Schiff birgt nur, wer das Feld hält.").
- `battles.rs:639-645`: a non-surviving ship that is not Destroyed goes into `unbooked`.
- `battles.rs:751`, `768`, `807-831`: per-side `held`; `owner.held |= held`; `else if held || battle.timed_out { stay } else { retreat_target, regroup_until = day + regroup_days, route = [target] }`.
- `battles.rs:840-848`: owners with `lost <= 0.0` are skipped; shock only if `!owner.held && share >= heavy_defeat_share`.
- `battles.rs:961`: `OwnerTally.held` is documented as "Mindestens eine ihrer Flotten hielt das Feld."
- `crates/sim/src/battle/step.rs:1123-1147`: "Treibende Schiffe bleiben beim Sieger erhalten; die eines Verlierers gehen verloren"; `ShipStatus::Disabled if hostile_victor[side] && !self.timed_out => abandoned`.
- `crates/sim/src/battle/mod.rs:599-603`: `is_fighting` is true for any Active ship of the side, with no class or `source` filter.
- `crates/sim/src/diplomacy/mod.rs:25-27`: `at_war` is `a != b && …`.
- `README.md:207-225` documents the per-party rules; `crates/sim/src/command.rs:112-116` documents `Retreat` ("was entkommt, weicht in ein Nachbarsystem aus").

**Runs** (existing binary `target/release/headless`, built clean; repository untouched, `git status` empty)
- `cargo test -p sim --lib fleet::battles`: 8 passed. `survivors_keep_what_the_battle_left_of_them_and_the_lost_are_gone` only tests a hostile holder.
- `headless battle --a horst:1 --b dolch:8`, 9 doctrine combinations × seeds 0..12: 12 of 108 runs like "A siegt nach 46 s  A verliert 85 % (5 Jg 1 Tr)  B verliert 100 % (8 Kv)".
- `headless battle --a horst:1 --b dolch:1 --seeds 0..6`: "A siegt nach 23 s, A verliert 0 %" in 6 of 6.
- `headless run --seed 10 --years 6 --war 1:0:1 --war 1:2:1 --war 1:3:1`, chronicle 09.03.2401: "1. Flotte (Syndikat) und 1. Flotte (Konkordat) halten das Feld. 1. Flotte (Orden) verliert 2 Fregatten und 3 Korvetten und weicht nach Rhaen aus. …" followed by "Die Niederlage bei Ithoth erschüttert das Vertrauen in Orden der Tiefen Uhr". Orden is an attacker allied with the holders.
- `headless run --seed 0..199 --years 3 --war 1:0:1 --war 1:2:1`: 113 three-party battles, 5 with one attacker holding and the other leaving the field.
- `headless battle … --trace` over about 2,100 practice battles (mirror and asymmetric fleets of dolch, waechter, speer, bastion, horst): initial − active − lost − escaped was 0 for every side checked, so no ship was drifting at the end.
- Reason drifting is so rare: reactor hp 50, module damage is 0.6 × hull damage, and each reactor kill has a 50 % breach (`assets/data/modules.ron:21`, `balance.ron:293-295`).

**Logs and scan scripts** are in `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_holdfield/`; the main ones:
- `carrier_pairs.log`
- `coal_10.log`
- `two/coal2_*.log`
- `scan.py`
- `disabled_scan.py`
- `disabled_any.py`
- `coalition_stats.py`

### korrektheit 4 · bestätigt · minor · The retreat after a lost battle is optional and the regroup protection can be moved onto an intact fleet

Ort: `crates/sim/src/fleet/mod.rs:608`

**Beschreibung.** conclude() gives a beaten fleet `route = [target]`, `progress = 0` and `regroup_until`. order_move() treats that fleet as docked and clears the route when the target is the current system (lines 608-611). open_battles() only considers `is_ready()` fleets, so the fleet then stays in the battle system and can neither be attacked nor join a battle for `regroup_days`; at home with a yard it is repaired meanwhile (repair_fleets has no enemy/blockade check). merge_fleets() copies the later regroup date to the target (line 686), so merging a regrouping remnant into an intact fleet makes that fleet unattackable for up to 30 days at a moment of the owner's choosing; split_fleet() does not copy the retreat route, so the detached part stays as well. This contradicts the rule in balance.ron and README ('weicht in ein Nachbarsystem aus').

**Fehlerszenario.** Defender D loses in its home system and gets route [neighbour], regroup 30 days. Next tick D issues MoveFleet{fleet, to: home}: accepted, route cleared. The victor's fleet stands in the same system for 30 days and cannot engage; D's ships gain 30 x repair_per_day hull in the dock, then a new battle starts. Variant: an intact fleet F and a regrouping one-ship remnant R lie in the same system; an enemy doom fleet arrives; MergeFleets{from: R, into: F} is accepted and F cannot be attacked until R's regroup date.

**Vorschlag.** While a retreat is pending reject march orders that keep the fleet in the battle system (or make a regrouping fleet that remains next to ready enemies attackable again after a short grace period); for merges into a ready fleet in a system with hostile ready fleets either reject the merge or do not transfer the regroup date.

**Gegenprüfung.** I could not refute it; every factual statement of the finding holds in the code and reproduces through the public SimCommand path. Severity "minor" is apt.

Code reading (worktree /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim):
- crates/sim/src/fleet/battles.rs:815-822: a side that does not hold the field gets regroup_until = day + regroup_days, route = [target], progress = 0 (comment: "räumt das System und sammelt sich").
- crates/sim/src/fleet/mod.rs:595-611: order_move has no regroup check. With progress 0 the fleet counts as docked (is_docked, line 235), so `fleet.system == to` clears the route. This branch predates the retreat feature (identical at d883061) and was not adapted.
- battles.rs:434-437: open_battles only collects `is_docked() && is_ready()` fleets, so a regrouping fleet neither starts nor joins a battle wherever it stands.
- crates/sim/src/fleet/supply.rs:153-158: repair_fleets checks only docked, not in battle, damaged, own system, yard. No regroup, enemy or blockade check.
- mod.rs:681-686: merge copies max(regroup_until) onto the target; mod.rs:717-738: split copies regroup_until but the new fleet has an empty route (new_fleet, line 417).
- Nothing else restricts this: regroup_until / is_regrouping / is_ready are used only for expiry, raid/escort and blockade.
- It contradicts README.md:221-225 and assets/data/balance.ron:256-258 ("weicht in ein Nachbarsystem aus und sammelt sich dort").

Reproduction: an out-of-tree probe crate against the sim harness (no repository file touched, git status clean afterwards), seed 24, defender = polity 0 beaten in its home system Lumaos. Results are in the evidence field.

What weakens it, but does not refute it:
- Regrouping fleets taking march orders and being immune everywhere is deliberate and tested (crates/sim/tests/military.rs:2706-2708 and 2107-2128). The max-merge rule is unit-tested (mod.rs:1077-1085), intended as a penalty.
- A legal leave-and-return order gives almost the same result as the stay order: back in the home dock on day 66, hull 0.743 instead of 0.790. The marginal gain of "stay" is only the round trip (8 of 30 days here). The underlying weakness is that the protection is not tied to having left, so the finding's second fix alternative (attackable again next to ready enemies) is the one that closes it; merely rejecting the stay order would not.
- Two wording points in the scenario: the merge must be issued before the enemy docks, because the battle opens on the arrival tick; and the repair gain needs hull segments on the home market (0 early in seed 24; 5.6-12.4 on capital markets after ten years per `headless fleets --seed 3 --years 10`).

It still matters: the chronicle reports "weicht nach Xantor aus" while the fleet stays, the natural halt order triggers it, the beaten fleet is repaired under protection next to a victor that can neither attack nor repair, and an intact fleet can be made unattackable with a remnant whose timer came from a loss-free early Retreat.

No memory notes existed for this session, so nothing here comes from memory.

**Belege der Gegenprüfung.** Probe source and logs: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/probe_regroup/ (src/main.rs, stay_24.log, stay_24_hull10.log, stay_24_blockade_hull10.log, merge_24.log, split_24.log).

1) Stay order (stay_24_hull10.log). Battle in Lumaos ends on day 57; the defender keeps 3 ships, route ["Xantor"], progress 0, regroup until day 87. `MoveFleet{fleet, to: Lumaos}` on the next tick -> Applied, route [], regroup_until still Some(87).
- The defender then lies 29 days in its home system beside the victor (6 ships, strength 119.1) with no battle; battle 2 begins on day 88.
- With 10 hull segments on the home market: defender hull 0.536 -> 0.790, strength 35.8 -> 49.5. Victor unchanged at hull 0.781, ammo 0.90.
- Same repair with the victor in blockade stance and the system blockaded (stay_24_blockade_hull10.log).
- Control without the order: the fleet marches to Xantor, hull stays 0.536.
- Control leave-and-return (order after one march day, route ["Xantor","Lumaos"]): back on day 66, 22 protected days at home, hull 0.743.
- With natural stocks (stay_24.log) the market holds 0 hull segments, so there is no repair, but the 29 protected days and the day-88 battle are the same.

2) Merge (merge_24.log). Remnant R (1 corvette) orders Retreat at once, escapes undamaged, returns home with regroup until day 43. On day 18, with the enemy en route, `MergeFleets{from: R, into: F}` -> Applied; F (intact, 6 ships) now has 7 ships, regroup_until Some(43), is_ready false.
- The enemy (10 ships, strength 332.5 against 112.1) docks on day 22; the battle starts only on day 44.
- Control without the merge: the battle starts on arrival, day 21.

3) Split (split_24.log). `SplitFleet` before the first march day -> Applied; the detached part (2 ships) has route [] and regroup Some(87). Ten days later it is still docked in Lumaos beside the victor, no new battle; the rest is in Xantor.

money_drift stayed 0 and check_invariants passed on every day of the stay runs.

Relevant lines: crates/sim/src/fleet/mod.rs:595-611 (order_move), 681-686 (merge), 717-738 (split), 1077-1085 (unit test fixing the max-merge rule); crates/sim/src/fleet/battles.rs:434-437, 815-822; crates/sim/src/fleet/supply.rs:153-158; README.md:221-225; assets/data/balance.ron:256-258.

### korrektheit 5 · bestätigt · minor · Build orders hang silently when the world loses its yard

Ort: `crates/sim/src/fleet/yard.rs:198`

**Beschreibung.** yard_slots() returns 0 as soon as a non-capital world has no arms capital left. advance_yards() then takes the branch `*used >= slots` (lines 196-202): the order neither advances nor lapses, `stalled` is set to false, there is no chronicle entry, and it keeps one of the `yard_queue` places. order_ship() checks for a yard only when the order is placed. `headless fleets` skips worlds with 0 slots (crates/headless/src/fleets.rs line 152), so such orders are not even listed, while PolitySummary.orders still counts them.

**Fehlerszenario.** A colony with arms capital 0.1 (typical: `headless fleets` shows 0.1-0.4 on most colony yards) accepts a cruiser order (200 build days). Its arms plants run at a loss (e.g. inputs missing under blockade): 4 % depreciation + 25 % scrap per year, and capital below 0.05 is set to 0 (economy/capital.rs line 86). After roughly two years yard_slots is 0; the order stays at e.g. 40 % with the parts and yard fee already paid, shown as 'wartet auf einen Bauplatz' forever unless arms capital returns or the owner cancels without refund.

**Vorschlag.** Treat 'no yard any more' explicitly: mark the order as stalled and write a chronicle entry (or let it lapse like on a change of owner), or keep one slot as long as an order with progress > 0 exists; list such orders in `headless fleets`.

**Gegenprüfung.** I tried to refute it and could not. The finding is real, with minor severity.

**The hang follows directly from the code.**
- `yard_slots()` returns 0 for a non-capital world whose arms capital is 0 (yard.rs:71-73).
- In `advance_yards()` the slot gate is `if *used >= slots` (yard.rs:196-202). With `slots == 0` this is `0 >= 0` on every day, so the order gets `stalled = false` and `continue`.
- That `continue` comes before the demand registration and before the completion check (yard.rs:261-263). So the order registers no parts demand, and a ship that is already 100 % built and waiting in dock is never delivered either.
- Orders are only ever removed by `cancel_order`, by the owner-change lapse (yard.rs:192-195) and by completion. `order_ship` checks for a yard only at placement (yard.rs:124).
- Nothing else catches the state: `Fleets::find_anomaly` (fleet/mod.rs:526-539) and `check_invariants` do not look at it.
- The flag is misleading: an order that was `stalled = true` for missing parts flips to `false` at the moment it becomes permanently stuck.
- The tool claims are correct: `headless fleets` skips worlds with 0 slots together with their orders (fleets.rs:151-154), while `PolitySummary.orders` counts them (harness.rs:418).

**The precondition is routine, not exotic.** I measured it with the existing headless tool, using yearly savegames from `run --save` in peacetime with no orders placed.
- Colony yards lose all arms capital while the owner stays the same: 74 times on seed 1 (years 4-30), 47 on seed 3 and 52 on seed 7 (years 8-22).
- On seed 1, 53 of the 74 had no yard again by year 30 (up to 19+ years); the other 21 came back after 1-11 years.
- At year 16, 30-39 of the 35-46 colony yards have arms capital below 0.45.
- Almost every colony arms plant has a negative return at that point, so it is being scrapped at 4 % + 25 % per year (capital.rs:78-88). "Runs at a loss" is the normal case even without a blockade.
- The claimed timing holds: 0.1 falls below 0.05 after about 28 months; a cruiser takes 200 build days (hulls.ron), and a queue holds up to 8 orders.

**The owner has no remedy.** No `SimCommand` restores arms capital, and `CancelBuild` refunds nothing. No existing test covers a pending order on a world whose yard vanished.

**Limits of what I verified.**
- I did not reproduce the hang at runtime. No existing command can place an order at a colony yard (`headless fleets --order` orders only at capital worlds, which always keep a slot), and I was not allowed to add code.
- The defect is latent today: there is no military AI yet and the game crate does not issue `BuildShip`. It becomes reachable as soon as a player or AI orders at a colony yard, which the sim API and the blockade test already allow.
- Money is conserved (`money_drift` is unaffected); this is a stuck state, not a leak.
- Two details of the finding are weaker than stated. The occupied `yard_queue` place is harmless, because a world without a yard accepts no new orders anyway. And `headless fleets` does not show such an order as "wartet auf einen Bauplatz"; it does not show it at all.

**Belege der Gegenprüfung.** Code read (all under /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim):
- crates/sim/src/fleet/yard.rs:53-75 (`yard_slots`), :124 (yard check only at placement), :189-202 (slot gate), :261-263 (completion check behind the gate), :266-287 (lapse only on owner change)
- crates/sim/src/economy/capital.rs:74-89 (depreciation, scrap, capital < 0.05 set to 0)
- crates/sim/src/economy/mod.rs:445-449 (`expected_return`; wages are not scaled by utilization, so an idle plant has a negative return)
- crates/headless/src/fleets.rs:150-154 (worlds with 0 slots skipped, orders included)
- crates/sim/src/harness.rs:293-331 and :418 (no invariant for this state; `orders` counts every order)
- crates/sim/src/fleet/mod.rs:526-539 (`find_anomaly` does not check for a yard)
- crates/sim/src/command.rs:26-117 (no command to restore arms capital; `CancelBuild` without refund)
- assets/data/hulls.ron (cruiser `build_days: 200`), assets/data/balance.ron (`yard_capital_per_slot: 4.0`, `yard_queue: 8`, `depreciation_per_year: 0.04`, `scrap_per_year: 0.25`)

Commands run:
- `target/release/headless fleets --seed 1 --years N` for N = 8..30: colony yards mostly show arms capital 0.1-0.5; listed yards fall from 52 (year 16) to 19 (year 25).
- `target/release/headless run --seed S --years N --quiet --save <scratch>` for seed 1 (years 4-30) and seeds 3 and 7 (years 8-22), then read the savegames with a small script.
- `cargo test -p sim --lib fleet::yard`: exit 0, 9 tests pass, none covers a pending order on a world without a yard.

Measurements (colony = owned world that is not the capital; arms capital = recipes 16-19):

| Seed | Years | Yard lost, same owner | Colony yards at year 16 | Of those below 0.45 |
|---|---|---|---|---|
| 1 | 4-30 | 74 | 46 | 34 |
| 3 | 8-22 | 47 | 46 | 39 |
| 7 | 8-22 | 52 | 35 | 30 |

- Seed 1: 21 yards returned after 1-11 years; 53 had not returned by year 30 (longest 19+ years).
- Colony arms plants with negative return at year 16: seed 1: 69 of 77; seed 3: 68 of 68; seed 7: 55 of 56. Mean utilization is 0.00-0.17.
- Seed 1 colony yards over time: 46 (year 16), 13 (year 25), 15 (year 30).

Scratch files are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim:
- ron.py
- yards.py
- gaps.py
- test_yard_lib.log
- save_s1_y4.ron to save_s1_y30.ron, save_s3_y8.ron to save_s3_y22.ron, save_s7_y8.ron to save_s7_y22.ron
- fleets_s1_y8.txt to fleets_s1_y30.txt

Memory: one stored note exists (about visually checking the game window in a cloud container). It does not bear on this task and I did not use it.

Possibly worth saving, learned from the savegames above: in peacetime almost all colony arms capital sits at near-zero utilization with negative returns by year 16, so colony yards disappear over time on seeds 1, 3 and 7.

### korrektheit 6 · bestätigt · minor · Crew pay, ship count and strength still include ships already destroyed in a running battle

Ort: `crates/sim/src/fleet/supply.rs:214`

**Beschreibung.** Losses are written back to `fleet.ships` only in conclude(). Until then monthly_upkeep() (lines 214-217), Fleets::ship_count, Fleet.strength and the run summary count ships whose BattleShip is long Destroyed. The treasury therefore pays `upkeep_per_month` for wrecks for the rest of the battle: normally one to three monthly payments, at the time limit up to 15. Money is conserved (it goes to households), but the war cost is overstated, and a fleet whose ships are all destroyed stays `alive`/`in_battle` until the battle ends.

**Fehlerszenario.** Carrier fleet (2 Horst, 2 Dolch) in a battle that runs to the 450-day limit: both corvettes are destroyed in the first weeks, yet pay_upkeep charges their build value x 1.2 % on each of the following ~14 month starts; `headless run` meanwhile reports 4 ships and the old strength for that fleet.

**Vorschlag.** For fleets in a battle derive upkeep (and the displayed counts) from the battle state, skipping ships whose BattleShip status is Destroyed, or write destroyed ships back at each day start instead of only at the end.

**Gegenprüfung.** I could not refute it. The mechanism is exactly as described, and it is not handled anywhere else. It stays minor: money is conserved and writing results back only at the end is a deliberate, tested design.

**Mechanism (confirmed by reading and by a passing test)**
- `write_back()` is called only from `conclude()`, which runs only for finished battles. Until then `fleet.ships` is untouched; the integration test asserts this at every day start of a running battle, and it passes.
- `monthly_upkeep()` sums the build value of every ship in `fleet.ships` of every alive fleet. It checks neither `in_battle` nor the `BattleShip` status.
- `Fleets::ship_count`, the daily `Fleets::refresh` (which sets `Fleet.strength`) and the run summary read the same unchanged ship list.

**Why it has an effect at all**
- A battle advances 2 s of battle time per calendar day (`dt` 0.1, 20 ticks per day). The 900 s limit is therefore 450 days, or 15 month starts.
- Losses happen early and the battle then drags on, so pay keeps flowing for wrecks. The practice battles reproduce the claimed scenario: 2 Horst + 2 Dolch per side hits the time limit in 8 of 8 seeds, with both corvettes gone by day 30.
- Carriers can be built in a campaign through ordinary build orders, so such fleets are reachable by commands alone.

**Size of the effect**
- Starting fleets: 36 scripted campaign battles lasted 24–55 days, so 0–2 month starts fall inside. The overpayment is at most about a hundred credits against treasuries of 15,000–19,000.
- Larger fleets with carriers: battles last 60–450 days. My estimates from the practice battles are roughly 550 credits for the claimed carrier fleet, about 2,200 for a three-carrier fleet, and about 5,000 for a 12-ship mixed fleet.
- Because `missed` is measured against the inflated `due`, a short treasury can be booked as in arrears (morale penalty, chronicle entry) even when it could pay every surviving crew.

**Other points in the finding**
- Stale strength is a visible regression: before this branch `fleet.strength` fell every day of a fight, and the unchanged game UI still displays that field. It now stays at the pre-battle value until the battle ends.
- The "fleet with no ships stays alive" part holds only in battles with three or more parties. A two-party battle ends in the same step in which one side has no acting ship.

**Not shown directly**
No existing command dumps campaign state in the middle of a battle, so I did not observe the monthly payment itself in a campaign run. The conclusion rests on the code path, the passing test and the practice battles.

**Fix direction**
Derive pay, ship count and strength from the battle state for fleets in a battle. Changing the write-back timing instead would break the existing test assertion that the fleet is unchanged while the battle runs.

**Belege der Gegenprüfung.** All paths are under /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim.

**Code**
- crates/sim/src/fleet/supply.rs:214-217 — `monthly_upkeep` = sum of `fleet.build_value(data)` over `fleets.of(polity)`, times `upkeep_per_month`; no battle check. Used by `pay_upkeep` at :237; `missed` computed against that `due` at :256-266.
- crates/sim/src/fleet/mod.rs:251-256 (`build_value` over all `self.ships`), :334-338 (`of` = alive and owner), :345-347 (`ship_count`), :433-450 (`refresh` derives strength from `ships`).
- crates/sim/src/fleet/plugin.rs:32 (upkeep at month start), :116 (`refresh` at each day start).
- crates/sim/src/fleet/battles.rs:544-553 (`conclude` only for finished battles), :783 (the only `write_back` call), :211-212 (comment that a fleet that no longer exists stays a party until the battle ends).
- crates/sim/src/battle/step.rs:1098-1122 (`check_end`: a two-party battle finishes once one side has no acting ship).
- crates/sim/src/harness.rs:413-417 (summary uses `ship_count` and `fleet.strength`).
- crates/sim/tests/military.rs:1883-1886 — "Solange gekämpft wird, bleibt jede Flotte, wie sie hineinging": `assert_eq!(fleet(&sim, fleet.id).ships, fleet.ships)` at each day start of the battle.
- assets/data/balance.ron:217 (`upkeep_per_month: 0.012`), :276 (`dt: 0.1`), :282 (`time_limit: 900.0`); crates/sim/src/clock.rs:9-10 (20 ticks per day, 30 days per month).
- Base commit: `git show d883061:crates/sim/src/fleet/mod.rs`, `resolve_skirmishes` did `fleet.strength -= damage[index]` daily and set `alive = false` at once. crates/game/src/ui/fleets.rs:58 and :99 display `fleet.strength`.

**Runs**
- `cargo test -p sim --test military -- fleets_at_war_fight_it_out upkeep_moves_exactly a_save_in_the_middle_of_a_battle three_communities`: 4 passed, exit 0.
- `target/release/headless battle --a horst:2,dolch:2 --b horst:2,dolch:2 --seeds 0..8`: 8 of 8 "Zeit um nach 901 s", each side loses 2 Kv. With `--trace 30` (seed 0): both corvettes present at 30 s, only the 2 carriers left at 60 s, unchanged to 901 s. That is about 14 further month starts × 1.2 % × 3,292 ≈ 553 credits.
- Mirrored `horst:3,waechter:4,dolch:4`: 8 of 8 at the time limit; in seed 0 all 8 escorts (15,096 credits) are gone by 150 s (day 75), about 12 payments × 181 ≈ 2,200 credits.
- Mirrored `bastion:1,horst:2,speer:2,waechter:3,dolch:4`, seed 7: loser down to one ship at 120 s, battle ends at 732 s (day 366); about 10 payments on roughly 43,900 credits of destroyed hulls ≈ 5,000 credits. The other seeds were decided after 129–280 s.
- `target/release/headless run --quiet --seed 0..11 --years 4 --war 1:0:1 --war 1:2:3 --war 1:4:5`: 36 battles, 24–55 days each. `run --seed 3 --years 30 --war 5:0:1`: battle 21.01.2405–18.02.2405, which spans the month start 01.02.2405.
- `target/release/headless fleets --seed 3 --years 1`: starting fleet build value 13,570, pay 163 per month, treasuries 14,880–19,448.
- `target/release/headless fleets --seed 3 --years 8 --order horst:1,dolch:2 --polity 0`: the fleet then holds 1 Träger, pay 332 per month.

Logs are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim (test_verify_upkeep.log, run_seed3_war.log, dur_0.log to dur_11.log).

### korrektheit 7 · bestätigt · minor · headless run panics when --war/--peace names a community that does not exist

Ort: `crates/headless/src/war.rs:117`

**Beschreibung.** parse_event() accepts any number for A and B. The scripted command is rejected by the simulation and logged with the bogus PolityId; print_aftermath() then prints rejected commands with `sim.polities().get(entry.polity).name`, which indexes the polity list unchecked. `sweep` and `hash` are not affected because they do not print the aftermath. Related: WarScript::issue (crates/sim/src/harness.rs lines 484-508) issues SetStance Blockade and MoveFleet even when the declaration of war was rejected, so `--war 1:0:9` leaves all fleets of community 0 in blockade stance without a war.

**Fehlerszenario.** `target/release/headless run --seed 3 --years 2 --quiet --war 1:9:1` (6 communities): 'thread main panicked at crates/sim/src/polity/mod.rs:35:19: index out of bounds: the len is 6 but the index is 9', exit code 101, no summary. Same with `--peace 1:8:1`.

**Vorschlag.** Check A and B against the number of communities before running (clear German error message) or use `polities().list.get(..)` when printing; in WarScript::issue send the fleet orders only if both communities exist.

**Gegenprüfung.** I tried to refute the finding and could not: the panic reproduces exactly as claimed with the up-to-date release binary. It is a real crash, though minor (developer tool, out-of-range input, simulation core unaffected).

Code path, read independently:
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/headless/src/war.rs:13-32 — parse_event accepts any u32 for A and B and only rejects A == B.
- crates/sim/src/harness.rs:478-509 — WarScript::issue queues DeclareWar/MakePeace through Headless::issue (harness.rs:78-84, CommandSource::Player) with the bogus PolityId as issuer.
- crates/sim/src/diplomacy/mod.rs:99-113 — check_pair rejects it cleanly ("Gemeinschaft besteht nicht mehr").
- crates/sim/src/command.rs:238-255 — the rejection is logged with polity = the bogus id.
- crates/headless/src/war.rs:112-121 — print_aftermath formats every rejected entry with sim.polities().get(entry.polity).name.
- crates/sim/src/polity/mod.rs:34-36 — get() is an unchecked `&self.list[id.index()]`, hence the panic.

Why it matters beyond cosmetics:
- The crash comes after the whole simulation has run, so the summary is lost and a requested --save file is never written.
- The crashing path is the tool's own documented feedback channel. The WarScript doc comment says rejected commands appear in the command log, print_aftermath's doc says it prints rejected commands, and WarScript::issue already uses list.get(b.index()), so nonexistent communities are an anticipated case that the print path missed.
- Other bad input (not a number, A == B) gets a German error from the value parser, and sweep/hash accept the same arguments without crashing, so the behaviour is inconsistent.

Corrections to the finding's scope:
- Only a nonexistent A (the issuer) panics. A nonexistent B is reported correctly and the run exits 0.
- The related sub-claim is only partly right and has no simulation effect. With a nonexistent B, WarScript::issue sends only SetStance Blockade; no MoveFleet is sent because the target is None (harness.rs:485-490, 505-507). Blockade stance does nothing without a war (crates/sim/src/fleet/movement.rs:88-92 requires diplomacy.at_war), so only the stored stance field and the state hash change. MoveFleet after a rejected DeclareWar happens only when the rejection is "es herrscht bereits Krieg", which is harmless. I rate that sub-point cosmetic.

Note for the fix: the polity list can grow at runtime (crates/sim/src/finance/cohesion.rs:520 pushes "Freistaat" polities; balance.ron max_polities: 12 against galaxy.ron polities: 6). A pre-run range check against the initial count would wrongly forbid scripting a later Freistaat. The safer fix is polities().list.get(entry.polity.index()) with a fallback label in print_aftermath; a pre-run check is optional on top.

Nothing from memory was used; the one stored note (about visually checking the game window in a cloud container) is unrelated.

**Belege der Gegenprüfung.** Build: `cargo build --release -p headless` exit 0, binary already up to date with HEAD 4b64d65 (log: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_warpanic_build.log).

1) `target/release/headless run --seed 3 --years 2 --quiet --war 1:9:1` -> exit 101, stdout empty, stderr:
   thread 'main' panicked at crates/sim/src/polity/mod.rs:35:19: index out of bounds: the len is 6 but the index is 9
   backtrace: core::panicking::panic_bounds_check <- headless::war::print_aftermath <- headless::main

2) `target/release/headless run --seed 3 --years 2 --quiet --peace 1:8:1` -> exit 101, "index out of bounds: the len is 6 but the index is 8".

3) Same as (1) plus `--save <file>` -> exit 101, save file not created.

4) `target/release/headless run --seed 3 --years 2 --quiet --war 1:0:9` -> exit 0, prints
   "Abgelehnt (01.01.2401, Karawane von Meridian): DeclareWar { target: PolityId(9) }: Gemeinschaft besteht nicht mehr"
   followed by the normal summary. Saved with --save: `wars:[]`, yet `stance:Blockade` appears twice and `stance:Guard` five times, consistent with community 0's single fleet in Blockade stance plus the logged SetStance command (six fleets in total, one per community) and no war.

5) Not affected, as claimed: `headless hash --seed 3 --days 720 --war 1:9:1` -> exit 0; `headless sweep --seeds 3..4 --years 2 --war 1:9:1` -> exit 0.

6) `headless sweep --seeds 0..8 --years 60` (peaceful) -> exit 0, exactly 6 communities in every seed, so indices above 5 are out of range in default runs.

Relevant lines: crates/headless/src/war.rs:117 `sim.polities().get(entry.polity).name`; crates/sim/src/polity/mod.rs:35 `&self.list[id.index()]`; crates/headless/src/war.rs:25-30 (no range check); crates/sim/src/harness.rs:484-508 (fleet orders issued regardless of the DeclareWar outcome).

## Blickwinkel: anforderungen

### anforderungen 1 · bestätigt · major · Colony shipyards flicker with noise-level arms capital; an order whose yard vanishes freezes silently

Ort: `crates/sim/src/fleet/yard.rs:198`

**Beschreibung.** `yard_slots` (yard.rs:53-75) makes any world with arms capital > 0 a full one-slot yard, and the monthly capital step zeroes every recipe stock below 0.05 (economy/capital.rs:86-88). Colony arms capital sits right at that cut-off (median 0.10-0.49, minimum at or below 0.05 in every saved state I inspected), so yard status is unstable. Seed 3 with no orders at all: 52 yards in year 16, 40 in year 18, 32 in year 20, 26 in year 22, 17 in year 30; 10 of the 40 yards of year 18 are gone two years later. Seeds 5 and 8 lose 8 and 10 of 40 and 37 yards between years 14 and 18. When this happens to a world with open orders, `advance_yards` sees `slots == 0`, takes the 'waits for a free slot' branch (yard.rs:196-202), sets `stalled = false` and never advances the order again. Nothing is logged, nothing lapses, the money paid stays locked in `spent`, cancelling refunds nothing, and `headless fleets` skips the world because it lists only worlds with slots (crates/headless/src/fleets.rs:151-154), so the order vanishes from the tool output. `order_ship` meanwhile rejects new orders there with 'hat keine Werft'. The same flicker switches repair on and off (supply.rs:160-166). No test covers a yard that ceases to exist; the only lapse test is the change of owner (yard.rs:878-911). The implementers listed 'every world with 0.05 arms capital counts as a yard' as an open point, but not this consequence. On hard-coded levers: I scanned the non-test code of fleet/ and found no numeric balance literal; the two constants that shape yards and procurement live in the economy: this 0.05 cut-off and `BUYER_SHARE = 0.5` (economy/mod.rs:52). Not reproduced end to end: `headless fleets --order` places orders only on capital worlds, which always keep a slot; the turnover of yards is measured, the frozen order follows from the code.

**Fehlerszenario.** A military AI or the player orders a cruiser (200 build days) or battleship (420) at a colony whose only arms capital is 0.06-0.15 units of an unprofitable arms recipe. The stock decays below 0.05 within months and is set to 0. From then on the order has `stalled == false` and a frozen `progress` in a queue without a slot; a UI following the documented rule (the first `yard_slots` entries are being built, the rest wait) shows it as waiting for a slot forever.

**Vorschlag.** Define a yard independently of the 0.05 rounding cut-off: a `military` lever for the minimum arms capital with hysteresis, or keep a yard alive while it has orders. In `advance_yards` handle `slots == 0` explicitly: mark the order (stalled or a new state) and write a chronicle entry, or let it lapse with one. List slot-less worlds that have orders in `headless fleets`. Add a test.

**Gegenprüfung.** I tried to refute the finding and could not: every checkable sub-claim holds, and the measured numbers reproduce exactly. The freeze itself is not reproduced end to end, because no existing tool or test places an order on a colony whose yard then disappears.

**What the code does**
- `yard_slots` (`crates/sim/src/fleet/yard.rs:71-74`) returns 0 only when arms capital is exactly 0 on a non-capital world; any positive amount gives a full slot via `.max(1)`.
- The monthly capital step (`crates/sim/src/economy/capital.rs:85-88`) sets every recipe stock below 0.05 to exactly 0. This cut-off predates the branch; the yard gate that depends on it is new.
- In `advance_yards` (`yard.rs:196-202`), `slots == 0` gives `*used >= slots` as `0 >= 0`, so the order takes the "Wartet auf einen freien Bauplatz" branch: `stalled = false; continue`. No progress, no demand reported, no chronicle entry, no lapse.
- Nothing else removes or flags such an order. Orders leave only by completion, owner change (`yard.rs:192-195`) or `cancel_order`, which refunds nothing by design.
- `order_ship` rejects new orders there with "hat keine Werft" (`yard.rs:124-129`).
- `headless fleets` skips worlds without slots (`crates/headless/src/fleets.rs:151-154`), so their orders are not printed.
- Repair uses the same gate (`crates/sim/src/fleet/supply.rs:160-166`).

**Why it matters**
- The documented contract gives a consumer no way to tell this state from normal queueing: `BuildOrder.stalled` (`yard.rs:35-37`) and `Fleets::orders_at` (`crates/sim/src/fleet/mod.rs:364-366`) both describe it as waiting for a slot.
- The implementers handled the analogous cases explicitly (owner change lapses with a chronicle entry; enemy in orbit holds the ship in dock). This case was overlooked.
- One further consequence the finding did not mention: the finished check (`yard.rs:261-263`) sits behind the slot gate. A completed, fully paid ship held in dock is never delivered once the world's slots reach 0.
- The precondition is the norm, not an edge case. In seed 3, 49 of 90 worlds are yards in year 14, and only 12 remain in year 26.

**Limits**
- Not reproduced end to end. `headless fleets --order` orders only at capital worlds, which always keep a slot. Tests load workspace data and order at capitals, or at a colony with +1.0 arms capital for at most a year (`crates/sim/tests/military.rs:643-760`).
- "Never advances again" is slightly overstated. The order resumes if arms capital reappears; some worlds do flicker back, but most stay without a yard for many years.
- An open order's demand for parts may keep alive arms works that produce those parts. With orders at the capitals from year 12, capital-world arms capital at year 20 was roughly 0.2–0.6 higher. It cannot help when the treasury is empty (no demand is reported), under blockade, or when the capital sits in the ammunition works.
- No conservation or determinism rule is broken. The game crate does not issue `BuildShip` yet, so today only API callers can order at colonies. I would rate it medium rather than major.

The 10 existing yard unit tests pass; none covers a yard that ceases to exist.

**Belege der Gegenprüfung.** Commands run in the worktree (logs in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_yard):

1. `target/release/headless fleets --seed 3 --years N`, counting "Werft" lines:
   - year 10: 34; 12: 41; 14: 49; 16: 52; 18: 40; 20: 32; 22: 26; 26: 12; 30: 17
   - Matches the finding's 52 / 40 / 32 / 26 / 17.

2. Seed 3, yards lost per interval:
   - 16→18: 13 gone, 1 new
   - 18→20: 10 gone, 2 new (Beleth II, Hesor II, Navenax I, Navoune I, Nyis II, Quiris I, Thoathoth II, Vorys II, Yroa II, Zelune III; all had arms capital 0.1–0.2)
   - 20→22: 7 gone, 1 new
   - 22→26: 15 gone, 1 new

3. Seeds 5 and 8, years 14→18:
   - seed 5: 40→37 yards, 8 gone
   - seed 8: 37→29 yards, 10 gone
   - Both match the finding.

4. Arms capital of the yards, seed 3 (`yard_capital_per_slot` is 4.0):
   - year 16: 52 yards, values 0.1–1.6, median about 0.25
   - year 20: 16 of 32 yards show 0.1
   - Non-capital worlds Aren II and Yroa II are listed in year 10 as "1 Bauplatz (Rüstungskapital 0.0)", i.e. a full yard on less than 0.05 units.

5. Per-world history, seed 3, years 10/12/14/16/18/20/22/26/30:
   - Navenax I: 1.0, 0.9, 0.5, 0.3, 0.2, then no yard through year 30
   - Bryheim I: 0.7, 0.4, 0.2, 0.1, then none
   - On/off worlds: Beleth II, Saeyseth II, Thoathoth II, Aren II
   - Only the six capital worlds and a few neighbours persist throughout.

6. `target/release/headless economy --seed 3 --years 18 --system Navenax`:
   - Navenax I: Munitionswerk 0.1 and Waffenschmiede 0.1, both at utilisation 0.00, returns -7.61 and -12.62. Idle works being scrapped, yet the world counts as a one-slot yard.

7. `target/release/headless fleets --seed 3 --years 20 --order speer:2,dolch:4 --order-year 12`:
   - All orders at the capitals completed (12 ships each).
   - Capital-world arms capital rose, e.g. Zeloth I 0.4→0.8, Lumheim I 0.5→1.1.
   - The set of colony yards changed (six gone, six new), which shows how sensitive yard status is.

8. `cargo test -p sim --lib yard`: exit 0, 10 passed. The only lapse test is `an_order_lapses_when_the_world_changes_hands`.

9. Code read:
   - `crates/sim/src/fleet/yard.rs:35-37, 71-74, 124-129, 153-163, 189-202, 261-263`
   - `crates/sim/src/economy/capital.rs:74-89, 186`
   - `crates/headless/src/fleets.rs:43-54, 79-96, 150-154`
   - `crates/sim/src/fleet/supply.rs:160-166`
   - `crates/sim/src/fleet/mod.rs:364-366`
   - `crates/sim/src/command.rs:414-433`
   - `crates/sim/tests/military.rs:643-760`

### anforderungen 2 · bestätigt · major · Regrouping gives 30 days of immunity with full freedom of movement: blockades can be lifted for free and a beaten fleet can return to the field at once

Ort: `crates/sim/src/fleet/battles.rs:435`

**Beschreibung.** A regrouping fleet is neither attacker nor target (`open_battles` collects only `is_ready()` fleets, battles.rs:433-438), but it accepts march orders (`order_move` rejects only `in_battle`, fleet/mod.rs:589-594; tests/military.rs:2706-2708 asserts the accepted order) and it is repaired and, outside a blockade, resupplied like any docked fleet (supply.rs:54-58 and 153-158 have no regroup condition). A retreat ordered on the first day is lossless (asserted for two starting fleets, military.rs:2695-2700; the sides start 2600 units apart). Three consequences follow directly. (a) Lifting a blockade for nothing: one ship of the system owner enters the blockaded system; next morning a battle opens, the blockader is `in_battle` and no longer counts (movement.rs:88-92), and the owner's fleet counts as orbit defence even while fighting (movement.rs:85-87), so the system is open. The owner orders `Retreat`; the ship leaves the field untouched in one to two weeks (my estimate from exit radius and ship speeds). The next token ship arrives, and so on. A starting fleet split into single ships keeps a system open permanently without a shot; the blockader cannot react, because it is in battle and MoveFleet is rejected. `a_fleet_in_battle_or_regrouping_does_not_blockade` asserts the open system during such a battle (military.rs:3064-3078). (b) The forced withdrawal is optional: right after the battle the loser issues `MoveFleet` to the system it stands in. With `progress == 0` the fleet is docked, the branch `fleet.system == to` clears the retreat route (fleet/mod.rs:608-611), and the fleet stays beside the victor, untouchable for 30 days; in its own system with a yard it is repaired at 1 % a day meanwhile. If it has already left, the same order brings it back within the immunity. (c) Under immunity a fleet marches through defended systems for 30 days (660 map units), so a chokepoint fleet stops nobody who retreats first. Derived from the code and from the tests' own assertions; not reproduced with the CLI, which cannot issue `Retreat`.

**Fehlerszenario.** Player blockaded by an enemy fleet: split the home fleet into single-ship fleets, send one into the blockaded system every week and order Retreat each time. The system is not blockaded on any day, no ship is lost. Or: lose a battle at home, order MoveFleet to the home system, repair for 30 days under the victor's guns, fight again.

**Vorschlag.** Tie the immunity to the withdrawal: end `regroup_until` (or make the fleet attackable) when a regrouping fleet enters or stays in a system with ready hostile fleets, and reject march orders that cancel the retreat route until the fleet has arrived. For blockades, keep the system blockaded while the blockader's side still holds the field, or do not count a retreating owner fleet as orbit defence. Add tests for both.

**Gegenprüfung.** I could not refute it. Every factual claim checks out in the code, and consequence (a) defeats the blockade mechanic at no cost. It is a design exploit rather than a crash, and it was not reproduced dynamically because no existing command can issue Retreat or SplitFleet; the verdict rests on code reading, passing tests that assert the building blocks, and a hand calculation.

**What holds, by reading the code**
- **Regrouping fleets are untouchable.** `open_battles` only collects fleets that are docked and `is_ready()` (battles.rs:432-438), so a regrouping fleet neither starts a battle nor can be attacked. The README says the same ("wird nicht angegriffen").
- **March orders are accepted while regrouping.** `order_move` rejects only `in_battle` (fleet/mod.rs:589-594), and `move_fleets` has no regroup condition.
- **Repair and resupply ignore regrouping.** Neither `resupply_fleets` nor `repair_fleets` checks it (supply.rs:54-58, 153-158).

**(a) Blockade lifted for free — the material part**
- `update_blockades` (movement.rs:85-93) needs a ready blockader, and it counts any owner fleet that is not regrouping as orbit defence. A fleet in battle counts, even one that has been ordered to retreat.
- A retreat ordered on day one cannot be punished. The sides start 2600 apart (spawn_radius 1300), the longest weapon reaches 900, and sensor range is 1700. Once the token is Routing, the blockader's formation stops, because `command()` counts only Active enemies (battle/step.rs:107-131).
- The battle still runs until the ship crosses exit_radius 2400 (step.rs:1086-1122). By my hand calculation that is about 9 days for a corvette, 11.5 for a frigate and 15.5 for a cruiser. The system is open for that whole time.
- With no losses there is no war weariness and no share-price shock (`owner.lost <= 0`). The only cost is 30 days of regrouping per fleet, which rotation dilutes. In an idealised schedule the six starting ships as single-ship fleets give 0 blockaded days over two years; four corvettes give about 7 %.
- The blockader has no counter: it is locked in battle, and any further fleet it brings joins the same battle.

**(b) Forced withdrawal can be cancelled**
- `conclude` sets `route = [target]`, `progress = 0` (battles.rs:814-822). The fleet is still docked, so MoveFleet to its own system hits the old branch `fleet.system == to → route.clear()` (fleet/mod.rs:608-611).
- Commands run before movement in the tick order, so the order always lands in time. The existing player UI already enables exactly this button when the route is non-empty (crates/game/src/ui/fleets.rs:70-75).
- This contradicts the README and balance.ron ("weicht in ein Nachbarsystem aus und sammelt sich dort"). The fleet stays at its own yard, is repaired at 1 % a day, and can host newly built ships.

**(c) Marching through defended systems**
- A regrouping fleet can be re-routed onward and passes any defended system for 30 days (22 units a day, 660 units). No battle can open against it.

**Caveats**
- The building blocks are deliberate and tested: no attack on regrouping fleets, march orders accepted, "wer kämpft, sperrt nicht". So (c), and partly (b), are weaknesses of the documented design; returning to the field by marching back is allowed anyway.
- The scenario's "send one every week" is imprecise. A token arriving during a running battle joins it as reinforcement, and all sides of one battle get the same regroup end. The cadence that works is the next token arriving the morning after the previous battle ended.
- Severity "major" is arguable; I would call it medium to major. The spec item "a blockade lets prices rise and factories (and yards) stand still" can be nullified by its victim at will.

No memory notes were available or used.

**Belege der Gegenprüfung.** All paths are under /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim.

**Code**
- crates/sim/src/fleet/battles.rs:432-438 — only docked, `is_ready()` fleets enter battles.
- crates/sim/src/fleet/battles.rs:807-831 — the loser gets `regroup_until = day + regroup_days`, `route = vec![target]`, `progress = 0.0`.
- crates/sim/src/fleet/mod.rs:589-594 and 608-611 — `order_move` rejects only `in_battle`; `if fleet.system == to { fleet.route.clear(); return Ok(()) }`. That branch predates this branch (`git show d883061`).
- crates/sim/src/fleet/movement.rs:85-93 — `defended = owner fleet && !is_regrouping()`; `besieged` needs `is_ready()`.
- crates/sim/src/fleet/supply.rs:54-58, 153-158 — no regroup condition.
- crates/sim/src/battle/step.rs:107-131 — formation halts when no Active enemy remains.
- crates/sim/src/battle/step.rs:301-309, 1086-1090, 1098-1122 — Routing steering, escape at exit_radius, battle end.
- crates/sim/src/battle/mod.rs:413-421 — sides 0 and 1 spawn opposite each other at spawn_radius.
- assets/data/balance.ron — regroup_days 30, spawn_radius 1300, exit_radius 2400, sensor_range 1700, fleets.speed 22. Longest weapon range is 900 (assets/data/modules.ron).
- crates/game/src/ui/fleets.rs:70-75 — "Marsch zum Ziel" is enabled for the fleet's own system when its route is non-empty.

**Existing tests that assert the building blocks**
- crates/sim/tests/military.rs:2695-2708 — lossless early retreat; march order accepted while regrouping.
- crates/sim/tests/military.rs:2101-2134 — the victor pursues and nobody attacks the regrouping fleet.
- crates/sim/tests/military.rs:3064-3078 — the system is open for as long as the blockader fights.
- crates/sim/src/fleet/movement.rs:265-273 — a defender in battle prevents the blockade.

**Runs**
- `cargo test -p sim --test military`: 29 passed, exit 0.
- `target/release/headless battle --a dolch:1 --b speer:1,waechter:2,dolch:3 --trace 2`: anchors start about 2600 apart, the blockader closes at about 38.7 units/s, first shots at about 16 s (day 8). A corvette that does not retreat is destroyed at 24 s.
- `target/release/headless run --seed 3 --years 6 --war 2:0:1`: exit 0; one 28-day battle of two starting fleets.

**Hand calculation**
- A small Python re-implementation of the Routing movement gives battle lengths of 9.0 days (dolch), 11.5 (waechter), 15.5 (speer) and 23 (bastion) for a day-one retreat.
- Rotation model with 30 days of regrouping: six starting ships as single-ship fleets, 0 blockaded days of 721; four corvettes, 54 of 720; three corvettes, 216 of 720.

**Files are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim**
- verify_regroup_tests1.log
- verify_regroup_tests2.log
- verify_regroup_run1.log
- verify_regroup/retreat_time.py

### anforderungen 3 · bestätigt · major · Holding station costs neither fuel nor ammunition: a blockade far from home never runs dry

Ort: `crates/sim/src/fleet/movement.rs:60`

**Beschreibung.** Fuel is burned only per distance travelled (movement.rs:60-63) and ammunition only inside the battle calculator; a docked fleet consumes nothing, and `update_blockades` (movement.rs:79-98) asks only for a ready fleet in blockade stance, not for strength or supplies. In `headless run --seed 3 --years 30 --war 10:0:1` the attacker wins the battle of February 2410 with one cruiser left (hull 0.18, strength 5.5 of 100). The saved states of years 11, 20 and 30 show that ship with bit-identical `ammo 0.82053596, fuel 0.51105237` and the enemy home system blockaded throughout: 19 years without one unit of supply, for 53 credits of pay a month. The blockade is not harmless for the victim: price level at the blockaded capital 0.90 and 1.16 in years 1-2 against 0.59 and 0.65 in the peace run of the same seed, consumer-goods coverage 0.44 and 0.40 against 0.65 and 0.62, household money -41 %; in seed 4 the capital stock of the blockaded capital is 20 % lower and consumer coverage 0.37 against 0.90. The capital's own yard cannot help, because it keeps finished ships in the dock while an enemy is in orbit (yard.rs:314-320). So 'long campaigns far from home run dry' holds for marching and repeated battles only; the cheapest and most damaging long campaign is free. The integration test pins this behaviour: `travelling_burns_fuel_and_there_is_no_resupply_abroad` asserts unchanged tanks after 20 days abroad (tests/military.rs:885-889).

**Fehlerszenario.** Any fleet remnant that reaches an undefended enemy system in blockade stance blockades it indefinitely with whatever fuel and ammunition it arrived with; nothing in supply, treasury or readiness ever forces it home.

**Vorschlag.** Give station-keeping outside own territory a daily cost in fuel (and readiness or ammunition) as a `military` lever, and let a fleet with empty tanks or below a strength threshold stop counting as a blockader; alternatively scale the blockade with `Fleet.strength`. Add a test that a blockade abroad ends when the supplies are gone.

**Gegenprüfung.** I could not refute it. Every factual claim reproduces, and the gap is a requirement gap ("supply with ammunition and fuel matters"), not a matter of style. It is a deliberate design with an unintended consequence, so I would rate it medium rather than major.

**What the code does**
- Fuel is only reduced in `move_fleets` for fleets that have a route (`crates/sim/src/fleet/movement.rs:43`, `:60-63`).
- Outside tests, ammunition and fuel are otherwise written in only two places: the battle write-back (`fleet/battles.rs:592-593`) and the top-up at home (`fleet/supply.rs:83-84`). A docked fleet abroad loses nothing.
- `update_blockades` (`movement.rs:79-98`) needs only a ready fleet in Blockade stance, a war, and no docked fleet of the owner. Neither strength nor supplies are checked.
- Raid and escort use `Fleet.strength`, which is build value times hull and module condition (`fleet/mod.rs:122-125`, `:163-190`). So a fleet with empty tanks and magazines blockades, raids and escorts at full effect.
- `README.md:186` and `fleet/mod.rs:3-5` say the blockade "rechnet mit dieser Zahl" (strength). The code does not.
- `yard.rs:314-320` keeps finished ships in the dock while an enemy is docked and no own fleet is present, so the victim's capital yard cannot lift the blockade.
- The behaviour is pinned by `tests/military.rs:885-889` and by the unit test comment "Wer liegt, verbraucht nichts" (`movement.rs:161-163`).

**What the runs show**
- In seed 3 the attacker keeps one cruiser (hull 0.18, strength 5.46 of 100). Its ammunition and fuel are bit-identical at years 11, 12, 15, 20 and 30, and the victim's home system is blockaded at each of those points.
- Seed 4 shows the same pattern with a cruiser at strength 25.
- The attacker buys no ammunition or fuel after arrival, because resupply only happens in own systems (`supply.rs:54-60`). Its treasury is higher than in the peace run at every sampled year, because lost ships lower the pay.
- The victim is hurt as the finding says: consumer-goods coverage and household money at the capital drop sharply in years 11-12 in both seeds.
- The blockade also costs the victim a flat 10 cohesion points on the capital and a 0.45 risk discount on its issuers for the whole period (`finance/cohesion.rs:216-218`, `finance/shares.rs:412-423`).

**Why this is still less than "major"**
- War weariness rises 0.6 per month for both sides. The attacker ends at cohesion 44-45 against about 68 in peace, and in seed 4 loses a planet to secession. So "nothing forces it home" is true for supply, treasury and readiness, but a long war is not free.
- Peace is unilateral today (`diplomacy/mod.rs:73-97`). The 19-year duration comes partly from the script having no military AI and no `--peace`.
- "Most damaging" is overstated for the long run. In seed 3 the blockaded capital adapts after about three years and ends larger than in the peace run. In seed 4 the lasting damage is household money (-34 %) and consumer coverage (0.80 against 1.0).
- Fuel per distance only was already the data design at the base commit (`balance.ron` at `d883061`), so the implementer followed the plan.

**Verdict**
The marginal supply cost of holding a blockade is zero, and nothing ties the ability to blockade to fuel, ammunition or strength. The headless war scenario therefore runs 20 years of blockade without a single supply purchase. For the one operation the specification names alongside supply, supply has no effect.

No facts from memory were used; the only stored note is unrelated.

**Belege der Gegenprüfung.** All commands were run in `/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim` with the existing `target/release/headless` (built 21:41, newer than every source file). Logs and saves are in `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/vstation`.

**1. Seed 3, war run**
`headless run --seed 3 --years 30 --war 10:0:1 --save .../war3_y30.ron` (exit 0)
- Chronicle: battle at Zeloth from 21.01.2410 to 18.02.2410. "1. Flotte (Karawane)" loses 5 of 6 ships and holds the field; "1. Flotte (Liga)" ceases to exist.
- Save at year 30: `fleets:(list:[(id:0,owner:0,...,strength:5.462236,stance:Blockade,system:44,...ships:[(id:0,design:3,hull:0.18181895,...,ammo:0.82053596,fuel:0.51105237,...` and `blockaded:[44]`.
- Summary at year 30: attacker Kriegsmüdigkeit 100, Zusammenhalt 44; victim 100 and 39; neutral communities 66-68.

**2. War against peace, seeds 3 and 4, years 10, 11, 12, 15, 20, 30**
Saves `w{seed}_y{year}.ron` and `p{seed}_y{year}.ron`, compared with my own reader (`ronparse.py`, `compare.py`; output in `compare3.txt` and `compare4.txt`).
- Seed 3, attacker ship (hull, ammo, fuel) is `(0.18181895, 0.82053596, 0.51105237)` at years 11, 12, 15, 20 and 30; blockaded is `[44]` each time.
- Seed 4, attacker ship is `(0.7902599, 0.81767863, 0.47715774)`, strength 25.23, at the same years; blockaded is `[14]` each time.

Victim's capital, war against peace:

| Seed | Year | Household money | Consumer coverage (war / peace) | Capital stock |
|---|---|---|---|---|
| 3 | 11 | -35.8 % | 0.443 / 0.649 | -8.3 % |
| 3 | 12 | -42.6 % | 0.400 / 0.617 | -11.6 % |
| 3 | 30 | +14.5 % | 0.82 / 0.92 | +44.1 % |
| 4 | 11 | -90.3 % | 0.111 / 0.934 | -13.9 % |
| 4 | 12 | -62.5 % | 0.373 / 0.895 | -19.6 % |
| 4 | 30 | -34.4 % | 0.80 / 1.0 | -1.7 % |

- Seed 3 at year 30: the capital's population is +31.7 % against peace.
- Attacker treasury, war against peace: seed 3 between +11.7 % and +25.4 %; seed 4 between +3.9 % and +22.9 %.
- Seed 4 at year 30: the attacker has 11 planets against 12 in peace; "Freistaat Cymheim III" has seceded.

**3. Pay**
`headless fleets --seed 3 --years 1 --polity 0 --ships`: build value 13570, pay 163 per month for the full fleet. With 68 % of the build value lost, the remaining cruiser costs about 52 per month.

**4. Tests**
`cargo test -p sim --test military -- travelling_burns_fuel_and_there_is_no_resupply_abroad a_fleet_in_battle_or_regrouping_does_not_blockade` (exit 0, 2 passed; log `test_military_subset.log`). The first asserts an unchanged tank after 20 days abroad (`crates/sim/tests/military.rs:885-889`).

**5. Code references**
- `crates/sim/src/fleet/movement.rs:43`, `:60-63`, `:79-98`, `:161-163`
- `crates/sim/src/fleet/supply.rs:54-60`, `:83-84`
- `crates/sim/src/fleet/battles.rs:592-593`
- `crates/sim/src/fleet/mod.rs:3-5`, `:122-125`, `:163-190`, `:246-248`, `:454-473`
- `crates/sim/src/fleet/yard.rs:302-320`
- `crates/sim/src/logistics/mod.rs:138-147`
- `crates/sim/src/finance/shares.rs:412-423`
- `crates/sim/src/finance/cohesion.rs:216-218`
- `crates/sim/src/finance/plugin.rs:130`
- `crates/sim/src/diplomacy/mod.rs:73-97`
- `README.md:186`, `:193-195`
- `assets/data/balance.ron`: `weariness_per_month: 0.6`, cohesion `weariness: 0.25`, `blockade: 10.0`, `fuel_per_distance: 0.0012`; `git show d883061:assets/data/balance.ron` already has only `fuel_per_distance` and `dry_speed` as fuel levers.

### anforderungen 4 · bestätigt · major · Fleet costs leave no measurable trace in the treasury; arrears never occur, and war makes treasuries grow

Ort: `crates/sim/src/fleet/supply.rs:222`

**Beschreibung.** Measured with the headless tools. (1) Upkeep of the starting fleet is 163 credits a month against treasuries of 16k-34k. (2) Seed 3, community 0 orders 2 battleships, 1 carrier, 2 cruisers and 3 corvettes in year 0: after 8 years the fleet is worth 60.9k instead of 13.6k, upkeep is 730 instead of 163, the state has paid about 50k for parts and yard fee, and the treasury stands at 17.2k against 20.1k without orders (about 6 % of the spending). (3) All six communities order a battleship, 2 cruisers and 4 corvettes in year 20 (fleet value and upkeep times 3): four years later four of six treasuries are higher than in the run without orders (differences -353, +2467, +2267, +1376, +2838, -23). The battleship is on schedule (82-86 % after 360 of 420 days) and costs about 8.2k including the fee instead of the nominal 13.4k, because part prices sit at 0.25-0.5 of base; hull segments are usually at the price floor, in peace and war. (4) In scripted wars treasuries rise. One year into the war: 25.2k vs 20.1k and 28.2k vs 16.4k (seed 3), 55.6k vs 22.3k and 44.1k vs 17.1k (seed 5), 20.3k vs 18.6k and 24.9k vs 20.5k (seed 8); household money falls instead, by up to 41 % (65 % under blockade in seed 4). No state had any debt in any state I inspected. (5) Arrears are 0.0 % at the end of every run of four sweeps (8x60 years peace, five-year war, three endless wars; 12x40 years with twelve overlapping wars). Causes, all outside fleet/: the state hands half of everything above its reserve to households each month (economy/capital.rs:399-405), so spending displaces transfers; the central bank mints for the state while the price level is below 0.95, and it is about 0.6 (capital.rs:461-470); at war the AI sets a reserve of 24 months of revenue (ai/plugin.rs:139). Consequences for this branch: the arrears rule (supply.rs:256-281), `morale_per_arrears` (battles.rs:277-286) and the README sentence about growing arrears are exercised only by tests that fabricate the state (2000 extra cruisers, tests/military.rs:1134; arrears 1.0 and weariness 100 written by hand, military.rs:2724-2725). The implementers reported upkeep as fiscally neutral for now and expected it to bite in war; the runs show it does not, also not at 3 to 4.5 times the starting fleet. Caveat: the tools place orders on one yard per community; a player using every colony yard at once could spend more.

**Fehlerszenario.** `headless fleets --seed 3 --years 8 --order bastion:2,horst:1,speer:2,dolch:3 --polity 0` gives treasury 17155 and arrears 0 %; the same call without `--order` gives 20061. `headless run --seed 5 --years 11 --war 10:0:1` more than doubles both belligerents' treasuries against the peace run. Neither player nor AI ever has to weigh a ship against anything else the state pays for.

**Vorschlag.** Balance decision to take together with the military AI: raise `military.upkeep_per_month` by roughly an order of magnitude (the implementers put the present upkeep at 2.5-6 % of income tax), and/or keep the war reserve out of the monthly hand-out so that purchases actually reduce it. Add a sweep-level check that a doubled fleet lowers treasury or transfers measurably. Until then describe the arrears path as unreachable in AI games.

**Gegenprüfung.** I could not refute it: every number in the finding reproduces, and the code explains why. It is a balance defect in the `military` levers under review, not a code bug. I would rate it medium rather than major.

**Why upkeep has no effect**
- `pay_upkeep` (crates/sim/src/fleet/supply.rs:222-255) pays the households by population through `pay_households`. That is the same function `settle_public_accounts` (economy/capital.rs:399-405) uses in the same month-start tick to hand out half of the treasury excess over the reserve.
- So while upkeep stays below the state's monthly surplus, it only relabels part of that transfer. Households receive the same total, and the steady-state treasury sits about twice the monthly upkeep lower.
- From the save files, the surplus is about 9-12k credits a month per community (reserve about 7k, treasury after the monthly books 15-20k). Upkeep of the starting fleet is 163, about 1.5 % of that. Money creation alone brings in about 330 a month per community.
- Arrears need upkeep above the treasury after the hand-out, which is at least the reserve unless spending has already pushed it lower. At 1.2 % a month that means a fleet roughly 40 times the starting one.
- With the largest order the tool allows (8 battleships, fleet value 7.7 times the start, upkeep 1257), all 12 communities in seeds 3 and 5 still show 0 % arrears.

**Why it matters**
- A full restock plus full repair of a starting fleet costs roughly 0.9k, by my estimate from hulls.ron and the tool's market prices, against about 10k of monthly state inflow. Building is capped by yard slots (1 per yard, 1-4 yards per community).
- The money side of "war costs feed back into economy and cohesion" therefore has no effect in any game state the tools can produce. The arrears rule, `morale_per_arrears` (battles.rs:277-286) and the README sentence on arrears are only reachable through fabricated test states (tests/military.rs:1134 and 2724-2725).
- Commit 9be40d9 itself says treasuries, bond yield and cohesion stayed "auf dem Stand von vorher".

**Where the finding overstates**
- "Four of six treasuries higher" is seed noise. The same order gives all six lower in seed 5 (-0.4k to -5.3k) and a mixed result in seed 7. The sound statement is "no systematic trace".
- "War makes treasuries grow" is the AI war chest from the earlier finance milestone (ai/plugin.rs:139, target 24 times revenue). The saves show targets of 42.1k and 35.8k in seed 5, with the treasuries sitting just above them. In scripted wars there is almost no fleet spending anyway: fleets are destroyed or stay abroad, and no AI rebuilds.
- War still feeds back through losses: war weariness (+20 per navy lost, about -5 cohesion), the share-price shock after a heavy defeat, and cohesion near 42 with secessions after decades of war. The requirement is met through losses, not through money.

**On the suggested fix**
Raising `upkeep_per_month` tenfold alone would still leave upkeep a relabelled transfer below the surplus (starting fleet about 1.6k against 9-12k). It only lowers the arrears threshold to a fleet about 6-7 times the start. Where the pay goes, or what the hand-out does, has to be part of the decision.

Conservation, determinism and the tests are unaffected. Nothing from memory was used.

**Belege der Gegenprüfung.** All runs used target/release/headless, rebuilt from the clean worktree at 4b64d65 (build exit 0). I did not run `cargo test`; the finding does not claim a failing test.

**Claimed scenario, reproduced exactly**
- `fleets --seed 3 --years 8 --order bastion:2,horst:1,speer:2,dolch:3 --polity 0`: treasury 17155, upkeep 730, fleet value 60874, arrears 0 %.
- Same call without `--order`: treasury 20061, upkeep 163, fleet value 13570.
- Treasury with orders against without, at years 1, 2, 4 and 6: 13.8k/14.9k, 15.1k/16.2k, 16.2k/18.6k, 17.2k/18.4k.

**Year-20 order (`--years 24 --order bastion:1,speer:2,dolch:4 --order-year 20`)**
- Seed 3 differences: -353, +2467, +2267, +1376, +2838, -23, as claimed.
- Seed 5: -2536, -462, -435, -487, -5337, -2537.
- Seed 7: +59, +557, +927, -1393, +2039, -1320.
- At `--years 21` the battleship is 82-86 % done with 6.7k-7.3k paid, so about 8.2k in total.

**Largest order (`--years 12 --order bastion:8`, seeds 3 and 5)**
- Fleet value 104786, upkeep 1257 a month, arrears 0 % for all 12 communities.
- Treasuries 13.3k-20.0k against 14.7k-22.4k without orders.

**Scripted war (`run --years 11 --war 10:0:1 --quiet`, against the same run without war)**

| Seed | Belligerent treasuries, war | Same communities, peace |
|---|---|---|
| 5 | 55611 and 44079 | 22262 and 17107 |
| 3 | 25220 and 28228 | 20116 and 16358 |
| 8 | 20275 and 24852 | 18628 and 20512 |

- Saves (`--save`) for seed 5 show `treasury_targets` of 42108 and 35763 for the belligerents, 0 in peace, and no bonds.
- Household money of the defender falls 41 % in seed 3 and 87 % in seed 4 (blockaded); the finding quotes 65 % for seed 4.

**Sweeps**
- `sweep --seeds 0..8 --years 60`: arrears 0.0 %, 6.0 ships per community, no battles.
- `sweep --seeds 0..8 --years 60 --war 10:0:1 --war 12:2:3 --war 14:4:5`: arrears 0.0 % in all 72 CSV rows.
- The eight matching `run` chronicles contain no "Sold schuldig" entry.

**Code**
- supply.rs:214-282: `monthly_upkeep` and `pay_upkeep`.
- economy/capital.rs:399-405 (hand-out), 412-432 (`pay_households`), 461-472 (minting).
- finance/plugin.rs:85-86: minting and hand-out run before the fleet upkeep in the same tick; fleet/plugin.rs:28-36 gives the order inside the fleet set.
- ai/plugin.rs:131-151: war chest.
- assets/data/balance.ron:217 (`upkeep_per_month: 0.012`), 245-246 (`ammo_goods`, `fuel_goods`), 231 (`repair_parts`), 94 (`state_reserve_per_million: 1.5`), 90-91 (`price_target`, `mint_per_month`).

Logs and saves are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_upkeep/

### anforderungen 5 · widerlegt · minor · The share-price shock after a heavy defeat cannot reach cohesion and is invisible against price noise; its test compares only against a run without the constant

Ort: `crates/sim/src/fleet/battles.rs:848`

**Beschreibung.** `shock_polity` multiplies the `shock` of every issuer of the loser by 0.9 (battles.rs:848-855, finance/shares.rs:473-488); it recovers by 12 % a month, so about 2 % are left after a year. Cohesion uses only value shares within a region and within the realm (finance/cohesion.rs:117-148 and 303-319); a uniform factor on all issuers of a community cancels out, apart from holdings in foreign issuers and bonds. So the path defeat -> share prices -> cohesion does not exist; only war weariness reaches cohesion. Against the market the shock is unmeasurable. Summed market value of the loser's firms relative to the same seed at peace, 1, 2 and 4 years after the defeat: seed 3 (home system blockaded afterwards) +40 %, +102 %, +93 %; seed 5: +19 %, -15 %, +101 %; seed 8: -12 %, -34 %, +66 %; seed 4 (blockaded), after 2 and 4 years: +2 %, +65 %. The peace runs themselves swing by a factor of two from year to year (seed 8: 216k, 449k, 199k). In seed 3 the blockade lifts the profit of the mining firm on the blockaded capital from -4 to +100 and +262 credits a day, because local raw materials become scarce, which outweighs the 0.53 risk discount and the shock many times. The test `losses_wear_a_community_down_and_a_heavy_defeat_moves_its_share_prices` (tests/military.rs:2382-2497) is honest about the mechanism, but its control run differs only in `defeat_shock = 1.0`; it would pass unchanged if a defeat made the loser's shares rise.

**Fehlerszenario.** README and report say a heavy defeat depresses the prices of all firms of the community; in `headless run --seed 3 --war 10:0:1` the loser's firms are worth 224k one year later against 161k in the run without the war.

**Vorschlag.** Either give the shock a consequence that matters (for example a lasting risk term while the enemy holds the home system, or an effect on bond yields) and test it against a no-war control over several seeds, or describe it as a short-lived display effect.

**Gegenprüfung.** Not a defect: the finding's numbers reproduce, but they describe a balance and design opinion, and the mechanism works as documented and tested.

1. **The claimed scenario is selective.** The 224k vs 161k figure for seed 3 covers only `IssuerId::Firm`. `shock_polity` and the README sentence cover all issuers of the community, including trading houses and Kontore. Over all of the loser's issuers the market value one year later is 426,056 with the war against 599,758 at peace (-29 %). The firm subtotal rises because of one mining firm on the blockaded capital (4,011 to 94,676). That is the blockade scarcity effect, not the shock, and it was already documented in milestone 3 (`crates/sim/tests/finance.rs:598-600`, present at d883061) and matches the spec "a blockade lets prices rise".

2. **The shock does depress prices when isolated.** I ran the same war with `defeat_shock: 1.0` from a copied data directory. At the end of the war year the loser's total market value is lower with the shock in all four seeds (3, 4, 5, 8). Later differences go in both directions; the simulation is chaotic over years.

3. **No requirement or documentation promises a path from share prices to cohesion.** README.md:226-228 names war weariness as the path to bond yields and unrest, and mentions the price shock separately. The weariness path exists: `battles.rs:845-846` raises it by 20 × share of navy lost, `cohesion.rs:213` and `:396` turn it into cohesion points, and `bonds.rs:37` into yields. In the runs the loser's cohesion falls well below the peace run. The uniform-factor property belongs to the pre-existing `shock_polity` from milestone 3, which is already used for `war_shock` in `command.rs:290`.

4. **The shock is not a display-only effect.** Prices feed capital increases (`shares.rs:107-108`), purchase cost (`shares.rs:164`), takeover bids (`shares.rs:274`) and investor ranking (`investors.rs:41-48`). In seed 3, ten months after the defeat, holder lists differ for 24 of the loser's 60 issuers and share counts for 22 between the shock and no-shock runs.

5. **The test is meaningful.** A control differing only in the constant is the right way to test one mechanism in a chaotic simulation. It checks the exact factor on exactly the loser's issuers, no change for anyone else, every stricken price at or below the control at the next monthly close, and total value below 97 %. It would fail if the shock raised prices, and it passes. The finding itself calls it "honest about the mechanism". A no-war control over several seeds would mostly measure blockade and risk effects.

6. **The suggested fixes largely exist already.** `risk_discount` (`shares.rs:412-423`) already applies a lasting term for blockade, hostile fleets and war (the 0.53 the finding cites), and defeats already raise bond yields through weariness. The size of the shock is a RON lever (`balance.ron:269-270`).

**Belege der Gegenprüfung.** **Code read**
- `crates/sim/src/fleet/battles.rs:836-857`: weariness and shock in `conclude`.
- `crates/sim/src/finance/shares.rs:95-111, 162-165, 274, 398-434, 438-488`: capital increases, purchase cost, takeover bid, `risk_discount`, `reprice`, `shock_polity`.
- `crates/sim/src/finance/cohesion.rs:105-150, 181-239, 299-407`: ownership shares, contributions, `update_cohesion`.
- `crates/sim/src/finance/investors.rs:36-57`, `crates/sim/src/finance/bonds.rs:37`, `crates/sim/src/command.rs:289-298`.
- `crates/game/src/ui/finance.rs:297`: the UI shows "Stimmung" when shock is below 0.97.
- `assets/data/balance.ron:265-270` and `:186` (cohesion weight 0.25 per weariness point).
- `README.md:162-164, 226-228`; `crates/sim/tests/military.rs:2288-2497`; `crates/sim/tests/finance.rs:546-622`.
- `git show d883061:crates/sim/src/finance/shares.rs` already contains `shock_polity` at line 473.

**Commands run**
- `cargo build --release -p headless`: exit 0.
- `cargo test -p sim --test military losses_wear_a_community_down`: 1 passed.
- `headless run --seed {3,4,5,8} --years {10,11,12,14} --quiet --save …` in three variants: peace, `--war 10:0:1`, and `--war 10:0:1` with `--data-dir` pointing at a scratch copy of `assets/data` where `defeat_shock` is 1.0. The repository is unchanged (`git status` clean). Saves were read with a small RON reader in the scratch directory.

**Loser's market value in seed 3 at year 11 (loser is polity 1, system 44 blockaded)**

| Issuer type | Peace | War | War without shock |
|---|---|---|---|
| Firm | 160,612 | 224,417 (+39.7 %) | 242,312 |
| House | 254,991 | 107,137 (-58 %) | 121,767 |
| Kontor | 184,155 | 94,502 (-49 %) | 97,419 |
| All | 599,758 | 426,056 (-29.0 %) | 461,499 |

- Largest single changes, peace to war: Handelshaus 254,991 to 107,137; Zeloth I Mining 4,011 to 94,676 (risk 0.53, mood 0.943); Kontor 44 117,871 to 55,121.
- The finding's firm-only percentages reproduce exactly: seed 3 +39.7 / +102.0 / +93.4 %; seed 5 +19.4 / -15.1 / +100.9 %; seed 8 -11.9 / -33.9 / +66.3 %; seed 4 +1.8 / +65.3 % (years 12 and 14).

**All issuers of the loser, war against peace**

| Seed | Year 11 | Year 12 | Year 14 |
|---|---|---|---|
| 3 | -29.0 % | -20.9 % | -17.4 % |
| 4 | -3.8 % | -53.1 % | -34.8 % |
| 5 | -5.4 % | -23.6 % | +26.1 % |
| 8 | -14.6 % | -26.5 % | +62.9 % |

**Shock against no shock, same war, all issuers of the loser**

| Seed | Year 11 | Year 12 | Year 14 |
|---|---|---|---|
| 3 | -7.68 % | +0.11 % | +20.71 % |
| 4 | -2.38 % | +12.93 % | -1.05 % |
| 5 | -5.53 % | -5.82 % | +2.42 % |
| 8 | -5.28 % | -21.04 % | +81.24 % |

**Cohesion and ownership**
- Loser's population-weighted cohesion, peace against war, at years 11 / 12 / 14: seed 3 73.75 / 72.62 / 70.22 against 67.93 / 60.92 / 53.18; seed 5 76.33 / 75.02 / 73.05 against 74.13 / 69.47 / 63.43.
- Loser's weariness at year 11 is 27.2: 20 from losing the whole navy plus 12 × 0.6 for the months at war.
- Seed 3, year 11, shock against no shock: holder lists differ for 24 of 60 loser issuers, share counts for 22, and shares in foreign hands are 234,102 against 246,633.

Everything is in `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/vs/`; the result tables are `analyse_out.txt`, `breakdown_out.txt`, `firms_out.txt` and `holdings_out.txt`, and the scripts are `ron.py` and `analyse.py`.

### anforderungen 6 · bestätigt · minor · War weariness and the heavy-defeat threshold are scaled by what is left of the navy, so the last corvette weighs as much as the first whole fleet

Ort: `crates/sim/src/fleet/battles.rs:843`

**Beschreibung.** `share = lost / navy`, where `navy` is the build value of the owner's ships in existing fleets just before the write-back (battles.rs:735-745, 839-847). The rule works as stated in the runs: the loser of seed 3 (100 % lost) gains exactly 20, the winner (9194 of 13570 lost) 13.6; afterwards weariness grows by 0.6 a month, and cohesion of unblockaded belligerents is about 2, 5, 9 and 16 points below the peace run after 1, 2, 4 and 8 years of unbroken war (blockaded loser: 12 points after two years). With peace after five years weariness is back at 0 seven years later and cohesion within one point of the peace run. But the measure has no absolute scale. A community that has lost its fleet and sends out replacement ships one at a time takes +20 weariness (5 cohesion points, 1.6 points of bond yield, as much as 33 months of war) and, since `share >= heavy_defeat_share` always holds, a heavy-defeat share shock for each single corvette of 1646 credits, the same as for the original 13570-credit fleet. Five such losses reach the cap of 100; the same five ships lost together in one fleet cost +20. Conversely a navy ten times the starting size loses a complete starting fleet for +2 and no shock. The implementers' note that `weariness_per_navy_lost` needs retuning with the AI does not mention this.

**Fehlerszenario.** A community without a fleet builds one Dolch, loses it against a blockader: weariness +20, chronicle entry about the defeat shaking confidence, shares times 0.9. Four more times and weariness is 100.

**Vorschlag.** Measure the loss against a stable reference, for example `lost / max(navy, value of military.starting_fleet)` or a scale from population or revenue, and require a minimum absolute loss for the heavy-defeat shock.

**Gegenprüfung.** I tried to refute this and could not: the mechanism is exactly as described, and I reproduced the single-corvette case with the existing headless tool. It is a minor, design-level flaw in the rule, not an implementation slip.

**Code.** In `/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/sim/src/fleet/battles.rs`:
- Lines 735-745: `navy` is the material value of the owner's ships in fleets that are alive at that moment, taken just before the write-back.
- Line 843: `share = (owner.lost / owner.navy).clamp(0.0, 1.0)`.
- Lines 845-846: weariness rises by `weariness_per_navy_lost * share` (20 per full navy).
- Lines 848-856: the price shock (factor 0.9) fires when `!owner.held && share >= heavy_defeat_share` (0.25).
- There is no floor on the denominator and no absolute term anywhere, so a one-ship navy that is destroyed always gives share = 1.0.

**Not purely cosmetic.** Weariness feeds bond yields (0.0008 per point), cohesion (0.25 per point) and crew morale (0.3 per point). The normal wartime rise is only 0.6 a month, so +20 equals 33 months of war.

**Not handled elsewhere.** The yard guard in `yard.rs:314-320` keeps a finished ship in the dock while an enemy fleet sits in that system and no own fleet is there. That blocks the literal path "new Dolch launches into the blockader at home". It does not cover ships built at a colony yard, ships launched before the enemy arrives, or battle remnants; all three reach the same state.

**Reproduced.** On seed 2, Kommune loses 5 of 6 ships in the first battle (11924 of 13570 credits) and gets +17.57 weariness and no shock. Its last corvette (1646 credits) is then destroyed and it gets exactly +20.00 plus the heavy-defeat chronicle entry and price shock. The same fleet lost in one battle (Orden, same run) costs +20; lost in two it costs +37.57, even though Kommune won the first battle. This contradicts the lever's own comment in `balance.ron:263-265` ("Kleinere Verluste zählen anteilig") and the meaning of "schwere Niederlage".

**Limits on severity.**
- The behaviour is documented and tested for full-size navies (`README.md:226-228`, `crates/sim/tests/military.rs:2382-2434`); no test covers a small navy.
- The effect is bounded by the cap of 100 and by peacetime recovery.
- No AI builds or moves warships on this branch, so the build-one-at-a-time variant needs player or script commands. The remnant variant needs only a second battle.

**Why it still needs a code change.** No existing RON lever can fix it, because none supplies an absolute scale. The suggested fix is sound: floor the denominator at the starting-fleet value, which `ship_loader.rs:447` already uses to calibrate strength, and require a minimum absolute loss for the shock. The existing test would stay green, since both navies there are at least the starting value.

Nothing from memory was used.

**Belege der Gegenprüfung.** All runs use the existing release binary from `/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim` (`cargo build --release -p headless`, exit 0). I did not run `cargo test -p sim`; the test evidence below is from reading the test.

**1. Seed 2, single corvette**
- Commands: `target/release/headless run --seed 2 --years 6 --war 2:0:1 --save <file>` and the same with `--war 4:2:1` added.
- Battle 1 (Pelios, 19.03.-30.04.2402): "1. Flotte (Kommune) 5 von 6 Schiffen verloren (88 % des Bauwerts: 1 Kreuzer, 2 Fregatten, 2 Korvetten), hält das Feld".
- Battle 2 (02.-16.04.2404): "Bei Pelios beginnt ein Gefecht: 1. Flotte (Kommune) mit einem Schiff gegen 1. Flotte (Räte) mit 6 Schiffen." Then "1. Flotte (Kommune) verliert 1 Korvette und besteht nicht mehr." Then "Die Niederlage bei Pelios erschüttert das Vertrauen in Sonnwend-Kommune: Die Kurse geben nach."
- Saved `war_weariness` of Kommune: 46.37406039943894 without the second war, 66.37406039943892 with it. The difference is exactly 20.0 for one 1646-credit corvette.
- Battle 1 contributed 46.374 − 48 × 0.6 = 17.57 for 11924 credits (11924 / 13570 = 0.8787), with no shock.
- Orden lost its whole fleet in one battle: 48.8 = 28.8 + 20.

**2. Seed 3, single cruiser remnant**
- Commands: `run --seed 3 --years 5 --quiet --war 2:0:1 --save <file>` and the same with `--war 4:0:2` added.
- Karawane `war_weariness`: 35.15047913860658 versus 55.15047913860658. Its last cruiser (4376 credits) costs +20.0; the first 9194 credits cost +13.55.
- Chronicle (from the same two-war scenario run without `--quiet`): "Die Niederlage bei Lumheim erschüttert das Vertrauen in Karawane von Meridian: Die Kurse geben nach."

**3. Ship values**
- `target/release/headless fleets --seed 3 --years 4 --order dolch:1 --polity 1 --ships` shows 7 ships with Bauwert 15216, against 13570 for the starting fleet, so a Dolch is 1646.
- The corvette hull has `build_days: 60` in `assets/data/hulls.ron`.

**4. Code and data references** (paths relative to the worktree)
- `crates/sim/src/fleet/battles.rs:735-745` (navy tally), `:839-857` (share, weariness, shock).
- `crates/sim/src/fleet/mod.rs:173-179` (`material_value` ignores condition), `:334-338` (`Fleets::of` counts alive fleets only).
- `crates/sim/src/fleet/yard.rs:302-320` (ship held in dock while the orbit is contested).
- `crates/sim/src/finance/plugin.rs:129-133` (+0.6 a month at war, −1.0 at peace).
- `crates/sim/src/finance/shares.rs:473-487` (`shock_polity`, floor 0.2).
- `assets/data/balance.ron:148` (`yield_per_weariness` 0.0008), `:186` (cohesion weariness 0.25), `:263-270` (`weariness_per_navy_lost` 20, `heavy_defeat_share` 0.25, `defeat_shock` 0.9).
- `crates/sim/tests/military.rs:2382-2434` (the only test of this rule; both navies are at least the starting fleet).

Logs and saves are in `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/`: `s2_one_war.out`, `s2_one_war.ron`, `s2_two_wars.out`, `s2_two_wars.ron`, `s3_one_war.ron`, `s3_two_wars.ron`, `fleets_s3.log`.

### anforderungen 7 · bestätigt · minor · A yard on a world inside a system held by another community's majority builds ships it can never fuel, arm or repair

Ort: `crates/sim/src/fleet/supply.rs:57`

**Beschreibung.** Whether a world is a yard depends on the owner of the planet (yard.rs:63-75), and its orders buy parts on the system market without further condition. Resupply and repair, however, require `galaxy.system_owner(system) == fleet.owner` (supply.rs:57 and 156), and the system owner is the community with the largest population in the system. Shared systems are common (7 of 47 settled systems in seed 3, 15 of 46 in seed 5) and contain yards of the minority. Seed 3, year 20: Zelune II (Syndikat, 258 M, arms capital 0.45) in the home system of the Kommune (3162 M), Nyiys II (Karawane) in a system the Liga holds, Quiroth III (Liga) in a system of the Kommune; seed 5, year 18: three more. A ship built there is delivered with ammunition 0 and fuel 0 (`Ship::from_yard`) into a system where it will never receive either, and it cannot be repaired at the yard that built it; it has to crawl at `dry_speed` to a system its community holds. The README promises supply in the own system and repair in the own system with a yard; for the player the yard world is own territory.

**Fehlerszenario.** `BuildShip` at Zelune II is accepted and paid for; the finished corvette stays at 0 % fuel and ammunition for as long as it lies there and enters any battle there with empty magazines.

**Vorschlag.** Use the same notion of own territory on both sides: allow resupply and repair wherever the community owns a planet with a yard in the system, or refuse orders at yards in systems the community does not hold. Add a test with a shared system.

**Gegenprüfung.** The finding is correct, and I could not refute it. It is a minor but real logic gap: the yard and the supply code disagree about what counts as a community's own territory.

**What the code does**
- `yard_slots` (yard.rs:53-75) and `order_ship` (yard.rs:103-151) only look at who owns the planet. `SimCommand::BuildShip` calls `order_ship` directly (command.rs:414-431) with no further check.
- `advance_yards` buys the parts on the market of the planet's system and delivers the ship into that system via `Ship::from_yard`, with ammunition 0 and fuel 0 (mod.rs:103-109).
- The only place that raises ammunition or fuel is `resupply_fleets` (supply.rs:83-84). It skips every fleet where `galaxy.system_owner(system) != Some(owner)` (supply.rs:57). `repair_fleets` has the same condition (supply.rs:156).
- `system_owner` is the community with the largest population in the system (galaxy/mod.rs:216-230).

So a ship built on an own world in a system where another community has more people gets no fuel, no ammunition and no repair there, although the yard bought its parts from that same market. Nothing else handles this case.

**The preconditions are common**
- Seed 3 at year 20 has 7 shared systems out of 47 settled; seed 5 at year 18 has 15 of 46. Both match the finding.
- Zelune II (Syndikat, 258 M) lies in the Kommune's home system (3162 M) and is listed by `headless fleets` as a Syndikat yard with one slot.
- Nyiys II (Karawane) and Quiroth III (Liga) are minority yards in the same way.

**Why it matters**
- A ship with empty magazines inflicts no losses in the battle calculator.
- From Zelune the Syndikat's nearest held system is three lanes away, about 43 days at dry speed, before resupply can even start.
- A community whose only world is a minority world (a free state after secession in a shared system) has nowhere to go: its navy stays unarmed for good.
- The comment on `from_yard` says the ship takes on supplies at the market "like any other"; that does not hold here.
- No existing test covers a shared system.

**Limits of what I confirmed**
- The exact scenario (BuildShip at Zelune II) is confirmed by reading only. The headless tool orders ships only at capitals, and no AI or UI issues BuildShip on this branch.
- For the same reason, sweeps and hashes are unaffected today; the gap is in the command interface that the player, scripts and the planned military AI use.
- The run that shows the failure used a scratch copy of the balance data with the secession thresholds changed, so that free states with a capital yard on a minority world appear early. The fleet code path is the same; the repository was not touched.
- Nothing from memory was used; the memory index was empty.

**Belege der Gegenprüfung.** **Code** (all under /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/sim/src/)
- fleet/supply.rs:57 and :156: `|| galaxy.system_owner(system) != Some(owner)` skips resupply and repair.
- fleet/yard.rs:63-75 (`yard_slots`: planet owner plus arms capital or capital world), :118-129 (`order_ship`: only `world.owner` and `yard_slots`), :210-258 (parts bought on `economy.markets[world.system]`), :324-327 (delivery into the planet's system with `Ship::from_yard`).
- fleet/mod.rs:103-109: `from_yard` sets ammo 0.0 and fuel 0.0.
- galaxy/mod.rs:216-230: `system_owner` is the largest population in the system.
- A search for writes to `.ammo` and `.fuel` finds only supply.rs:83-84 as a refill.

**Preconditions, default data**
- `target/release/headless fleets --seed 3 --years 20` lists "Werft Zelune II 1 Bauplatz (Rüstungskapital 0.5) Markt Zelune" under Irrlicht-Syndikat. Zelune is the Kommune's home system (Werft Zelune I, 1. Flotte (Kommune) in Zelune). Nyiys II (Karawane) sits beside Nyiys I (Liga), Quiroth III (Liga) beside Quiroth II (Kommune).
- Save from `headless run --seed 3 --years 20 --quiet --save`, parsed with a script: 47 settled systems, 7 shared; Zelune has Kommune 3161.9 M against Syndikat 258.4 M. Seed 5 at year 18: 15 of 46 shared. Seeds 0-11 at year 18: 3 to 15 shared systems each.
- In seed 3 the nearest Syndikat-held system from Zelune is Merethia via Ortor and Bryheim: 365 units, 43 days at dry speed (22 × 0.4 per day).

**Run showing the failure** (scratch copy of assets/data; only cohesion thresholds changed to force early secessions)
- Command: `target/release/headless --data-dir <scratch>/data_secede fleets --seed 5 --years {20,30,40} --order dolch:1 --order-year 8 --ships`.
- Freistaat Galdor II, Hesoth II and Ashanheim III each hold one corvette at "Munition 0 % Treibstoff 0 %" at years 20, 30 and 40. Their capital is a minority world: Galdor has Kommune 3012 M against 240 M, Hesoth has Vesperbund 2853 M against 235 M, Ashanheim has Kommune 430 M against 198 M.
- They have 600-1900 credits in the treasury, pay 20 per month upkeep, and the Galdor market holds 773 fuel.
- Control in the same run: Freistaat Soldor II and Yrren II, which hold the majority in their systems, show 100 % / 100 %.
- Seeds 0, 1 and 4 show the same pattern (Peltor I, Draildor II, Kaloror III, Peluis I, Vesathune II, Vorund II, Galara II at 0 % / 0 %).

**Effect of empty magazines**
- `headless battle --a dolch:3 --b dolch:3` gives 33 % losses on both sides; with `--ammo-a 0`, side A loses 33 % and side B 0 %.

**Tests**
- `cargo test -p sim --lib fleet::` passes (41 tests). `no_resupply_abroad_under_blockade_or_in_battle` only covers another community's home system. Nothing in supply.rs, yard.rs or tests/military.rs covers a shared system; the helper `largest_colony_system` in tests/military.rs explicitly picks systems the community holds.

Logs and scripts are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/v_minyard/ (shared.py, data_secede/, secede_fleets_s*.log, seed3_y20.ron). `git status` in the worktree is clean.

### anforderungen 8 · bestätigt · minor · The headless tools cannot combine build orders with wars, so no run exercises losses, rebuilding, goods and treasury together

Ort: `crates/headless/src/main.rs:47`

**Beschreibung.** `--war` and `--peace` exist on `run`, `sweep` and `hash` (main.rs:47, 67, 86); `--order` exists only on `fleets` (crates/headless/src/fleets.rs:27), which takes no wars and cannot save. The war script itself issues only DeclareWar, SetStance, MoveFleet and MakePeace (crates/sim/src/harness.rs:478-509). In every scripted war the two starting fleets meet once (9 to 11 of 12 ships lost in each of the 8 seeds of the sweep); after that nobody builds, and the attacker's survivor never goes home. Hull segments stay at or near the price floor in the war states I inspected. A defender that wins at home is back at 100 % hull and ammunition within seven months from the ordinary home market (seeds 5 and 8), which is the one supply loop the war runs do show. The loops this branch is about (a blockaded yard standing still, replacement purchases moving prices, war-time orders against the treasury) appear in no sweep and no checksum; they are covered only by integration tests that inject the goods. To see prices or share values of a war run at all I had to write a reader for the RON save (/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/tools/ron.py).

**Fehlerszenario.** A balance change that makes war-time rebuilding unaffordable, or that leaves yards permanently stalled, passes `sweep --war ...` unchanged, because the sweep contains no orders.

**Vorschlag.** Add `--order` (with a choice of world, not only capitals) to `run`, `sweep` and `hash`, or let the war script rebuild lost ships; add `--war` and `--save` to `fleets`; print per community the prices of the military goods and the military spending in the run summary.

**Gegenprüfung.** Verdict: the finding is accurate and I could not refute it. It is a verification/tooling gap of minor severity, not a defect in the simulation itself.

What I checked (repository untouched, git status clean at 4b64d65):

1. The CLI facts hold. `--war`/`--peace` exist only on `run`, `sweep`, `hash` (crates/headless/src/main.rs:47, 67, 86). `--order` exists only on `fleets` (crates/headless/src/fleets.rs:27), which has no `--war` and no `--save`. Headless has no way to load a save. `WarScript::issue` (crates/sim/src/harness.rs:478-509) issues only DeclareWar, SetStance, MoveFleet, MakePeace. `SimCommand::BuildShip` is issued nowhere except in tests and in `fleets --order`; no AI builds ships.

2. The claimed failure scenario reproduces with the existing tool. I ran `hash --seed 3 --days 7200 --war 10:0:1` once with the stock data and once with `--data-dir` pointing at a copy where yard_fee is 5.0 instead of 0.15, yard_queue 1 instead of 8, and yard_capital_per_slot 400 instead of 4. All five checksums are bit-identical. The yard code never runs in any `run`/`sweep`/`hash`, with or without a war.

3. It is not handled elsewhere. In a private scratch copy of the sources, all 29 tests of crates/sim/tests/military.rs still pass with yard_fee 5.0. Every integration test that orders a ship injects the parts first (set_stock at lines 371, 468, 550-583, 672, 1338, 1431, 1562); the post-battle resupply test injects ammunition and hull segments (line 2510). No test or tool orders a ship after battle losses.

4. The war runs behave as described. `sweep --seeds 0..8 --years 30 --war 10:0:1` gives exactly one battle per seed with 9 to 11 ships lost. In seed 3 the attacker's single surviving corvette sits in the enemy capital for 20 years and the defender has 0 ships. Hull segments sit at the price floor (35 = 0.25 x 140) in the home markets.

Why it matters and is not cosmetic: I ran the missing combination from the scratch copy (scripted war plus BuildShip orders, outside the repository). It shows behaviour no existing run can show:
- With war 10:0:1, war 12:2:1 and peace 20:0:1, and every community ordering a replacement fleet at its capital in year 11, the attacked community in 8 of 8 seeds still has a finished ship in its capital dock at year 30. That ship occupies the only build slot, and the 2 to 5 orders behind it are at 0 %. This is the documented dock rule (crates/sim/src/fleet/yard.rs:314-320) meeting a blockade that has no strength threshold; in seed 3 one corvette of strength 5.5 is enough.
- Replacement purchases do move prices (hull segments 35 to 84 at the attacker's home in seed 3), and rebuilding at an unblockaded yard is affordable.
- A 500 % yard fee still lets both sides rebuild within about two years, so the real effect of that lever can only be judged in a combined run.

Points in the other direction, so the severity stays minor:
- Each mechanism has unit and integration tests, including blockade plus build order (military.rs:644) and battle, return home, resupply and repair (military.rs:2499).
- `fleets --order` shows that in peace the unmodified economy delivers a cruiser and two corvettes to all six communities within two years.
- In my combined runs (8 seeds, 30 years) the invariants and money_drift held every year, skipping idle ticks matched ticking every step, and a save in the middle of a battle with 12 open orders continued identically. The gap hides no determinism or money bug that I could find.
- README marks the war script as a stopgap until the military AI exists, which will put orders into every run.

No facts from memory were used; the memory index was empty.

**Belege der Gegenprüfung.** Paths in the repository (/home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim):
- crates/headless/src/main.rs:47, 67, 86 (--war on run, sweep, hash)
- crates/headless/src/fleets.rs:19-37 (FleetsArgs: --order, --order-year; no --war, no --save), 78-97 (orders only at capitals)
- crates/sim/src/harness.rs:478-509 (WarScript::issue)
- crates/sim/src/fleet/yard.rs:314-320 (finished ship stays in dock and keeps its slot while an enemy fleet is docked)
- crates/sim/src/fleet/movement.rs:79-98 (blockade without strength threshold)
- crates/sim/tests/military.rs:371, 468, 550-583, 672, 1338, 1431, 1562, 2510 (injected stock)

Runs with the existing binary (logs in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_order_war):
- hash_default.txt and hash_yard.txt: `hash --seed 3 --days 7200 --every 1800 --war 10:0:1`, stock data versus yard_fee 5.0 / yard_queue 1 / yard_capital_per_slot 400. Identical: 8d6f1d44a689077b, a48055d8026186d5, 76e025be1ef97221, 24d5279d4a052f3b, 7f6e054c280c2d7a.
- sweep_war.txt: `sweep --seeds 0..8 --years 30 --war 10:0:1`. One battle per seed; losses 9, 9, 11, 11, 11, 10, 9, 10.
- run_s3_war.txt: seed 3 after 30 years. Attacker 1 ship (strength 5), defender 0 ships, both at war weariness 100, no build orders.
- fleets_s3_o10_y12.txt: `fleets --seed 3 --years 12 --order speer:1,dolch:2 --order-year 10`. All six communities at 9 ships in peace.

Runs from a private copy of the sources (same directory, ws/crates/sim/tests/combo.rs is the exploratory driver; not part of the repository):
- military_fee5.log: 29 of 29 military integration tests pass with yard_fee 5.0.
- combo_capital.log: seed 3, war in year 10, both sides order a starting fleet at the capital in year 11. Attacker has 6 new ships by August 2412. Defender's order #6 (Speer) is at 100 %, 3840 credits paid, and still in the dock on 01.01.2420; orders #7 to #11 are at 0 %; Zeloth is blockaded by one corvette of strength 5.5.
- combo_colony.log: same, ordered at a colony yard. Both sides have 6 new ships by 2414.
- combo_sweep.log: 8 seeds, 30 years, wars 10:0:1 and 12:2:1, peace 20:0:1, orders in years 11 and 14. No invariant violation. In every seed community 1 ends with a finished ship in the capital dock and 2 to 5 orders at 0 % (for example seed 3: "#6 Speer bei Zeloth I 100% seit Tag 3960", #7 to #11 at 0 %). Skipping versus every tick identical for seeds 2 and 5.
- combo_determinism.log: orders placed during the battle. Yearly checksums identical between skipping and every tick over 13 years; save and load with 1 running battle and 12 open orders continues identically.

### anforderungen 9 · widerlegt · minor · API gaps for the fleet interface: no reason for a stalled order, no supply or repair predicates, no record of military spending, no way to scrap ships or direct new ones

Ort: `crates/sim/src/fleet/yard.rs:37`

**Beschreibung.** The public API is enough to list fleets and ships (`Fleets.list`, `Fleet.ships`, `supply`, `day_range`), show queues (`orders_at`, `yard_slots`, `BuildOrder`), mark and show battles (`Battles.active`, `in_system`, `of_fleet`, the public `Battle` fields, `reports`, `debris`, `pauses`) and send every order (`BuildShip`, `CancelBuild`, `MergeFleets`, `SplitFleet`, `SetFleetOrders`, `Retreat`). Every fleet name the game crate uses today still exists (checked by grep; the game crate was not compiled). Gaps: (1) `BuildOrder.stalled` is a bool (yard.rs:35-37); whether goods or money are missing, and which good, must be recomputed by the UI, and `headless fleets` itself can only print that parts or money are missing. (2) 'Finished, held in the dock' must be inferred from `progress >= 1 - 1e-6` (crates/headless/src/fleets.rs:45 copies the epsilon), and an order at a world without slots looks like one waiting its turn. (3) The conditions for resupply and repair are inline in supply.rs:54-58 and 153-166; there is no public answer to 'can this fleet be supplied or repaired here, and if not, why', so the UI must duplicate the checks for system ownership, blockade, yard and battle. (4) Nothing records what the fleet costs: upkeep paid, resupply, repair and yard purchases leave only `BuildOrder.spent` per open order and `monthly_upkeep` as the amount due. (5) No command scraps a ship or disbands a fleet, so upkeep and arrears can only be reduced by losing ships. (6) A finished ship joins the lowest-numbered own fleet lying in the system (yard.rs:303-313) with empty tanks; `Fleet::min_fuel` then limits the whole fleet to `dry_speed` (fleet/mod.rs:260-262) until the ship has refuelled at 5 % a day, and no command chooses the receiving fleet.

**Fehlerszenario.** The build panel shows a stalled order without being able to say what is missing; the fleet panel cannot explain why a fleet is not being resupplied; a player who ordered too many ships cannot cut upkeep; a fleet sent off on the day a new corvette joined crawls at 40 % speed without explanation.

**Vorschlag.** Add a stall reason to `BuildOrder` (waiting for a slot, no yard, missing good with its id, no money, held in the dock), public predicates for resupply and repair that return a reason, monthly military spending per community in `Fleets` or the finance state, and commands to scrap ships and to set the fleet a yard delivers to.

**Gegenprüfung.** Not a defect: the six observations are factually correct, but none of them is a wrong result, a rule or spec violation, or a broken API. They are ergonomics wishes and two feature requests for the fleet UI, which the README still lists as open for milestone 4.

Checked against the four claimed failure scenarios:

1. **"The build panel cannot say what is missing."** Overstated. `stalled` is only a bool, but the reason can be rebuilt from public state: `Market::available`, `Market::ask`, `Economy::treasury`, the design's `parts` and `build_days`, `yard_fee`. `headless fleets` already derives all four order states from the public API alone and prints the parts market beside each yard.

2. **"The fleet panel cannot explain missing resupply."** Overstated. Every condition in `resupply_fleets` and `repair_fleets` uses public items: `Fleet::is_docked`, `in_battle`, `Fleets::is_blockaded`, `Galaxy::system_owner`, `yard_slots`, `Ship::is_damaged`. The UI would duplicate four checks; that is inconvenient, not impossible.

3. **"A player cannot cut upkeep."** True for finished ships: the only code that removes ships is the battle write-back, and there is no scrap command. But scrapping is not in the stated spec or the README command list, and open orders can be cancelled. At the current balance it does not bite: upkeep was 255 a month against treasuries of 17–20k, and 583 a month with eight extra cruisers against 20–32k, with 0 % arrears in both runs. The existing arrears test has to add 2000 cruisers to make a fleet unaffordable.

4. **"A fleet crawls at 40 % without explanation after a corvette joins."** The behaviour is real but intended. It is documented in the README, stated in the `Ship::from_yard` doc comment, and pinned by passing tests. The explanation is available through public `Fleet::min_fuel`, `Fleet::day_range` and per-ship `fuel`. The player can detach the new ship with `SplitFleet` or regroup with `MergeFleets`.

The missing record of military spending is also true, but the code base has no categorised state ledger for any spending, so this is a new feature rather than a regression. War costs still feed back mechanically through market purchases, pay to households and war weariness.

The suggested fix (stall-reason enum, spending ledger, two new commands) adds state and commands and raises design questions, such as whether scrapping refunds physical goods. That is a decision for the UI and military-AI milestone, not a review fix.

**One sub-point worth keeping as a low-priority note:** an order at a world whose yard has vanished is silently stuck. With zero slots, `advance_yards` sets `stalled = false` and skips the order, so it never progresses and never lapses, and `headless fleets` does not list it because it skips worlds without slots. Colony yards do vanish in ordinary runs: arms capital below 0.05 is set to 0, and in seed 3 the world Cyma III has a yard at years 5 and 20 but none at 10 and 30. No shipped front end can create this state today, since headless orders only at capitals and the game has no build panel; it is reachable through `SimCommand::BuildShip` at a colony.

I did not compile the game crate; the claim that its fleet names still exist was confirmed by grep only. Nothing from memory was used.

**Belege der Gegenprüfung.** All paths under /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim.

**Code read**
- crates/sim/src/fleet/yard.rs:35-37 — `stalled: bool`.
- crates/sim/src/fleet/yard.rs:196-202 — `if *used >= slots { order.stalled = false; continue; }`, which also covers slots == 0.
- crates/sim/src/fleet/yard.rs:303-320 — host fleet selection and the held-in-dock branch.
- crates/sim/src/fleet/supply.rs:54-58 and 153-166 — resupply and repair conditions, all on public items.
- crates/sim/src/fleet/mod.rs:103-109 — `Ship::from_yard`, empty magazines and tanks by design.
- crates/sim/src/fleet/mod.rs:235, 260-276, 324 — public `is_docked`, `min_fuel`, `day_range`, `is_blockaded`.
- crates/sim/src/economy/mod.rs:169, 175, 299 — public `Market::available`, `Market::ask`, `Economy::treasury`.
- crates/sim/src/galaxy/mod.rs:216 — public `system_owner`.
- crates/sim/src/command.rs:26-117 — no scrap or disband command; `CancelBuild`, `MergeFleets`, `SplitFleet` exist.
- crates/sim/src/fleet/battles.rs:679 — the only non-test code that removes ships from a fleet.
- crates/sim/src/economy/capital.rs:85-88 — `if *capital < 0.05 { *capital = 0.0; }`.
- crates/headless/src/fleets.rs:44-54 — `order_state` derives all four states from the public API, including the copied `1.0 - 1e-6`.
- crates/headless/src/fleets.rs:150-154 — worlds with zero slots are skipped, so their orders are not listed.
- crates/game/src/ui/fleets.rs and crates/game/src/traffic.rs — every `Fleet`/`Fleets` name they use still exists (grep only).
- README.md:192, 195 — new ships arrive empty; an empty tank slows the fleet.
- README.md:200 — command list without scrapping.
- README.md:241 — milestone 4 UI listed as open.

**Tests pinning the intended behaviour**
- crates/sim/tests/military.rs:435-439, 450-452 — new ship joins the home fleet with ammo 0 and fuel 0 and refuels the next day.
- crates/sim/src/fleet/mod.rs:866-870 and crates/sim/src/fleet/movement.rs:174 — one empty tank throttles the whole fleet.
- crates/sim/tests/military.rs:1134 — 2000 cruisers needed for an unaffordable fleet.

**Commands run**
- `cargo build --release -p headless` — exit 0.
- `cargo test -p sim --lib fleet::` — 41 passed, 0 failed.
- `cargo test -p sim --test military -- a_shipyard_turns a_fleet_without_fuel a_yard_stands_still build_orders_are_validated fleets_are_split_merged an_unaffordable_fleet` — 6 passed, 0 failed.
- `target/release/headless fleets --seed 3 --years 10 --order speer:1,dolch:2 --ships` — 9 ships per community, upkeep 255 a month, treasuries 17–20k, arrears 0 %.
- `target/release/headless fleets --seed 3 --years 30 --order speer:8` — 14 ships per community, upkeep 583 a month, treasuries 20–32k, arrears 0 %.
- `target/release/headless fleets --seed 3 --years {5,10,20,30} --polity 0` — yard worlds change over time; Cyma III is listed at years 5 and 20 and missing at 10 and 30; Kaleia I and Navenax I disappear by year 20.

Logs are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim, all prefixed `apigap_`.

### anforderungen 10 · bestätigt · minor · Tests: the economic links are untested or tested on fabricated states

Ort: `crates/sim/tests/military.rs:93`

**Beschreibung.** The mechanics are tested carefully: exact moves of money and goods against a control game, a shadow battle for the write-back, a save in mid-battle, skipping against every tick. What the tests do not establish: (1) Every yard, resupply and repair test injects the goods with `set_stock` (military.rs:93-98; 12 to 300 units per part, where natural stocks of military goods are 1-9 units). No test runs an order or a repair through an unmanipulated economy, except the determinism script, which asserts only that some ship was finished in two years (military.rs:1520). (2) No test asserts a price or supply reaction to military demand; the chain named in the comment at yard.rs:230-231 is checked only as demand > 0. It does work: with the 8-ship programme of seed 3, drives rise from 106 to 250 and weapon systems from 125 to 234 in year 2, and arms capital on the capital from 1.7 to 2.5. (3) Fiscal weight is tested for one month start (military.rs:1079-1120); arrears and the breaking fleet only on hand-made states (military.rs:1134, 2724-2725). (4) No test connects losses to cohesion or bond yields; the control of the share-price test differs only in the shock constant. (5) 'No resupply under blockade' is unit-tested by writing `fleets.blockaded = vec![home]` with the home fleet docked there (fleet/supply.rs:413-417), a state `update_blockades` never produces, because a docked owner fleet that is not regrouping prevents the blockade (movement.rs:85-87); in the game the rule can apply only to a regrouping fleet. (6) Not tested at all: a yard that ceases to exist with open orders, a yard in a shared system, a march order that cancels the withdrawal of a regrouping fleet, replay of commands given during a battle (the replay check covers build commands, military.rs:1544-1552). (7) The adapted finance test now requires `high > low + 10` per game and `+ 20` only for the mean of three seeds (tests/finance.rs:238 and 244), while the commit message says the threshold is unchanged. Verified by running: `cargo test -p sim` passes (83 unit tests, 6 + 9 + 29 integration tests); `hash --seed 3 --days 3600` is identical twice and with `--exact`; `hash --days 1440 --war 1:0:1 --war 1:2:1 --peace 3:0:1` is identical with and without `--exact`; four sweeps (8 seeds x 60 years in peace, with a five-year war and with three endless wars; 12 seeds x 40 years with twelve overlapping wars) end without invariant violation and with arrears 0.0 %. The peace sweep shows supply 0.99, utilisation 0.75, jobs filled 0.68, price level 0.64 and six communities alive in every seed. Not verified: the claim that 14 mutations are caught (needs code edits) and the comparison with sweep values from before the branch (needs a build of the base commit). Nothing in this report comes from stored memory notes; there were none.

**Fehlerszenario.** A change that stops yard demand from reaching prices, or that leaves natural stocks too thin for any repair, keeps all 29 integration tests green, because each test brings its own goods.

**Vorschlag.** Add scenario tests on unmanipulated games: an order at a capital in year 20 finishes on time and moves the part prices; a war with a rebuilding programme lowers treasury or transfers against a control; a blockaded import-dependent yard stalls without injected stock; plus the missing cases under (6).

**Gegenprüfung.** Confirmed as a minor but real test gap. Nothing is broken today, and two parts of the finding are overstated.

**What I confirmed (mutations on a scratch copy, never in the repository)**
- **(1) Injected goods.** Every yard and repair test, and the munition part of the resupply tests, brings its own goods (`set_stock`, military.rs:93-98; +80 at the home market at military.rs:672). With all arms plants switched off, so that no hull segment, weapon, drive or round is ever produced, all 29 military.rs tests and all 83 unit tests still pass. The same holds with arms plants capped at 10 %.
- **Determinism script.** The ship assertion at military.rs:1518-1521 is met from the 12 injected units alone (a Dolch needs 5.0 / 2.9 / 0.8), despite the comment at military.rs:1430 that the economy must deliver afterwards.
- **(2) Price reaction.** Reporting yard demand at one thousandth of its size keeps all 127 tests green. Only "demand > 0" is pinned.
- **(4) Cohesion and bond yields.** Removing the war-weariness term from cohesion (finance/cohesion.rs:213) or from the bond yield (finance/bonds.rs:37) keeps all 127 tests green. The spec for this part and the README section "Gefechte – Folgen" both promise this feedback.
- **(3) and (6), by reading.** Both are accurate descriptions:
  - An order at a yard whose slots drop to 0 waits forever with `stalled = false` (yard.rs:196-201); no test covers it.
  - `MoveFleet` to the current system clears a regrouping fleet's withdrawal route (fleet/mod.rs:608-611); military.rs:2707 only redirects it home.
  - Replay is checked only for build commands (military.rs:1544-1552) and colony commands (harness.rs:682-706).

**What is refuted or overstated**
- **Failure scenario, first clause.** Removing the yard's demand report is caught by three tests, including the integration test `a_yard_stands_still_without_goods_or_money_and_resumes_when_they_return` (military.rs:547; natural hull demand there is zero). Only attenuation goes unnoticed.
- **Failure scenario, second clause.** It holds for the 29 military tests. The whole suite goes red only incidentally, through the M3 colony test (finance.rs:212, seed 24 stays loyal) and, with production fully off, the raider test (economy.rs:391).
- **(5)** The hand-written state can occur for one service step: supply runs before `update_blockades` (fleet/plugin.rs), so a home fleet arriving in a system flagged the day before sees the stale flag. Otherwise the rule only bites for regrouping fleets. This is not a defect.
- **(7)** The thresholds are as described, but commit 9be40d9 states the change of measurement openly and the 20-point threshold is unchanged for the mean. I see no discrepancy.
- **(1), detail.** Fuel in the resupply test comes from natural stock.

**Why it is only minor**
On the unmodified code the links work:
- An order in an untouched economy at year 20 finishes close to its build time and moves part prices and arms capital against a control.
- Two years of war lower cohesion and raise the bond yield against a peace control.
- Replay of commands given mid-battle reproduces the state exactly.
- A damaged defender is fully repaired and resupplied from natural stocks by the next year start.

One thing the injected-goods tests hide: at game start a 60-day corvette takes 271 to 339 days, because no arms industry exists yet.

**Scope for a fix**
- A natural-economy order test against a control (finish time, part prices, arms capital).
- A losses-to-cohesion and bond-yield test against a peace control.
- A replay check in the scripted war.
- The edge cases under (6).
- Not needed: (5), (7), and the "yard demand removed" scenario.

Nothing here comes from stored memory notes; there were none. The worktree is unchanged (git status clean at 4b64d65).

**Belege der Gegenprüfung.** **Reviewer's own checks, reproduced in the worktree**
- `cargo test -p sim`: exit 0; 83 unit, 6 + 9 + 29 integration tests.
- `headless hash --seed 3 --days 3600`: identical twice and with `--exact`.
- `headless hash --days 1440 --war 1:0:1 --war 1:2:1 --peace 3:0:1`: identical with and without `--exact`.
- `headless sweep --seeds 0..8 --years 60`: supply 0.99, utilisation 0.75, jobs 0.68, price level 0.64, six alive, arrears 0.0 %.
- `headless fleets --seed 3 --years 2 --polity 0`, without and with `--order bastion:8`: weapons 125 to 234, drives 106 to 250, arms capital 1.7 to 2.5, exactly as reported.
- `headless economy --seed 13 --years 0 --extra-days 3 --system Kalara`: stock and demand 0.00 for all four arms goods, arms capital 0.0. So the assertion at military.rs:547 is discriminating.

**Mutations (scratch copy, full `cargo test -p sim --no-fail-fast` each; baseline 127 passed)**

| Mutation | Result |
|---|---|
| M1: yard demand not reported (yard.rs:233) | caught by 3: `fleet::yard::tests::a_day_of_building_moves_goods_and_money_exactly`, `fleet::yard::tests::the_scarcest_input_sets_the_pace_and_marks_the_order_stalled`, `a_yard_stands_still_without_goods_or_money_and_resumes_when_they_return` |
| M2: yard demand × 0.001 | not caught, 127/127 |
| M3: `procure` demand not reported | caught by 2 unit tests |
| M8: `procure` demand × 0.001 | caught by 1 unit test |
| M4: weariness term × 0 in cohesion.rs:213 | not caught, 127/127 |
| M5: weariness term × 0 in bonds.rs:37 | not caught, 127/127 |
| M6: arms plants never run (production.rs) | military 29/29 ok, unit 83/83 ok; failed only economy.rs:391 "nur 0 Frachter aufgebracht" and finance.rs:212 "Seed 24: Konzentration muss Unruhe erzeugen" |
| M6b: arms plants capped at 10 % | military 29/29, unit 83/83, economy 6/6 ok; failed only finance.rs:212 |
| M7: losses do not raise weariness | caught by 2 military tests |

**Probes on the unmodified copy**
- Natural order, seeds 12 / 18 / 3 / 40, no injected stock:
  - Year 0, one Dolch (60 build days): done after 339 / 301 / 301 / 271 days, stalled 211 to 280 of them.
  - Year 20, one Dolch: done after 75 / 64 / 69 / 68 days.
  - Year 20, two Speer: done after 414 / 400 / 400 / 400 days.
  - Year 20, both orders: peak part prices 1.6 to 4.1 times the control; arms capital at the capital 0.97 to 2.59 against 0.14 to 1.41.
- War against peace control after two years, seeds 28 / 22 / 3: weariness 25.5 to 34.4, bond yield 6.0 to 6.8 % against 4 %, cohesion 4 to 5.5 points lower.
- Replay with `SetFleetOrders` issued mid-day and `Retreat` during a running battle, plus a rejected `MoveFleet`, seeds 27 / 23 / 31 / 5: hash, battle reports and command log identical.
- `headless run --seed 0 --years 6..14 --war 5:0:1 --save`: the defender's three surviving ships are at hull, modules, ammo and fuel 1.00 from the year-6 save on.

**Files**
- Tests under review: /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/sim/crates/sim/tests/military.rs
- Mutation results: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_econlinks/mutation_summary.txt
- Mutation script: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_econlinks/mutate.py
- Probe output: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_econlinks/probe.log
- Probe source: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/sim/verify_econlinks/ws/crates/sim/tests/probe_replay.rs

