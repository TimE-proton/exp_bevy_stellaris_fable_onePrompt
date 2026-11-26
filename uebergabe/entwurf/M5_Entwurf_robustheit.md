# KONFLUENZ M5 – Planetary siege as a minimal state machine

## 1. Core idea

1. A siege is one plain record per planet; its phase (`Blockade → Surface → Underground`) is **derived, never stored**, and the only free inputs are the attacker's standing orders and troop landings.
2. Two stocks drive everything and only fall while the siege lasts – the defender's **stores** (no restocking under hostile orbit) and the attacker's **ammunition and landed troops** – so even "sit and wait" ends every siege in bounded time, and nothing is undone without fresh resources.
3. The five phases of the specification are activities gated by three derived predicates (`control`, `shield_up`, `guns_active`), not nested sub-machines.
4. The daily resolver is a deterministic pure function without randomness over (`Siege`, `PlanetDefense`, `OrbitView`); a thin system applies its effects to fleets, economy and finance.
5. Conquest changes one fact, the planet's owner; the existing ownership model produces the consequences, plus one new cohesion factor, „Besatzung".

## 2. State

Paths are relative to `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/`. New module `siege/` (`mod.rs` data, `step.rs` pure resolver, `orbit.rs` fleet/tech adapter, `aftermath.rs`, `plugin.rs`). Resource `Sieges` joins `sim_state!` (`state.rs`) and `CommandContext` (`command.rs`).

```rust
struct Sieges {
    defenses: Vec<PlanetDefense>,       // per planet, also in peace
    active: BTreeMap<PlanetId, Siege>,  // at most one per planet
    embarked: BTreeMap<FleetId, f64>,   // troop units aboard
    plans: Vec<Option<AiPlan>>,         // per polity, saved AI memory
    stats: SiegeStats,
}
struct PlanetDefense {
    garrison: f64, garrison_target: f64, // units („Verbände"), militia included
    stores: f64,   // in siege days
    guns: f32,     // ground batteries
    shield: f32,   // 0 none, else generator condition 0..1
    station: f32,  // orbital station, hull points (800 when built)
    bunker: u8,    // deep facilities, level 0..3
}
struct Siege {
    attacker: PolityId, defender: PolityId, started_day: u64,
    orders: SiegeOrders,
    att: f64, att_peak: f64,                           // landed attacker units
    def_surface: f64, def_deep: f64, garrison_start: f64,
    zones: Vec<Zone>,                                  // surface only
    devastation: f64,                                  // 0..1
    landed_today: bool,
    report: SiegeReport,                               // yesterday, for UI and AI
}
struct Zone { kind: ZoneKind, hold: f32 }              // 1 defender-held … 0 fallen
enum ZoneKind { Country, City, Battery, Generator }    // „Umland, Stadt, Batterie, Schildgenerator"
struct SiegeOrders { bombard: Bombard, support: bool, assault: bool, focus: u8 }
enum Bombard { Off, Precise, Heavy }                   // „aus, gezielt, flächig"
```

Ground regions are called zones (`finance::Region` already means the whole planet). At siege start: `[Country, City × clamp(pop/1500, 1, 3), Battery if guns>0, Generator if shield>0]`. Derived: `phase()`; `shield_up = shield>0 ∧ stores>0 ∧ Generator.hold>0`; `guns_active` likewise with Battery; `control` = new `Fleets::orbital_control(planet)`: a docked `Stance::Blockade` fleet at war with the planet's owner, not `in_battle`, no docked fleet of the owner (as `fleet::update_blockades`, but per planet).

Also `finance::Region.occupation: f64` (0..1) and `SiegeBalance` (`data/defs.rs`, block `siege` in `assets/data/balance.ron`). `GameData::resolve_and_validate` checks `starve_rate, strand_rate > 0`, `0 < capitulate_frac < 1`, `fort ≥ 1`: the termination guarantee is validated at load.

## 3. Algorithms

`SimSet::Siege`, `run_if(day_start)`, planets in ID order; no fine ticks are ever requested. `tA`, `tD` = `TechEffects` of attacker and defender; defaults in brackets.

**0 Lifecycle.** `control = Some(a)` and no record → create (`def_deep = garrison·(0.25 + 0.12·bunker)`, rest surface). Dissolve when owner changed, peace, attacker extinct, or `att = 0` without control; landed troops re-embark onto a docked own fleet or are lost. Records appear and vanish only here.

