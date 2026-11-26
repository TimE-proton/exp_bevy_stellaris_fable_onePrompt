# Belagerungsrechner (M5 Schritte 1 und 3): Abschlussbericht

## summary

Steps 1 and 3 of the M5 specification are implemented on branch wip/m5-rechner (worktree .worktrees/belagerung), pushed, all required commands green.

What exists now:
- Skeleton (step 1): new module crates/sim/src/siege/ (mod.rs data + read model, step.rs resolver + forecast, plugin.rs empty SiegePlugin registered before AiPlugin). `Sieges` is in sim_state! directly after `fleets` and is built in SimState::generate with one neutral PlanetDefense per planet. `SiegeBalance` is the last field of `Balance`, block `siege` is the last block of assets/data/balance.ron; it is resolved and validated at load (goods, technologies, value ranges, fort >= 1, termination bound 495 <= 500 days) with German messages. New neutral fields: PlanetEconomy.disruption; Region.besieged/occupation/garrison_cover/war_damage; CohesionFactors.occupation/garrison_cover/war_damage with the contributions „Besatzung" and „Kriegsschäden" and the factor on „Aufbruchstimmung". No production hook, no risk hook, SAVE_FORMAT unchanged. siege::check(world) is called by one line in harness::check_invariants.
- Resolver and forecast (step 3): step::resolve(&mut Siege, &mut PlanetDefense, &PlanetFacts, &OrbitView, &SiegeBalance) -> DayEffects with the seven parts of the day, zone effects and verdicts, no randomness, no access to fleets, markets or research; step::form builds the record and its zones; step::land, step::hull_shares; siege::forecast for Aushungern / Sturm / Flächenbeschuss iterates the same resolver through a small `Projection` (about 2-70 microseconds per forecast).
- Tests: the eight tests of section 8 plus ten further resolver tests in siege/step.rs, eight tests in siege/mod.rs (state, invariants, RON/postcard round trip of a running siege, cohesion wiring), four loader tests. cargo test -p sim: 209 passed (115 unit, 94 integration).
- Tool: `cargo headless siege` (crates/headless/src/siege.rs, registered at the end of enum and match): scenario from options, daily report and the three forecasts in German; `--reference` prints the three roads for the three reference planets.
- Tuning: six levers changed, three added (see measurements). At a landing force of 1.5 x the garrison: waiting 176 days with no losses and the prize intact, storm about four weeks with 7-10 % troop losses, area fire 9-14 days at 14-31 % of the prize.

Behaviour of existing games is unchanged: with the new state at neutral values the sweep metrics (seeds 0..8, 60 years), the `economy --seed 3 --years 40` report and the chronicle of `run --seed 5 --years 60` are byte-identical to the binary built before my changes (only the state hash differs, because the state grew).

Memory: nothing from the stored memory was used (the one note, on checking the game window in a cloud container, does not bear on this task and was not opened). Possibly worth saving, learned in this task: (1) the headless binary takes `assets/data` relative to the current directory, so running a worktree's binary from the parent checkout silently loads the parent's data - always cd into the worktree or pass --data-dir; (2) measured in games (seeds 1 and 3, years 0/20/40): the book value of a planet's firms is about 38 credits per million inhabitants (single planets 20-80); (3) the starting fleet (1 Speer, 2 Wächter, 3 Dolch) has 35 hull segments = 17.5 V of berths and a gun-weighted toughness of 1084, computed from hulls.ron/modules.ron/designs.ron; (4) the full `cargo test -p sim` takes 3-5 minutes on this machine, longer under parallel agents.

## measurements

Reference setup (tool defaults, reproducible with `target/release/headless siege --reference`, run inside the worktree): cohesion 70 (will 0.82), essentials and forage 1.0, depot 60 days, no stake; colony and world have militia only, the home world has the start values of section 3 (guns 1, regulars 10, depot 60). The attacker lands 1.5 x the garrison and brings the starting fleets that carry those troops (150 firepower, 17.5 V of berths, weighted toughness 1084 per fleet), 90 days of supply, full magazines and tanks. Prize = firm book value (38 credits per million, measured) + prize_per_million x population. Losses in V (Verbände); defender losses = killed in combat and by fire (the rest of a garrison dissolves when zones fall).

