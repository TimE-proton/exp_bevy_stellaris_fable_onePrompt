# M5 Planetary siege – causal-linkage design

## 1. Core idea
1. A siege has no hit points of its own: every phase consumes or destroys quantities that already exist – `Market.stock`, `Planet.population`, `PlanetEconomy.capital`, ship hull and ammunition, treasury money, shares.
2. A planet's regions are its economy: one zone "Stadt" plus one zone per `Sector` holding capital, so fighting or bombing a zone idles or destroys exactly that sector, and its share price follows through the existing `reprice`.
3. Defences are goods: guns fire `munition`, shields burn `treibstoff`, bunkers live off a depot bought on the market in peacetime; a blockade defeats them through the ware, and a self-sufficient planet cannot be starved until its farms or fuel chain are taken or destroyed.
4. One integrator, `resolve` (Durchhaltewille), starts at the planet's cohesion reduced by the attacker's ownership stake and is drained by hunger, missing ammunition, bombardment and lost zones; at zero the planet capitulates.
5. Conquest changes the owner but not private shareholdings, so the existing cohesion model decides whether the prize integrates or drifts back – bombardment costs the attacker twice, in capital and in cohesion.

## 2. State
Module `crates/sim/src/siege/` (`mod.rs`, `orbit.rs`, `ground.rs`, `conquest.rs`, `plugin.rs`; `SiegePlugin` in `SimPlugins`), resource `Sieges` in `sim_state!` (`state.rs`).

```rust
enum ZoneKind { City, Mining, Agrar, Industry, Arms }  // City + data::Sector; index of [_; 5]
struct PlanetDefence {        // Sieges.planets[PlanetId], permanent
    guns: [f32; 5],           // orbital gun levels per zone
    shield: [f32; 5],         // generator levels per zone
    stations: f32,            // orbital, outside the shield
    bunkers: f32,             // deep facilities
    depot: [f64; 3],          // nahrung, munition, treibstoff (goods units)
    garrison: f64,            // Korps; 1 Korps = 1 Mio. people taken from `population`
    orders: Vec<BuildOrder>, sortie: bool, shield_on: bool,
    occupation: Option<Occupation>,   // { from, day, grudge }
}
struct Siege {                // Sieges.active: BTreeMap<PlanetId, Siege>
    attacker: PolityId, defender: PolityId,
    phase: Phase,             // Blockade | Orbit | Ground | Underground
    fleets: Vec<FleetOrders>, // low orbit: { fleet, bombard: Off|Precise|Area, aim: Zone(z)|Stations, support }
    army: f64,                // landed Korps
    control: [f32; 5],        // 0 defender … 1 attacker
    taken: Vec<ZoneKind>, focus: ZoneKind, assault: bool, levy: f32,
    resolve: f64,             // 0..100
    grudge: f64,              // 0..1 destroyed share of capital and population
    cover: f32,               // shield cover today
    tally: Tally,             // losses, ammunition, credits per side; prize at start
}
```
`Sieges` also holds `embarked: BTreeMap<FleetId, f64>` (Korps aboard, ≤ Σ `transport[class]`) and `pending: Vec<Conquest>`.

Derived, never stored: zone capital `K_z` = Σ `capital[r]` with `recipe.sector == z`; zone population `P_z = population·w_z` (`w_City = city_share`, rest ∝ labour demand); a zone is active if it is City, has capital or holds installations. One new economy field: `PlanetEconomy.disruption: [f64; 4]`. Levers: `balance.ron: siege` → `SiegeBalance` (`data/defs.rs`), checked in `GameData::resolve_and_validate`.

## 3. Algorithms
`siege_day` in `SimSet::Siege`, daily, in `PlanetId` order, fixed number of draws from `Rng::stream(seed, Domain::Siege, planet, day)`. Indices A/D: `TechEffects` of attacker/defender.

**(1) Superiority, blockade.** A `Siege` opens when the attacker's blockade covers the system (`Fleets::is_blockaded`), a fleet gets siege orders or troops land; `resolve = Region.cohesion·(1 − stake)`, `stake = ownership_of_region(..).foreign[attacker]`. `sup` = attacker fleet docked, no defender fleet docked, no `Battle` there; without it no bombardment, support or ammunition for the army. It closes on peace or after `lift_days` without `sup` and army. Stand-off blockaders are out of gun range; only `Siege.fleets` are in low orbit.