**1 Blockade and supply.** Under `control`: `stores −= 1 + (1 − essentials_met)` days per day; no restocking or recruiting under hostile control. At `stores = 0` shield and guns go dark, defender power ×0.5, and `starve_rate(0.02)/will` of the garrison dissolves daily back into `Planet.population`. `will = (0.4 + 0.6·cohesion/100)·(1 − 0.5·attacker_share)` reads `finance.regions[p].cohesion` and `factors.foreign_share`: a world the attacker already owns barely resists. Landed attackers draw `0.5·att` ammo points per day from the besieging ships; without control or ammo: power ×0.5, `strand_rate(0.02)` dissolves daily.

**2 Planetary defence.** `close` = a bombardment, support or landing takes place today. Fire in battery equivalents = `2·[station>0] + guns·[close ∧ guns_active]`; hull damage `fire·gun_damage(10 hull points)·(1+tD.weapon_damage)` is spread over ships by `value × gun_weight[class]` (corvette 0.2 … battleship 1.0: capital ships suffer most) and divided by `design.hull·(1+tA.hull)`. The station sits outside the shield and the fleet answers automatically: `station −= 60·B` (B below), 2 % ammo per day. Once it is gone, waiting is safe.

**3 Bombardment.** Needs `control`, ammo, `¬shield_up` and a phase before Underground; otherwise nothing happens and no ammo is spent (`report.blocked`: „Schild hält"). `B = min(2, Σ value·hull of ships with ammo / 12 000)·(1+tA.weapon_damage)`. Levels `(m, c, ammo/day)`: Precise (1, 0.1, 2 %), Heavy (2, 1, 5 %).
`guns −= 0.15·B·m`; `def_surface ×= 1 − 0.01·B·m`; every `capital[r] ×= 1 − 0.006·B·c`; `population ×= 1 − 0.0015·B·c`; `devastation += 0.01·B·c`.
30 days: Heavy −17 % capital, −45 % surface garrison; Precise −2 %, −26 %.

**4 Landing and ground war.** `LandTroops` moves units from `embarked` to `att`, losing `min(0.4, 0.04·guns)` of them to active guns; the shield does not stop landings. Country must fall first, then `focus` picks any standing zone. With `assault`:

```
sup = 0.5·min(1,B)   if orders.support ∧ control ∧ ¬shield_up ∧ ammo (1.5 %/day), else 0
PA  = att·(1+tA.ground_attack)·(1+sup)·supplyA
PD  = def_surface·(1+tD.ground_defense)·fort[zone]·will·supplyD   // fort 1.0, City 1.6, Battery 1.3, Generator 1.6
r   = PA/(PA+PD);   cas = 0.05·min(att, def_surface)
att −= 2·cas·(1−r);   def_surface −= 2·cas·r;   hold[zone] −= 0.08·max(0, 2r−1)
```

City fighting: `capital ×= 0.999` per day. Generator fallen → shield down; Battery fallen → guns silent, captured intact. Zones never revert while `att > 0`.

**5 Underground.** When all zones fell or `def_surface` ≤ 15 % of its start, the surface is lost and 60 % of the survivors join `def_deep`. Same formula against `def_deep` with `fort = 2.5 + 0.4·bunker`, intensity 0.02, **`sup = 0`, bombardment without effect**. With `assault = false` the attacker holds the surface and step 1 does the work.

**6 Verdict.** `def_surface + def_deep ≤ max(0.1, capitulate_frac(0.15)·garrison_start)` → `Conquest` event for `SimSet::Aftermath` („{Planet} kapituliert vor {Gemeinschaft}."), with or without landed troops. `att ≤ 0.15·att_peak` → landing force lost, zones reset, back to Blockade.

**Bound.** Under unbroken control: at most `180·(1+0.5·bunker) + ln(1/0.15)/0.02` = 275 … 545 days. A landed force without control is gone within 95.

**Monthly** (`month_start`, no hostile control): militia `max(0.5, 0.002·pop)` is free; recruit towards `garrison_target` (≤ `max(1, 0.005·pop)` units; each 1 waffensysteme + 1 munition at `Market::ask`, `population −= 0.02`); upkeep 6 Cr per unit and 1 % of installation value via `economy::pay` to local households (unpaid units disband); refill stores to `180·(1+0.5·bunker)` days, one day costing `0.01·garrison` (+0.1 with shield) bundles of 1 munition + 4 nahrung.

## 4. Commands

| `SimCommand` | Validation (rejections are German strings) |
|---|---|
| `BuildDefense { planet, kind }` | own planet without hostile control; caps (guns ≤ 2 + pop/1000, shield and station 1, bunker 3); parts (battery 6 waffensysteme; shield 20 elektronik, 10 maschinen, 8 waffensysteme; station 12 rumpfsegmente, 6 waffensysteme; bunker level 40 legierungen, 8 maschinen) ≤ half the market stock; treasury covers `ask` |
| `SetGarrison { planet, units }` | own planet; `0 ≤ units ≤ max(10, 0.02·pop)` |
| `EmbarkTroops { fleet, planet, units }` | own docked fleet, not in battle; from own garrison above militia, or own landed force under `control`; capacity `Σ lift[class]` = (0, 0, 1, 3, 6, 8), fighter to carrier |
| `LandTroops { fleet, planet, units }` | own planet → garrison; enemy planet → at war, `control == me`, no other attacker's siege („keine Orbitalhoheit über …") |
| `SetSiegeOrders { planet, bombard, support, assault, focus }` | siege with `attacker == me`; `focus` a standing zone, Country first |

Orders are standing; when blocked, the report says why.

## 5. AI behaviour

`ai/siege.rs`, appended to the `SimSet::Ai` chain in `ai/plugin.rs`; commands only on change. Enemy numbers come from `siege::view(planet, viewer)`, which applies `Research::estimate` (midpoint): no cheating.

*Defender, monthly.* Threat = hostile and rival fleet value within 3 hops (`Galaxy::hops_from`). Spend up to 20 % of `PolityFinance.revenue` (35 % at war) on `SetGarrison` and `BuildDefense` for the capital, `chokepoint` systems and the highest `prize_value`, in the order Guns, Bunker, Shield, Station. Besieged: send a relief fleet only if own value ≥ 1.2 × the estimated besieger.

*Attacker, monthly plan.* One operation per polity, kept until it ends. Target = max `prize_value / (1 + est. garrison·fort)` among reachable enemy planets; `prize_value` = book value of local issuers + five years of tax. Raise troops at a staging world (`SetGarrison`), `EmbarkTroops`, `MoveFleet`, `SetStance(Blockade)`.

*Attacker, daily: pure `decide(view) -> Decision`.*
1. Station alive: stay if fleet value ≥ 1.5 × station, else leave.
2. `r ≥ 0.6` for the next zone: land everything, assault with support; focus Country → Generator (shield up) → Battery (ships being hit) → Cities.
3. Shield down, `r` too low: `Precise` until `r ≥ 0.6`; `Heavy` only with treasury runway < 4 months or `war_weariness > 60`.
4. Shield up, `r` too low: wait.
5. Underground: assault if `r ≥ 0.55`, else hold and starve.
6. Mean hull < 50 % or ammo < 15 %: re-embark and go home unless a second fleet takes over.

## 6. Feedback into economy, finance and cohesion

- **Markets and production** (`economy/mod.rs`, `economy/production.rs`): troops, stores and installations are goods taken from `Market.stock` and paid from the treasury, so arms prices and Arms-firm profits rise; bombardment and city fighting cut `PlanetEconomy.capital`, hence output, wages and taxes; recruits and losses come out of `Planet.population`; besieging fleets must leave the blockade to rebuy ammunition (M4).
- **Share prices** (`finance/shares.rs`): `risk_discount` +0.15 during bombardment or ground war; `book_value` falls with capital; local issuers `shock ×= 0.8` on conquest.
- **War weariness** (`PolityFinance.war_weariness`): +0.05 per unit lost, +`0.03·B·c` per bombardment day suffered, +6 planet lost, −3 planet won. Unchanged code turns this into lower cohesion in every home region („Kriegsmüdigkeit") and a higher `bond_yield` (`finance/bonds.rs`); the drained treasury cuts state spending (`settle_public_accounts`) and triggers the war bonds of `steward_finances`.
- **Conquest** (`siege/aftermath.rs`): new `polity::transfer_planet` (owner, `tax_exempt = false`, capital moves to the most populous remaining world, trade-house `home` follows; last planet → `alive = false`, treasury and state holdings to the victor, its bonds void); `apply_secession` should use it too. **Shareholdings stay**, except `HolderId::State(old)` stakes in local issuers, which pass to `State(new)`. Private holders of the old realm thus become „Fremder Besitz"; above `foreign_pull` the region drifts back unless the victor bids (`TakeoverBid`) or expropriates (`Expropriate`). Loot: those stakes, surviving installations, 10 % of `finance.elites[p]` into the victor's treasury. Damage: capital, population, `devastation`.
- **Fresh cohesion** (`finance/cohesion.rs`): the transfer sets `Region.owner_seen`, `since_day` and stage as `apply_secession` does, but `cohesion = 35·(1 − 0.5·devastation)` and `occupation = clamp(0.6 + 0.8·devastation − attacker_share, 0.05, 1)`. New row in `CohesionFactors::contributions`: „Besatzung" = `−35·occupation·(1 − min(1, garrison/(0.01·pop)))`; „Aufbruchstimmung" is multiplied by `1 − occupation`; `occupation` decays 2 % per month. Holding conquests costs garrisons.

## 7. User interface

`siege::view()` returns a `SiegeView`, the struct the AI also reads:
- planet with zones (kind, `hold`, focus), bunker level, phase („Blockade", „Bodenkrieg", „Untergrund");
- orbit: besieging fleets with hull and ammo %, station hull, guns active or silent, shield with reason („hält", „Generator gefallen", „ohne Vorräte");
- forces: landed, embarked, capacity; defender surface and deep (ranges for the enemy);
- supplies: „Vorräte reichen etwa N Tage", attacker supply status;
- `SiegeReport` (ship damage, losses, capital and population destroyed, ammo used, blocked orders), `devastation`, „Wert der Beute" now against siege start, `r` per zone with and without support.

Controls map 1:1 to commands: bombardment level, support, land and re-embark sliders, „Sturm" or „Abwarten", click a zone for focus; defender: garrison target and four installations. New `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/siege.rs`; `SimRefs` gains `sieges`.

## 8. Tests

Resolver tests in `siege/step.rs`; world tests in `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/tests/siege.rs`.
1. **Shield blocks bombardment.** 30 days `Heavy` under `shield_up`: capital, population, garrison and fleet ammo bit-identical to a run with `Off`; with `shield = 0` capital is strictly lower.
2. **No orbital support underground.** Underground, `support` on vs. off: identical `Sieges` hash and ammo; the same pair in a City differs.
3. **Starving out ends.** Blockade only, on a colony and on a self-sufficient home world with bunker 3: the owner changes after stores reach zero and before the bound; removing the fleet on day 50 lifts the siege instead.
4. **Bombardment lowers the prize.** Same conquest with `Heavy` vs. assault only: Heavy ends with less capital, population and summed issuer value after the next `reprice`, higher `occupation`, but fewer attacker losses.
5. Guns damage battleships more than corvettes; no ship damage while waiting once the station is gone.
6. Taking the Generator drops the shield; a cut-off landing force is gone within 95 days and zones reset.
7. Conquest: state stakes move, private stay and count as foreign; losing the capital relocates it; losing the last planet ends the polity; `money_drift == 0` daily; `check_invariants` passes.
8. Termination: pseudo-random orders (`Rng::stream(seed, Domain::Siege, …)`) never produce a siege older than the bound under constant control.
9. Skipped vs. exact ticks and save/load mid-siege give equal hashes.
10. Sweep, 32 seeds with forced wars: `SiegeStats` in `RunSummary`; conquests and lifted sieges both occur; `ai_rejected` below 5 %.

## 9. Risks

- **M4 interfaces still moving.** Only `siege/orbit.rs` touches `Fleet`, `Ship` and `tech` (`orbital_control`, `bombard_power`, `apply_hull_damage`, `spend_ammo`, `troop_capacity`, `effects`, `estimate`); the resolver sees an `OrbitView`.
- **Capital loss and extinction are new paths.** Nothing in `crates/sim` sets `Polity.alive = false` or moves `capital` today; `transfer_planet` is the single entry and is tested on its own.
- **Starvation by rule.** Garrisons cannot restock under hostile orbit even when the system market is full; deliberate, it guarantees termination on self-sufficient worlds.
- **Snowballing.** Damped by militia, stores, `occupation`, garrison upkeep, foreign-ownership pull and weariness; watched in sweeps.
- **Bombardment dominant or useless.** Levers are the two level tuples and the kill rates; test 4 pins both sides.
- **Tails and oscillation.** Thresholds end every geometric decay, values below 1e-6 snap to 0; the AI keeps one saved plan per polity and commands only on change.
- **`Market.standing`** for munition and waffensysteme may also be reset by M4 yards: agree on one owner.

Memory: no stored note was used.

### Critical Files for Implementation
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/state.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/fleet/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/polity/mod.rs