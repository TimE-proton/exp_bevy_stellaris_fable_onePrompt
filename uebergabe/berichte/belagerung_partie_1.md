# Belagerung in der laufenden Partie, erste Hälfte: Orbit, Tagesablauf, Befehle (Stand bd85c0b)

## summary

Step 4, first half, is done on wip/m5-rechner. Final commit bd85c0b, pushed, worktree clean; every required command is green on that commit.

What now works in a real game, through commands only: a fleet with troops aboard besieges a planet, the garrison lives off its stores and the system market, stations hold the orbit open, a relief fleet or peace lifts the siege, and an ordinary planet that is starved out changes its owner the same day.

1. **Merge.** 5ac904d is merged as a real merge commit (66505fd). Only state.rs and headless main.rs conflicted; balance.ron, harness.rs, economy/mod.rs and lib.rs merged automatically and I read them. The sweep CSV without the hash column (seeds 0..8, 60 years, 49 lines) is identical to a binary I built from 5ac904d, after the merge and still at the final commit. Only the hashes differ, because the state is larger.
2. **orbit.rs** is the only file of the module that names fleets, ships or research, tests included.
3. **Home worlds** start with guns 1, regulars 10 and 60 days of stores (`home_start`); their plan equals these levels.
4. **Daily step.** `siege_day` runs the lifecycle, the day calculator per siege and its effects. I added a second system, `siege_watch`, that runs every tick (see decisions).
5. **Conquest.** `resolve_conquests` hands ordinary planets over through the seam `hand_over`. A capital's verdict stays pending and its siege rests.
6. **Commands.** BeginSiege, SetSiegeOrders, LiftSiege, SetGarrisonOrders, and also EmbarkTroops: starving a planet out needs an occupation force aboard, so I implemented it as the task allowed. The besieged side's MakePeace is rejected.
7. **Invariants.** `siege::check` covers the new states with one call from `check_invariants`.
8. **Tests.** 8 world tests in crates/sim/tests/siege.rs and 30 new unit tests. `cargo test -p sim`: 327 passed, 0 failed, 1 ignored (the existing 150-year research test).

Three things you should know:
- Commit 0b1c818 was pushed with two failing unit tests; the next commit, 91d8f7e, fixes them. Every later commit was tested before it was pushed.
- The pending-capital path cannot be reached by commands yet: home worlds have 10 regulars and a capital needs about 15.8 V to occupy. It is covered by unit tests only.
- The station in the station test is written into the state as a starting condition, because installations cannot be built in the game yet. Everything else in the world tests is played by commands.

Not built, as instructed: sack, LandTroops, Capitulate, SetDefensePlan, procure_day, siege_month, the production and risk hooks, the read model, AI. No file under fleet/, finance/, economy/, polity/, harness.rs, crates/headless or README.md changed after the merge commit.

Memory: nothing from stored memory was used; the one note (checking the game window in a cloud container) does not bear on this task and was not opened. Possibly worth saving, all measured in this session:
- Sweep times on this machine vary with load (47 to 88 s for the same sweep), so only interleaved runs compare; done that way, the final binary and the 5ac904d binary both take about 7 s for 40 years of seed 5.
- The starting fleet has firepower 132.8, 17.5 V of berths and a gun-weighted toughness of 1084.
- On seed 3 after two years, the nearest foreign colony reachable without passing an enemy fleet is G3's colony 90 units from G4's home.

## api_notes

All in crates/sim/src/siege, re-exported from `sim::siege` unless noted.

**Systems and order (plugin.rs)**
- `SimSet::Siege`, chained: `siege_watch` (every tick), then `siege_day` (day start).
- `SimSet::Aftermath`: `conquests` (day start), which calls `resolve_conquests`.
- Nothing requests fine ticks. `siege_watch` returns at once while `active`, `assign` and `embarked` are all empty.