TUNED LEVERS (commit 2764a4a)
| Planet (garrison; attacker) | Road | Outcome | Days | Attacker losses | Defender losses | Hull of heavy ships | Prize kept |
| Colony 50 M (militia 5.8 V; 8.7 V, firepower 74) | Aushungern | starved | 176 | 0.0 | 0.0 | 0 % | 100 % |
| | Sturm | stormed | 29 | 0.9 | 4.2 | 0 % | 100 % |
| | Flächenbeschuss | stormed | 9 | 0.2 | 4.6 | 0 % | 69 % |
| World 500 M (militia 18.3 V; 27.4 V, firepower 235) | Aushungern | starved | 176 | 0.0 | 0.0 | 0 % | 100 % |
| | Sturm | stormed | 24 | 1.9 | 11.2 | 0 % | 100 % |
| | Flächenbeschuss | stormed | 10 | 0.6 | 11.2 | 0 % | 79 % |
| Home world 4000 M (militia 51.7 + regulars 10, 1 gun; 92.5 V, firepower 793) | Aushungern | starved | 176 | 0.0 | 0.0 | 0 % | 100 % |
| | Sturm | stormed | 24 | 9.4 (3.7 of them intercepted by the gun while landing) | 36.3 | -5 % | 99 % |
| | Flächenbeschuss | stormed | 14 | 6.6 | 35.8 | -1 % | 86 % |
Waiting ends within the bound (495 days by formula, limit 500) and leaves the prize intact; the landing takes 3.5-4 weeks and costs 7-10 % of the wave; fire is 2-3 times as fast and costs 14-31 % of the prize.

SAME SCENARIOS WITH THE LEVERS EXACTLY AS SPECIFIED (before tuning): Sturm 13 / 9 / 10 days with 0.9 / 1.7 / 8.8 V lost; Flächenbeschuss 7 / 6 / 9 days, prize kept 87 / 93 / 95 %; Aushungern 176 days. I.e. every world fell to a storm within two weeks and fire was barely faster on the home world.

SENSITIVITY, tuned levers (colony / world / home world)
- force ratio 1.0: Sturm 52 / 44 / 42 days, losses 1.4 / 3.2 / 11.3 V (17-24 % of the wave); Flächenbeschuss 15 / 18 / 29 days, prize 66 / 75 / 87 %.
- ratio 2.0: Sturm 20 / 15 / 15 days; Flächenbeschuss 6 / 6 / 10 days.
- ratio 3.0: Sturm 10 / 5 / 8 days (militia-only worlds collapse quickly against overwhelming force).
- ratio 0.5: Sturm fails (survivors too few to occupy), Flächenbeschuss needs 56 / 77 / 102 days.
- Aushungern by depot (home world): 0 days -> 77; 60 -> 176; 120 -> 276; 180 and 240 -> not within one tank (fuel is out after 334 days).  Colony with shortage (forage 0, essentials 0.5): 116 days.
- Capital fortified per the AI plan of section 5 (stations 2, guns 3, shield 2, bunkers 2, regulars 25.8, depot 180) against 150 V / firepower 1290: posture Forward: Sturm 35 days, 25.8 V lost, hull -18 %; Flächenbeschuss 26 days, prize 85 %. Posture Deep: Sturm 121 days and 72.9 V (the bunkers), Aushungern fails on fuel.
- Threatened world (500 M, regulars 4.6, gun 1, bunker 1, depot 120) against two full starting fleets: Aushungern 276 days, Sturm 28 days / 4.0 V / hull -19 %, Flächenbeschuss 12 days / prize 74 %.
- One full starting fleet (17.5 V) against a 50 M colony: Sturm 10 days, 0.4 V; against the untouched home world: Sturm fails (fleet destroyed by the gun), Aushungern succeeds after 176 days.