**(2) Defence.** The defender requisitions from the system market at `Market::ask` (treasury → `Market.cash`, need added to `demand_today`; an empty treasury buys nothing), shortfall from `depot`.
- Shield, if `shield_on` and a fleet is in low orbit: `L` = Σ `shield[z]` over zones not taken, `fuel = shield_fuel·L`, `cover = min(1, L·(got/fuel)·(1+shields_D) / max(1, population/shield_pop_ref))`.
- Guns: `G = stations + Σ guns[z]` (zones not taken), need `gun_ammo·G`; `H = gun_damage·G·(got/need)·(1+weapon_damage_D)·rng(0.8..1.2)` hull points per day on ships in low orbit, split ∝ hull `signature` (capital ships absorb most), divided by `1+(hull_A+armor_A)/2`.

**(3) Bombardment** (needs `sup`). `B = Σ bombard[class]·hull fraction` over ships with ammunition; cost `B·bombard_ammo[level]` from `Ship.ammo`, charged even when the shield holds. `E = B·(1−cover)·(1+weapon_damage_A)`, `x = E/(E + bulk_base + bulk_per_capital·K_planet)`. Per day in the aimed surface zone: `capital[r] *= 1 − cap·x`, `population −= P_z·pop·x`, `guns[z] *= 1 − gun·x`, garrison `−= D·w_z·troop·x/fort[z]`; aim `Stations` ignores the shield. `grudge` += destroyed share, `resolve −= r_bomb·100·`share. Bunkers, depot and underground garrison are never affected.

**(4) Ground war** in `focus`, if `assault` or `sortie`; the shield stops fire, not landing craft.
```
front = front_base + front_per_pop·P_z
PA = min(A, front)·(1+ground_attack_A)·s_A·(1 + support·min(1, S/min(A, front))·(1−cover))
PD = min(D + militia_q·M, front)·(1+ground_defense_D)·s_D·fort[z]
lossA = k·PD·rng    lossD = k·PA·rng
control[z] += advance·(PA−PD)/(PA+PD)        // gains only with assault, losses only with sortie
```
`A` = `army`, `D` = `garrison`, `s = 1` if supplied, else `unsupplied`. The attacker draws `troop_ammo` per engaged Korps and `support_ammo·S` (`S = B` of supporting fleets) from ships in low orbit; the defender buys `troop_ammo` as `munition`. Militia `M = population·militia_share·cohesion/100·essentials_met`. `control ≥ 1`: zone taken, its guns and generators stop, next focus. `control ≤ 0`: the front falls back one zone; from the last one the army re-embarks or is lost. A landing wave loses `min(0.5, landing_loss·G)` and starts its zone at 0.2.

**(5) Underground**, once all active surface zones are taken and `bunkers > 0`: `D = min(D, bunker_capacity·bunkers)`, the rest returns to `population`. Only the depot supplies: food for `D`, `bunker_fuel·bunkers`, ammunition when fighting. Assault: `front = tunnel_front`, no support term, `PD·underground_mult`. Without `assault` nothing is fought (Halten); once a depot good is empty the garrison is unsupplied and `resolve` falls by `r_depot`.

**Resolve** (surface): `Δ = −r_hunger·(1−essentials_met) − r_cut·[garrison fights unsupplied] − r_zone·Σ w_z(taken) + r_relief·[defender fleet in system]`; `resolve ≤ 0` ⇒ capitulation ⇒ `pending`.

**Coupling.** `disruption[sector]` = `disrupt_contested` (focus), `disrupt_occupied` (taken), plus today's destroyed share; ×0.9 per day afterwards. Daily, open orders and the depot target (`depot_days·bunkers` days of use) buy 1/30 of their remaining need; monthly pay `troop_pay_days·wage` per Korps, treasury → households.