**day.rs**
- `tidy(...)`: the every-tick part. Assignments and embarked troops follow the fleets; a siege is lifted when owner ≠ defender, when the two are no longer at war, or when the attacker is dead.
- `run_day(...)`: lifecycle, then per siege in PlanetId order: unload supply, forage purchase, `resolve`, effects; then disruption on every planet.
- Lifecycle order: `tidy`; drop assignments (fleet not docked in the planet's system, no war with the owner, or another community already besieges the planet); lift when no control and no army; create in FleetId order; station veto; `Region.besieged`.
- `lift(...)`: closes as Lifted. An army ashore re-embarks into free berths if the attacker holds the orbit; the rest is lost.
- Helpers: `planet_facts`, `firm_value`, `prize`.

**mod.rs**
- `close(sieges, finance, planet, outcome, day) -> Option<Siege>` is the one place a record ends. It returns zone militia to `PlanetDefense.militia`, drops a pending conquest for that planet, clears `Region.besieged` and records outcome and days. The caller decides what happens to an army ashore.
- `check_records(galaxy, diplomacy, finance, sieges)`: defender owns the planet and is at war with the attacker; world militia equals zone militia; `Region.besieged` ⇔ record exists; every pending conquest belongs to a siege of the community that takes it.
- `Blocked::Handover` ("Übergabe steht aus") is new; `Blocked::ALL` now has 8 entries. While a conquest is pending for a planet, `siege_day` does not resolve that siege and writes a report with only this reason.
- `setup_sieges` sets the home-world start values.

**orbit.rs** (`sim::siege::orbit`)
- Read-only free functions, usable with `&Fleets` and `&Research` by read model, AI and UI: `squadron(...) -> Squadron { fleets, ships, view: OrbitView, toughness, fuel, berths, supply_aboard }` and `control(...)`.
- `Orbit<'a>` wraps `&mut Fleets` and `&Research`. Methods: `standing`, `own` (the fleet layer's rejection texts), `berths`, `besiegers`, `control`, `squadron`, `apply`, `settle`, `merged`, `split`, `veto_blockades`, `refresh_blockades`, `is_blockaded`.
- `apply(data, sieges, fleets, attacker, effects) -> Toll { ships_lost, troops_lost }` applies hull by class weight, ammunition and fuel, removes wrecked ships, calls `Fleets::refresh` and drops troops without berths.
- `OrbitAccess` is the SystemParam for systems; `orbit::check(world)` holds the fleet-side invariants.
- `orbit::testing::Scene` (test only) is the hand-placed scenario all unit tests of the module use.

**conquest.rs**
- `hand_over(galaxy, economy, finance, issuers, planet, to, HandOverTerms { cohesion, stage, shock }, day) -> Option<PolityId>` is the SEAM, marked "NAHTSTELLE" in its doc comment. It sets owner, `tax_exempt = false`, the region's owner_seen, since_day, stage, months and cohesion, moves `State(old)` shares in the given issuers to `State(new)`, drops the old state's offers and applies the shock.
- `resolve_conquests(...)`: an entry is valid only while its siege exists with `attacker == to` and the defender still owns the planet. Capitals are pushed back untouched.
- When the shared ownership path arrives: replace `hand_over`, delete the capital branch in `resolve_conquests`. `tidy` already closes the sieges of a dead attacker.
- `mirror_region(balance, defense, owner, population, region)` writes occupation, garrison_cover and war_damage; a later monthly step can call it for every planet.
- The place for sack is marked by a comment inside `conquer`.

**orders.rs and command.rs**
- New `SimCommand` variants at the end, in this order: `BeginSiege { fleet, planet }`, `SetSiegeOrders { planet, orders }`, `LiftSiege { planet }`, `SetGarrisonOrders { planet, posture, hold_fire }`, `EmbarkTroops { fleet, planet, strength: f64, supply_days: u16 }`.
- `CommandContext` gained `sieges`.
- MakePeace calls `peace_veto` first. MergeFleets and SplitFleet carry troops and the siege target along.
- LiftSiege closes the record at once and clears the assignments; without a record it only clears the community's own assignments to that planet.

**Data**: three new levers in balance.ron `siege`: `supply_days_cap: 120`, `conquest_cohesion: (5, 70)`, `occupation_min: 0.1`.

**step.rs**: one addition, `stations_after_duel`, which `resolve` now uses itself.

**For the second engineer**
- LandTroops: `step::land` plus a deduction from `sieges.embarked[fleet]`; check with `orbit.own`, `orbit.control` and `invested`.
- Capitulate: push `Conquest { outcome: Capitulated }`; the mercy factor is already applied. The command must land the missing occupation force itself, as the resolver's verdict does.
- Read model: `Campaign::view` in tests/siege.rs shows how a `SiegeView` is assembled from `orbit::squadron`, `planet_facts` and `firm_value`.

## decisions

1. **Every-tick system `siege_watch`.** Fleets die and peace is made in the middle of a day, and `check_invariants` is called at arbitrary ticks. With a day-start-only lifecycle, the three new invariants would be violated between the event and the next morning. Battles end and commands apply only on ticks that are executed anyway, so skipped and exact runs stay identical (tested).

2. **Control is stricter than the specification's sentence.** The besieging fleet must be ready (not regrouping) and have fuel in every tank; an enemy fleet that is regrouping does not contest the orbit. The first and last mirror the blockade rule. The fuel rule mirrors the forecast, which ends a course at `Limit::Fuel`; without it, world and forecast would disagree.

3. **Firepower** is `Ship::condition(data)` × Σ dps of weapons that are not point defence. Weapons with ammunition per shot count only with ammunition aboard. Carried fighters are not counted (their class has gun weight 0). Magazine fill comes from `Fleet::supply`, weighted by capacity.

4. **Station veto without touching fleet files.** `update_blockades` runs in SpaceCombat; the siege layer removes vetoed systems directly afterwards and again after a conquest. No fleet file changed.

5. **A siege with a pending conquest rests.** I read "nothing else happens" as: no resolution, no fuel, no supply. The report carries `Blocked::Handover`, which needs no new state field.

6. **Forage** buys only the essential goods of the day bundle (food today), through the existing `economy::procure`, from the owner's treasury. Nothing is bought when stores are empty or the surface is lost, because the resolver ignores forage then. Whether the planet is invested today is asked of the resolver's own rule, so the garrison already buys on the day the last station falls.

7. **`wear_month`** is applied in `siege_day` every 30 days of a record's age; the specification lists it under weariness, not under the monthly step.

8. **Grievances** are firm book value destroyed that day × the third party's stake.

9. **Conquest details the specification leaves open.**
   - Stake is read fresh from `ownership_of_region`.
   - The losers' militia is set to 0.
   - Stores become days for the new garrison: (old depot × remaining garrison + army supply) / army, capped.
   - The plan is the current levels, rounded up.
   - Region mirrors are written at once.
   - Assignments to the planet are cleared.

10. **New levers instead of literals**: `conquest_cohesion`, `occupation_min`, `supply_days_cap`.

11. **`hand_over` uses only public items of finance**: the public `holdings` and `offered` fields and `shares::seat_of_issuer`. It does not make `issuers_of_region` or `Issuer::add/remove` public, so the ownership branch has no conflict there. The cost is eight duplicated lines (`local_issuers`).

12. **EmbarkTroops.**
    - Supply is paid from the treasury, like fleet resupply, because no armament budget exists yet.
    - Only whole sets of the day bundle are bought.
    - Under a siege that is not invested, only regulars above ground can embark.

13. **SetGarrisonOrders** moves regulars between surface and underground at once and adjusts `surface_start` by the same amount. Otherwise a switch to Deep could make the surface count as broken.

14. **Peace veto, literally.** If two communities besiege each other, neither can make peace until one lifts.

15. **LiftSiege closes immediately**, and peace closes in the same tick, instead of waiting for the next day start. The task asked only that records close through one function.

16. **Station test target.** The twin uses the nearest foreign capital, because it has real stores from `home_start`; only the station is written. The defender moves its home fleet aside by command first.

17. **Chronicle entries** for the start, lifting and conquest of a siege (ChronicleKind::War).

18. **Extra tests beyond the list.**
    - A fuel test (empty tanks lose the orbit).
    - A ten-year many-wars test on seeds 4, 10 and 14 with invariants checked every day.
    - Two forecast-against-world checks.
    - A one-off mutation run: with the veto, the fuel rule and the peace rules disabled, 5 unit tests and 3 world tests failed as they should.

## measurements

All from `cargo test -p sim --test siege -- --nocapture` at the final commit, seed 3. Phase names are the game's German labels. Log: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/belagerung/f1/measure.log

**A. Colony starved out.** G4 besieges G3's nearest colony (53 M inhabitants, no stores, no installations) with the starting fleet and 8 V aboard; stance Blockade; no fire, no landing.

| Day | Phase | Stores (days) | Militia V | Regulars V | Aboard V | Ships | Hull | Ammo | Fuel | System |
|---|---|---|---|---|---|---|---|---|---|---|
| 725 | Einschließung | 0.0 | 5.91 | 0 | 8.00 | 6 | 100 % | 100 % | 89 % | closed |
| 726 | Einschließung | 0.0 | 5.77 | 0 | 8.00 | 6 | 100 % | 100 % | 89 % | closed |
| 727 | Einschließung | 0.0 | 5.63 | 0 | 8.00 | 6 | 100 % | 100 % | 88 % | closed |
| 744 | Einschließung | 0.0 | 3.77 | 0 | 8.00 | 6 | 100 % | 100 % | 83 % | closed |
| 764 | Einschließung | 0.0 | 2.34 | 0 | 8.00 | 6 | 100 % | 100 % | 77 % | closed |
| 784 | Einschließung | 0.0 | 1.46 | 0 | 8.00 | 6 | 100 % | 100 % | 71 % | closed |
| 803 | Einschließung | 0.0 | 0.93 | 0 | 8.00 | 6 | 100 % | 100 % | 65 % | closed |
| 804 | fallen | 0.0 | 0 | 1.82 | 6.18 | 6 | 100 % | 100 % | 65 % | open |

- The planet fell after 80 days with no losses on either side and no damage. The 1.82 V on day 804 are the occupation force that landed.
- The forecast for "Aushungern", built from the world on day 1, said 81 days.

**B. Capital with and without a station (twin runs).** G0's fleet after a 365-unit march (46 % fuel left) over the nearest capital: militia 55.78 V, regulars 10, stores 60 days, guns 1. The twin has one station level.

| Day | Without station | With station |
|---|---|---|
| 25 | Einschließung, stores 59.4, closed, hull 100 % | Anmarsch, stores 60.0, station 0.95, open, hull 97 %, ammo 99 % |
| 28 | stores 57.6 | station 0.79, stores 60.0, open, hull 90 %, ammo 96 % |
| 32 | stores 55.2 | station 0.59, stores 60.0, open, hull 82 %, ammo 92 % |
| 36 | stores 52.8 | station 0.40, stores 60.0, open, hull 76 %, ammo 88 % |
| 44 | — | station 0.03, stores 60.0, open, hull 71 %, ammo 80 % |
| 45 | — | Einschließung, station 0, stores 59.4, still open, hull 71 %, ammo 79 %, fuel 40 % |
| 46 | — | stores 58.8, closed |

- "Hull" is the weakest ship, the cruiser. Without a station, stores fall 0.60 a day (the market covers 40 %).
- One station level held 21 days and cost the cruiser 29 % hull and the fleet 21 % of its magazines.

**C. Fuel.** The same fleet without a station: the forecast said the course fails on fuel after 155 days. In the game the siege was lifted on day 181 with empty tanks; the capital had used up its stores and kept 17.1 of 65.8 V.

**D. Relief.** On the 50th siege day (day 774, militia 1.85 V) the defender sends its fleet. It arrives on day 788: battle, siege lifted, system open, militia 1.36 V. After 47 days of battle the besieger holds the field with 5 of 6 ships (weakest hull 64 %, ammo 71 %) and a new siege begins with the militia back at 6.12 V.

**E. Many wars.** Ten years, every community against its neighbour in the list, invariants and money sum checked every day.

| Seed | Sieges begun | Starved | Lifted | Still running | Battles |
|---|---|---|---|---|---|
| 4 | 23 | 12 | 11 | 0 | 59 |
| 10 | 62 | 2 | 59 | 1 | 58 |
| 14 | 18 | 12 | 5 | 1 | 2 |

A one-off run over seeds 0..16 with checks every ten days also showed no violation.

**F. Unit-level numbers.**
- On the first day of a capital siege the owner paid 6.04 credits for the garrison's food.
- Starving a capital with stores set to 0 took 77 days.
- One day of area fire on a 20 M colony destroyed 5 % of population and capital.

## game_api_changes

crates/game was not built. This rests on grep over crates/game/src and on my diff since the merge commit.

**Result: nothing the game uses was removed, renamed or changed in signature by my work.**

Sim items the game uses that are touched:
- `sim::command::SimCommand`: five variants appended (BeginSiege, SetSiegeOrders, LiftSiege, SetGarrisonOrders, EmbarkTroops). The game only constructs variants (panels.rs, finance.rs, fleets.rs) and has no exhaustive match, so it compiles unchanged.
- `sim::state::SimState`: fields `battles` and `sieges` come from the merge and the earlier siege work. The game does not build it by literal.

Changed, not used by the game (grep found no hit for `sim::siege`, `Sieges` or `CommandContext`):
- `command::CommandContext` gained `sieges`.
- `siege::Blocked::ALL` is now `[Blocked; 8]`.
- `SiegeBalance` gained three fields.
- `sim::siege` gained the items listed under api_notes.

Changes in meaning the game has to follow:
1. `MakePeace` (sent from panels.rs) can now be rejected with "{Gemeinschaft} belagert {Planet}: Frieden kann nur der Belagerer anbieten".
2. A planet can change its owner by conquest on any day, not only by secession at a month start.
3. `Fleets.blockaded` (read in traffic.rs and ui/economy.rs) no longer contains systems whose owner has a station.
4. New chronicle entries of kind War: start, lifting and conquest of a siege.
5. Home worlds have defences from day one, and `Region.besieged`, `occupation`, `garrison_cover` and `war_damage` now receive real values. The cohesion panel shows "Besatzung" and "Kriegsschäden" by itself, because it iterates the contributions.
6. Saves: SAVE_FORMAT is still 4, as instructed. A save written by a build without the siege state loads with an empty defence list and then fails `siege::check`.

## files_changed

- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/assets/data/balance.ron
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/data/defs.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/data/siege_loader.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/siege/conquest.rs (new)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/siege/day.rs (new)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/siege/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/siege/orbit.rs (new)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/siege/orders.rs (new)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/siege/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/siege/step.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/tests/siege.rs (new)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/sim/src/state.rs (conflict resolved in the merge commit only)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung/crates/headless/src/main.rs (conflict resolved in the merge commit only)
- All other files that differ from 2764a4a came in unchanged through the merge of 5ac904d (fleet/*, tests/military.rs, headless fleets.rs and war.rs, README.md and others)

## checks

All run in /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung on the final commit bd85c0b with a clean worktree, every cargo and headless call under `nice -n 10`, output to log files, exit codes read directly. Logs and summary.txt: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/belagerung/f1/final/

| Command | Result |
|---|---|
| `cargo build -p sim -p headless` | exit 0 |
| `cargo test -p sim` | exit 0; 327 passed, 0 failed, 1 ignored |
| `cargo clippy -p sim -p headless --all-targets -- -D warnings` | exit 0 |
| `cargo fmt -p sim -p headless -- --check` | exit 0, no output |
| `cargo build --release -p headless` | exit 0 |
| `target/release/headless sweep --seeds 0..8 --years 60` | exit 0, stderr empty, no invariant violation |
| `target/release/headless hash --seed 3 --days 3600` | two runs identical, and identical with `--exact` (day 3600: 56713fab0ffeeb70) |

- **Test breakdown**: unit 191, battle 45, economy 6, finance 9, military 34, siege 8, tech 34. The ignored test is the existing 150-year research pace test.
- **Sweep against the core**: the CSV without the hash column (49 lines) is identical to the output of the binary built from 5ac904d, which I built from `git archive` into the scratch directory. No sieges occur in this sweep, because nothing issues siege commands.

Additional checks:
- The scripted war sweep from the core report (seeds 0..8, 40 years, nine wars and one peace): exit 0, no violation, 5.8 battles and 19.5 lost ships per run, the same means as the core report.
- `hash --seed 3 --days 3600` with five scripted wars and one peace: identical with and without `--exact`.
- `cargo doc -p sim --no-deps`: no warnings.
- `headless siege --reference`: exit 0.

Not run: any build or test of crates/game, as instructed.

## commit

bd85c0b852f38f84b4c044538602a4d539b0bccb on wip/m5-rechner, pushed (origin/wip/m5-rechner is the same commit). Merge commit: 66505fd. One commit per numbered step: 0b1c818 (orbit; two unit tests failed here, fixed in 91d8f7e), 9988300 (home start), 6c36bed (siege_day), d5a2112 (conquest), 5a12ce5 (commands), 48ea295 (check), 83a5ca3 (world tests); then b01c667, 82b9826, 6733167, 396ca46, 3b3a049, 5bc9e03, bd85c0b (refinements and further tests).

## open_issues

- Second engineer: sack is not built. The place is marked in conquest.rs `conquer`; `SetSiegeOrders` already accepts `sack: true` and the flag travels in `Conquest::sack` without effect. Still missing there: no sack after capitulation or liberation, and grievances for looted credits.
- Second engineer: LandTroops and Capitulate are missing. No army can go ashore by command, so storming, disruption above 0 and re-embarking are covered by unit tests only, and troops aboard cannot be brought back to an own planet.
- Second engineer: the production hook (`disruption` is written but nothing reads it) and the risk hook (`Region.besieged` is written but `risk_discount` does not read it) are not built. Tests 10, 11 and 15 remain; the conquest parts of 11 (state shares, foreign holders, Restless, occupation) are already asserted in my tests.
- Capital conquest stays pending and its siege rests indefinitely until the shared ownership path exists. In that state the defender cannot make peace; the attacker can make peace or lift the siege. This cannot be reached by commands today, because a capital needs about 15.8 V to occupy and a community has 10 regulars.
- Occupation and war damage never decay, militia is never set in peacetime, and the Region mirrors are written only at conquest. All of that belongs to the monthly step (siege_month), which I did not build. A conquered planet keeps its full occupation penalty until then.
- A short interruption resets the militia: when a siege is lifted and re-formed, the record recomputes the militia from the formula, as the specification says. In the relief test it went from 1.36 back to 6.12 V. Once the monthly step maintains the standing militia, creation could take the smaller of the two values.
- Colonies have no stores and no regulars until procurement exists, so any fleet with about 2 V aboard takes a small colony in roughly 80 days without a shot. The relief test's 'stores refilled against payment' cannot be tested yet.
- The fleet layer's blockade does not need fuel, the siege does. A fleet with empty tanks loses the siege but keeps the system closed (seen in the fuel test). This is a question for the fleet branch.
- Supply for embarked troops needs munition on the home market, which is usually empty at game start, so `EmbarkTroops` normally buys none. It is paid from the treasury until an armament budget exists; so is the garrison's food.
- On the day the last station falls the system still counts as open; the blockade starts the next morning.
- `PlanetDefense` stays with the planet through secession and abandonment. An abandoned colony keeps regulars and occupation values, and a later settler inherits them. Neither the shared ownership path nor the monthly step exists yet to reset them.
- Integration: SAVE_FORMAT is unchanged although the state contains `Sieges`; it should be raised when the branches are joined.
- Integration: README.md does not mention sieges, the new commands or tests/siege.rs.
- Integration: command.rs is the likely conflict point with the other branches (new variants and match arms at the end, the MakePeace arm, the MergeFleets and SplitFleet arms).
- Integration: the scripted wars of the headless tool never besiege, so sweeps contain no sieges until a script or the AI issues the commands; `RunSummary` has no siege figures yet.
- The relief world test depends on timing in its scenario: the relief arrives on siege day 64 and the colony would fall on day 80. A balance change that moves either number breaks the test.
- The many-wars test is pinned to seeds 4, 10 and 14 and asserts that at least one conquest and one lifting occur.