LEVERS CHANGED AGAINST THE SPECIFICATION
1. r 0.03 -> 0.015. With 0.03 a storm at 1.5:1 was over in 9-13 days; halving the daily loss rate makes the ground war take weeks without changing who wins or the loss ratio.
2. beach_days 3 -> 6. Keeps the beachhead phase at about a quarter of a storm now that the storm takes twice as long; otherwise posture Forward loses its point.
3. tunnel_front 8 -> 16. Keeps the underground kill rate at the specified 0.24 V per day (r x front), so storming 40 regulars in three bunker levels still costs about five months and roughly three times as many attackers; with r halved and front 8 an underground assault would always be slower than the stores (bunker_days 120) and never be worth ordering.
4. zone_fall 0.15 -> 0.10. At 0.15 a zone holding less than 15 % of the army's strength fell without a fight, which is every zone of a many-zoned world from about 1.3:1; at 0.10 zones are fought for up to about 2:1, durations are similar across planet sizes (29 / 24 / 24 days instead of 25 / 16 / 18), and untouched cities usually still hold after the Center falls.
5. strand_rate 0.02 -> 0.025. zone_fall is also the threshold below which a landing counts as repelled; at 0.10 a cut-off landing would last 114 days with 0.02, which breaks test 7 (gone within 100 days). With 0.025 it is gone after about 91 days.
6. bombard_k 0.025 -> 0.05. With 0.025 area fire shortened the fall of the home world only from 10 to 9 days; with 0.05 the fire road is 2-3 times as fast as the storm. The published ratios (precise 15:1, area 3:1) and bombard_cap are untouched.
LEVERS ADDED (values taken from the specification's prose, so they are no longer literals in code): city_from (100, 1000) = population thresholds for one and two City zones; militia_cohesion (60, 0.3, 1.3) = reference cohesion and bounds of the militia factor; storm_loss_limit 0.5 = loss share up to which the forecast (and later the AI) treats a storm as acceptable.
All other levers have the specified values.

## api_notes

All in crates/sim/src/siege (re-exported from `sim::siege`), plain serde data, no HashMap.

STATE (mod.rs)
- `Sieges` (Resource, in sim_state! after `fleets`): defenses: Vec<PlanetDefense> (per planet, permanent), active: BTreeMap<PlanetId, Siege>, assign: BTreeMap<FleetId, PlanetId>, embarked: BTreeMap<FleetId, Embarked {troops, supply}>, pending: Vec<Conquest {planet, to, outcome, sack}>, budget: BTreeMap<PolityId, Money>, plans: BTreeMap<PolityId, AiPlan {target, staging, fleets, started_day}>, grievances: BTreeMap<(PolityId, PolityId), f64>, stats: SiegeStats. Helpers: Sieges::new(planets), defense(), defense_mut(), siege(), reserved(fleet), find_anomaly(planets). `setup_sieges(data, galaxy, polities)` is the place for the home-world start values (step 5); today it returns neutral defences.
- `PlanetDefense`, `DefensePlan`, `Posture {Forward, Deep}`, `Siege`, `SiegeOrders {fire, assault, focus, sack}`, `Zone {kind, share, militia, taken}`, `ZoneKind`, `Fire {Hold < Support < Precise < Area}`, `Outcome {Stormed, Starved, Capitulated, Lifted}`, `SiegeReport` (yesterday: zone fought/fallen, losses, hull, ammo, shield absorbed, damage, `blocked: BlockedSet` of `Blocked` reasons with German labels), `SiegeStats {begun, stormed, starved, capitulated, lifted, days, longest, prize_start, prize_kept, lost_attackers, lost_defenders}` with count(), conquests(), record(outcome, days). All enums have German `label()`.
- Derived, never stored: `Siege::phase(&defense) -> Phase` (Anmarsch, Einschließung, Beschuss, Bodenkrieg, Untergrund), `Siege::{stake, zone_militia, reserve, defenders, zone_taken, surface_lost, front}`, free functions `will(cohesion, stake, balance)`, `invested(orbit, defense)`, `guns_active(siege, defense)`, `occupation_force(population, balance)`.
- Invariant kept by the resolver: while a record exists `defense.militia` equals the sum of the zone militia, `siege.deep <= defense.regulars`; scar and scar_by are written by the resolver.

RESOLVER INPUTS (hand-buildable)
- `PlanetFacts {day, population, cohesion, essentials_met, forage}`; `OrbitView {control, firepower, ammo (magazine fill 0..1; only > 0 is tested), embarked, att, def}`; `TechView {ground_attack, ground_defense, weapon_damage, shields, hull, armor, gates: [bool; 4]}` holds surcharges (0.1 = +10 %; TechEffects factors minus 1), gates indexed by `Gate {Bunker, Support, Tunnel, Shield}` in the order of `SiegeBalance::gate_ids`. `TechView::default()` is the neutral value.

STEP (step.rs)
- `form(attacker, defender, &mut defense, &facts, stakes, prize_start, balance) -> Siege`: militia = militia_strength x (1 - stake of the attacker), zones, deep under posture Deep, garrison_start, surface_start, population_start; also writes defense.militia. `build_zones`, `militia_strength(population, cohesion, balance)` (the monthly formula with stake 0).
- `land(&mut siege, &defense, strength, day, balance) -> Landing {landed, lost}`: gun interception, army_peak, landed_day. The LandTroops command should call it with clock.day() and deduct `strength` from the fleet.
- `resolve(...) -> DayEffects {hull, ammo, fuel, population, capital_fire, capital_ground, shock, disruption, attacker_lost, defender_lost, fire_victims, landed, verdict}`; `capital()` is the product of both capital factors; `DayEffects::NONE` is a day without effect. Contract for siege_day (step 4): build the two views; with control and an army ashore add the embarked supply to `siege.army_supply` before the call; afterwards apply hull via `hull_shares(hull, &[(ShipClass, toughness)], balance)` (hull fraction per ship), ammo and fuel as shares of magazine and tank, population x= effects.population, every capital x= effects.capital(), issuer shock x= effects.shock, disruption = max(effects.disruption, old x (1 - disrupt_decay)), deduct effects.landed from the embarked troops, weariness from attacker_lost / defender_lost / fire_victims, and push a Conquest when verdict is Some (Stormed or Starved; Capitulated and Lifted come from command and lifecycle).
- `Projection` (new(&SiegeView, balance), step, land, prize, prize_loss, hull_loss, invested, storm_succeeds): a miniature of what the game does around the resolver (supply unloading, magazines, tanks, firepower fading with hull damage, population and firm value). The tool and the forecast use it; `Projection::land` books the landing on the previous day, as a command between day boundaries would.
- `forecast(&SiegeView, Course, &SiegeBalance) -> Forecast {falls, outcome, days, own_losses, enemy_losses, hull_loss, prize_loss, limited_by: Option<Limit>}`; `trace(...)` is the same with a per-day observer. Courses: `Starve` = hold fire, no landing; `Storm` = land everything as soon as invested, assault, support fire; `Area` = area fire, landing as soon as a storm would succeed with at most storm_loss_limit of the force lost (nested trial run). With empty magazines the courses cease fire. `Limit` names why a course fails (NoControl, Stations, Fleet, Fuel, Ammunition, Shield, NoTroops, Repelled, Time). hull_loss is the share lost by the hardest-hit class (damage / weighted toughness); prize_loss is a share of today's prize.
- `SiegeView {planet, siege, defense, facts, orbit, toughness, fuel, supply_aboard, firm_value}` is the plain read model the forecast runs on; step 6 fills it from the world (estimates for foreign planets go into the same fields) and may add display-only fields. toughness = sum over ships of gun_weight[class] x (hull + shield + armour); 0 means unknown (no fading). fuel must be filled (0 means the fleet cannot stay).

DATA
- `data::SiegeBalance` (all levers; resolved `day_bundle_ids`, `regular_bundle_ids`, `station_ids`, `gun_ids`, `shield_ids`, `bunker_ids`, `gate_ids`), `SiegeGates`, `HomeStart`, `data::siege_day_bound(&SiegeBalance)`; `CohesionBalance.{occupation, occupation_hold, war_damage}`.
- `siege::check(world)`: vector lengths, strengths and stores finite and >= 0, scar/occupation/stakes/zone shares in 0..1, deep <= regulars, disruption and the Region mirrors in 0..1.

DELIBERATELY MISSING (later steps): orbit.rs (building OrbitView/TechView from fleets and research, applying DayEffects to ships), siege_day and the lifecycle (creating/closing records, station veto, Region.besieged), all SimCommands, procure_day and siege_month (economy::buy_for_state), resolve_conquests / transfer of a planet, the production hook and the risk hook, weariness and grievances bookkeeping, `siege::view` / `defense_view` builders, AI, RunSummary fields, anything in crates/game. The headless `siege` subcommand is scenario-based; a later `--seed/--planet` mode can build the same SiegeView from a world and reuse the output functions.

## deviations

Additions to the interfaces of section 2 and 7 (nothing removed or renamed):
1. PlanetFacts has a field `day`. The resolver needs today's date for „landed yesterday" and the beachhead; the specified signature of resolve has no other place for it.
2. Siege has two more fields: `population_start` (the bombardment floor is defined against the starting population) and `surface_start` (see 5).
3. forecast takes `&SiegeBalance` as a third argument (the resolver needs it and the view is plain data without a reference to it); Forecast additionally carries `outcome` and `enemy_losses`.
Clarifications of the resolver where the text was open or would not work as written:
4. Shield: bombardment exceeding the charge passes through with the remainder. Read literally („charge > 0: nothing else is hit") a shield that recharges every day could never be broken, which contradicts test 1 („with B 500 the charge reaches 0 and damage starts").
5. Occupation force for every conquest: Stormed also requires army + embarked >= occupy_k x sqrt(population) (the missing part lands without loss, as specified for Starved), and the rule „surface lost when zone militia + reserve <= zone_fall x start" applies only to an army of that size and is measured against the surface garrison at the start (`surface_start`), not the whole garrison. Otherwise a token landing takes a planet whose surface was broken by fire, and a planet whose regulars sit in bunkers would count as broken on day one.
6. Cut off = invested or surface lost: stores are consumed and an empty depot dissolves the garrison only then (the underground under a besieger who lost the orbit is still cut off; a planet behind standing stations is not).
7. The beachhead factor applies while today <= landed_day + beach_days, so that a landing ordered after the day's resolution (the normal case) gets the full number of combat days.
8. On a Starved or Stormed verdict only the missing part of the occupation force lands from the ships, not all embarked troops.
9. The station duel uses F as written (not B = F x (1 + weapon_damage)); see open issues.
Levers: six changed (r, beach_days, tunnel_front, zone_fall, strand_rate, bombard_k) and three added (city_from, militia_cohesion, storm_loss_limit) - reasons under measurements.
Layout:
10. Loading and validation of the block live in a new file crates/sim/src/data/siege_loader.rs, called by one line at the end of GameData::resolve_and_validate (pattern of ship_loader/tech_loader; keeps data/mod.rs nearly untouched).
11. rng.rs and finance/mod.rs were not touched: Domain::Siege (and Intel) already exist, Region lives in finance/cohesion.rs.
12. The headless subcommand is `siege` with scenario options instead of the `{seed, planet, days}` of section 6 (that form needs the world systems of step 4/6).
13. The definition of the forecast course „Flächenbeschuss" (area fire, landing when a storm becomes acceptable, cease-fire with empty magazines) and of the reference attacker (1.5 x garrison) are my decisions; the specification does not define them.
14. README.md was not updated (not in the list of files I may touch).

## files_changed

- assets/data/balance.ron
- crates/sim/src/data/defs.rs
- crates/sim/src/data/mod.rs
- crates/sim/src/data/siege_loader.rs (new)
- crates/sim/src/siege/mod.rs (new)
- crates/sim/src/siege/step.rs (new)
- crates/sim/src/siege/plugin.rs (new)
- crates/sim/src/lib.rs
- crates/sim/src/state.rs
- crates/sim/src/harness.rs
- crates/sim/src/economy/mod.rs
- crates/sim/src/finance/cohesion.rs
- crates/headless/src/siege.rs (new)
- crates/headless/src/main.rs

## checks

All run in /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/belagerung on the final commit 2764a4a with a clean working tree, output redirected to log files, exit codes checked (logs: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/belagerung/final/, summary in summary.txt):
- cargo build -p sim -p headless: exit 0
- cargo test -p sim: exit 0; 209 passed, 0 failed, 1 ignored (pre-existing): 115 unit tests (39 of them new: 27 in siege, 4 in data::siege_loader... the siege module contributes 27, the loader 4), battle 45, economy 6, finance 9, tech 34
- cargo clippy -p sim -p headless --all-targets -- -D warnings: exit 0
- cargo fmt -p sim -p headless -- --check: exit 0, no output
- cargo build --release -p headless: exit 0
- target/release/headless sweep --seeds 0..8 --years 60: exit 0, no invariant violation. Per-polity metrics (CSV without the hash column, 49 lines) are identical to the binary built from the starting commit 9c63cad; likewise the 191 lines of `economy --seed 3 --years 40` and the chronicle of `run --seed 5 --years 60`. The state hashes differ from before, as expected, because the state gained `Sieges` and the new fields.
- target/release/headless hash --seed 3 --days 3600: two runs identical, and identical with --exact (day 3600: d9f8e11abad8ae5f)
- crates/game was not built, as instructed.
Additional: a temporary probe confirmed equal run time of old and new binary under the same load (7.0-7.7 s for 40 years of seed 5); `cargo doc -p sim --no-deps` shows no broken links in the new code.

## commit

2764a4a00fc5588cbd37c1844c835ab2f89193ae on branch wip/m5-rechner, pushed to origin/wip/m5-rechner (six commits on top of 9c63cad: a06531d levers and validation; b274578 state, invariants, resolver; c2eb622 tests and headless tool; 27de85c tuning; 672ea78 forecast cease-fire; 2764a4a cohesion tests)

## open_issues

- Starving at the start values: one starting fleet with its 17.5 V aboard takes an unrelieved home world (depot 60) in 176 days without a shot, because the occupation force is only 15.8 V. This is the risk 'starving dominates' of section 10; levers home_start.depot_days, occupy_k, station_fuel. I did not change them - it needs the sweeps of step 7 with relief fleets and AI.
- Militia-only worlds collapse quickly against overwhelming force: a full starting fleet (17.5 V) storms a colony of up to 100 M in about 10 days with 0.4 V lost; at 3:1 any militia-only world falls within 5-10 days. The loss law of the specification (losses proportional to the opponent's strength, militia bound to zones) makes this inherent; the defender's answer is regulars. Levers militia_k (left at 0.7) and the AI's regulars rule, to be settled with sweeps.
- Guns are hard on small fleets: each gun level costs a single starting fleet 2.8 % hull of its heavy ships per day in low orbit (gun_damage 30 against a weighted toughness of 1084); three guns destroy it in 12 days, and under a shield counter-battery fire cannot reach them. station_hp, station_damage, gun_damage, shield_cap and shield_regen are unchanged and were only checked with hand-computed fleet numbers, because the ship-level fleets of M4 are not in this branch.
- Station duel uses F, not B: the attacker's weapon technology does not help against stations while the defender's raises station damage. I kept the text of section 3; if that asymmetry is unintended it is a one-word change in step (1) of resolve.
- Stores longer than a tank: with depot 180 or more, waiting takes longer than the 334 days a tank lasts (station_fuel 0.003), so the bound of 495 days is only reachable with relieved fleets. Intended or not is a balance decision.
- A reinforcement wave resets landed_day and thereby puts the whole army back under the beachhead factor for beach_days; the specification has only this one field. A per-wave weighting would need one more field.
- SAVE_FORMAT is unchanged as instructed: until the integration raises it, a save written before this branch loads with an empty `defenses` list and then fails siege::check.
- README.md does not yet mention the siege tool or the new block of balance.ron (outside my file list).
- crates/game was not built. Its cohesion panel iterates CohesionFactors::contributions, so the two new rows appear by themselves once they are non-zero; nothing there constructs the changed structs by literal (checked by search).
- The attacker wins a storm even at 0.8:1 against militia-only worlds (losing 30-40 % of the wave); whether parity should favour the defender more (fort of City and Center, will_base) is left for the sweeps - the fort values are unchanged.