```ron
siege: (
  city_share: 0.4, shield_pop_ref: 1500, shield_fuel: 2.0, gun_ammo: 0.2, bunker_fuel: 0.5,
  gun_damage: 20, landing_loss: 0.04,
  bombard:   (0, 0.5, 1, 3, 8, 4),     // per ShipClass
  transport: (0, 0.5, 1, 3, 6, 8),     // Korps per ShipClass
  bombard_ammo: (1.5, 4), support_ammo: 1, troop_ammo: 1,  // ammo points; goods via military.ammo_goods
  bulk_base: 20, bulk_per_capital: 0.2,
  precise: (cap: 0.002, pop: 0.0002, gun: 0.15, troop: 0.02),
  area:    (cap: 0.03,  pop: 0.004,  gun: 0.10, troop: 0.03),
  fort: (1.6, 1.2, 1.0, 1.3, 1.3),     // City, Mining, Agrar, Industry, Arms
  front_base: 2, front_per_pop: 0.004, tunnel_front: 1.5,
  k: 0.01, advance: 0.05, support: 0.5, unsupplied: 0.3,
  militia_share: 0.005, militia_q: 0.3, max_mobilisation: 0.05,
  bunker_capacity: 5, underground_mult: 3.0, depot_days: 180,
  r_hunger: 0.5, r_cut: 0.5, r_zone: 0.2, r_relief: 0.5, r_bomb: 1.0, r_depot: 2.0,
  disrupt_contested: 0.8, disrupt_occupied: 0.4, troop_pay_days: 30,
  korps:   [("waffensysteme", 0.5), ("munition", 1)],
  guns:    [("waffensysteme", 2), ("maschinen", 1)],
  shield:  [("waffensysteme", 3), ("elektronik", 2), ("maschinen", 2)],
  station: [("rumpfsegmente", 4), ("waffensysteme", 2)],
  bunker:  [("legierungen", 10), ("maschinen", 2)],
  conquest_base: 40, grudge_pts: 25, stake_pts: 30, levy_pts: 20,
  occupation_pts: 30, occupation_years: 20, garrison_hold: 0.003,
  wear_korps: 0.1, wear_bombard: 2.0, wear_portfolio: 1.0, wear_planet: 5,
  siege_risk: 0.25, conquest_shock: 0.8, lift_days: 5, ai_patience: 720,
)
```

## 4. Commands
```rust
BuildDefence     { planet, zone: ZoneKind, work: Work, levels: u8 }  // Guns | Shield | Station | Bunker
RaiseTroops      { planet, korps: f32 }    DisbandTroops { planet, korps: f32 }
EmbarkTroops     { fleet, planet, korps: f32 }
LandTroops       { fleet, planet, zone: ZoneKind, korps: f32 }
SetSiegeOrders   { fleet, planet: Option<PlanetId>, bombard: Bombard, aim: Aim, support: bool }
SetGroundOrders  { planet, assault: bool, focus: ZoneKind, levy: f32 }
SetDefenceOrders { planet, sortie: bool, shield_on: bool }
Capitulate       { planet }
```
Validated in `command.rs::execute` (`CommandContext` gains `sieges`), rejections in German:
- `BuildDefence`: own planet, active zone, no enemy Korps landed; the order waits for goods.
- `RaiseTroops`: own planet, garrison ≤ `max_mobilisation·population`; raises what the market's equipment allows, else „keine Ausrüstung am Markt". `DisbandTroops` returns Korps to `population`.
- `EmbarkTroops`: own fleet docked in the system, own garrison or landed army, free capacity.
- `LandTroops`: own planet (no enemy `sup`) → garrison; enemy planet → war, `sup`, active zone, no third besieger.
- `SetSiegeOrders`: own docked fleet, war with the owner; `Some` enters low orbit and sets `Stance::Blockade`, `None` returns to stand-off.
- `SetGroundOrders`: siege attacker; focus active, not taken; `levy` in 0..0.5.
- `SetDefenceOrders`, `Capitulate`: planet owner; the latter needs an active siege.

## 5. AI
`crates/sim/src/ai/siege.rs`: `fortify`, `plan_sieges` (monthly), `conduct_sieges` (daily) in the `AiPlugin` chain. Enemy garrison, depot, bunkers and tech are known only as ranges (`siege::estimate`, widened by `1 − intel`); the AI plans on the pessimistic bound.

Attacker
1. Target: maximise `prize_value / (expected hull loss + Korps loss + upkeep·t)`; `prize_value = Σ book_value(firms) + 5 years of wage tax`.
2. Never land without `sup`; start at stand-off.
3. `t_starve = stock days + resolve/(r_hunger·(1−autarky))`, autarky = local food output / need. If `t_starve ≤ ai_patience` and no stronger enemy fleet within three jumps: wait.
4. Else with `cover ≥ 0.5`: no bombardment; land on the generator zone if Korps ≥ 1.5·`D·fort`, otherwise wait or leave.
5. Shield down: Precise on the zone with most guns until gun damage < 2 % of fleet hull per day, then land; order generator → Agrar (if autarkic) → Arms → Industry → Mining → City.
6. Area only as a last resort (own weariness > 60 after `ai_patience`), never with `stake ≥ 0.1`.
7. Underground: Halten, unless relief is expected and `PA ≥ PD`. Fleets below 25 % ammunition rotate home; abort at weariness > 80 or `bond_yield > max_yield`.
8. After conquest: garrison `garrison_hold·population`; `ai/plugin.rs::steward_finances` already hands seized state shares to `Public(planet)`; `Expropriate{Foreign}` only if foreign share ≥ `foreign_pull` and `expropriation_risk < 0.2`, else `TakeoverBid`.

Defender
1. Peace: 15 % of treasury above reserve on planets ranked by `prize_value·threat` (capital, chokepoint, border, war): bunker and depot → guns → shield → station.
2. Garrison target `population·(0.3 % + 1 %·threat)`.
3. Besieged: shield on only while a fleet is in low orbit; `sortie` if `PD` without fort ≥ 1.5·`PA`; `Capitulate` a non-capital planet when relief is impossible and `resolve < 15`, saving capital stock and private shares.

## 6. Feedback
**Economy**
- `economy/production.rs::produce_and_consume`: `staffed = capital·employment·(1 − disruption[sector])`. Idle works cut supply, prices rise, unpaid wages lower `needs_met`, `grow_population` shrinks the planet.
- Every military consumption is a market purchase (shield fuel, ammunition, equipment, installations, depot; ship ammunition via M4 resupply): arms prices, profits and share prices rise in war. Recruits leave `Planet.population`, pay returns to households; no new money account, so `state::money_drift` is untouched.

**Finance**
- `finance/shares.rs::risk_discount`: `+ siege_risk` for issuers on a planet in phase ≥ Orbit; `reprice` follows lower earnings and `book_value` by itself.
- `PolityFinance.war_weariness`: `+ wear_korps` per Korps lost; defender `+ wear_bombard` per percent of its population lost, `+ wear_planet`; attacker `+ wear_portfolio` per percent of its holders' share wealth destroyed (bombing one's own investments). Weariness already drives `finance/bonds.rs::bond_yield`, the `weariness` cohesion factor of every home region and battle morale.
- `tax_exempt` while City is taken lowers `revenue` and raises `bond_yield`; `steward_finances` issues the war bonds.

**Conquest** (`siege/conquest.rs` in `SimSet::Aftermath`; `transfer_region` extracted from `finance/cohesion.rs::apply_secession`)
- `Planet.owner = attacker`; `Elite(planet)`, `Public(planet)` follow via `polity_of_holder` and keep all holdings; the landed army becomes the garrison.
- Shares of `State(old)` in the planet's issuers pass to `State(attacker)`. Private holders of the old realm keep theirs and count as `Ownership.foreign`: „Fremder Besitz" and `foreign_pull` can carry the planet back.
- Loot: `levy·`(firm cash + `Finance.elites[planet]`) → attacker treasury; if `system_owner` changes, `levy·`market stock moves to the attacker's capital market; installations and depot pass over. Damage is what bombardment removed: `book_value`, earnings, tax base.
- `Region`: `owner_seen`, `since_day`, `stage = Restless`, `cohesion = conquest_base − grudge_pts·grudge + stake_pts·stake − levy_pts·levy`; issuers `shock *= conquest_shock`; `founded_day = today` reuses the infant rule of `economy/capital.rs`, so ruins are not scrapped.
- New `CohesionFactors.occupation = (0.3 + 0.7·grudge)·(1 − years/occupation_years)·(1 − 0.5·min(1, garrison/(population·garrison_hold)))`, contribution „Besatzung" `−occupation_pts·occupation`; no „Aufbruchstimmung" meanwhile.
- A lost capital moves `Polity.capital` to the most populous remaining planet and `TradeHouse.home` with it; with none left `alive = false`, treasury and state shares pass to the conqueror, its bonds are written off.

## 7. User interface
`siege::view(world, planet, observer) -> SiegeView`, pure, enemy values as intel ranges:
- Planet: zones („Stadt", „Bergbaurevier", „Agrarland", „Industriegürtel", „Rüstungswerke") with control, capital, disruption, population, guns, generators, fort, focus, aim.
- Orbit: fleets per side (stand-off or low orbit), ships by class, hull, ammunition days, bombard power, gun damage today, stations.
- Shield and supplies: cover, levels, fuel per day; market days of Nahrung, Treibstoff, Munition; autarky; depot days.
- Forces: Korps, militia, supply state, `PA`, `PD`, front, advance per day, losses.
- „Durchhaltewille" with contributions (like `CohesionFactors::contributions`), „Groll", „Wert der Beute" now and at start, costs of both sides, forecast days to capitulation.

Controls: Bombardement aus/gezielt/flächig, Ziel, Feuerunterstützung (`SetSiegeOrders`); Landen (`LandTroops`); Sturm/Halten, Schwerpunkt, Kontribution (`SetGroundOrders`); Abzug (`EmbarkTroops`); defender: Ausfall, Schild, Kapitulieren, Ausbau, Ausheben.

## 8. Tests
`crates/sim/tests/siege.rs`, twin runs on one seed as in `tests/economy.rs`:
1. `shield_blocks_bombardment`: cover 1, guns present, 60 days Area: `tally.capital_destroyed == 0`, population equals the control, ship ammunition fell, battleships lost a larger hull share to the guns than corvettes; with fuel in market and depot set to 0, capital falls within 10 days.
2. `underground_gets_no_orbital_support`: Underground, ten battleships in low orbit, twin A `support` on, B off: garrison, depot and attacker losses identical after 60 days; on a surface zone the same twins give A strictly more `control`.
3. `starving_out_ends`: colony with Agrar capital set to 0, stand-off blockade only: capitulation within 900 days, owner is the attacker, nothing destroyed, no Korps or hull lost; an autarkic planet under the same blockade has not capitulated.
4. `bombardment_lowers_the_prize`: conquest with and without Area: `prize_value` below 0.8×, cohesion at conquest at least 5 lower, wage tax two years later lower.
5. `conquest_keeps_private_shares`: `State(old)` holds 0, `Elite(old capital)` unchanged, `Ownership.foreign[old] > 0`, stage Restless, „Besatzung" negative; a 60 % attacker stake starts `resolve` at 40 % of the control.
6. Every day of a full siege with loot: `money_drift == 0`, `find_anomaly == None`; same-seed hashes equal; `skipping_idle_ticks_changes_nothing` still passes; attacker weariness and `bond_yield` exceed the peace control.
7. Sweep, 16 seeds × 80 years with scripted wars: new `RunSummary` columns (sieges by outcome, duration, destroyed capital share); a conquest in at least half the seeds, no siege over 15 years.

## 9. Risks
- M4 types still move: only `siege/orbit.rs` touches `Fleet`, `Ship`, `Research` (`superiority`, `bombard_power`, `spend_ammo`, `damage_ships`, `transport`).
- Shared system market: a neutral co-owner prevents starvation; `t_starve` uses real inflow, the view says so, the AI assaults instead.
- Endless sieges of autarkic planets: bounded by `ai_patience`, upkeep, monthly weariness, abort rules, sweep check.
- Balance cliffs (home worlds impregnable or trivial): frontage and transport tables are the main levers; `cargo headless siege --seed --planet` prints the daily tally.
- Ownership edge cases (capital lost, extinction, planet abandoned mid-siege by `abandon_dead_colonies`): one `transfer_region`/`close_siege` path; new invariant "every living polity owns its capital" in `harness::check_invariants`.
- Deferred: allied sieges, defender orbital support, stations as `Battle` participants.

_Memory: no stored note was used for this design; nothing new to save._

### Critical Files for Implementation
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/economy/production.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/fleet/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/shares.rs