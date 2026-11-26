# KONFLUENZ M5 – Planetary siege: final implementation specification

Synthesis: kernel of C (one record, derived phases, pure resolver, bounded end), decisions of B (fire levels, defence plan, posture, capitulation, forecast), causal hooks of A (defence consumes goods, production disruption, stake-weighted weariness). Paths are relative to `/home/user/exp_bevy_stellaris_fable_onePrompt/`. Identifiers are English, player texts German. Code references were checked against `main`; the M4 worktrees were not read and nothing was compiled.

## 1. Core idea

1. One record per besieged planet on top of a few standing defence numbers. The five phases are predicates on that state, never a stored machine.
2. Three roads with a published exchange rate: starve (slow, prize intact), storm (weeks, costs troops), bombard (fast; military effect to collateral 15:1 „gezielt", 3:1 „flächig").
3. Defence is goods bought in peace and spent in war. Under hostile orbit the garrison is not restocked, so every siege ends; real shortage only changes the speed.
4. The day resolver is a pure function without randomness. Forecast, AI, interface and tests call the same function.
5. Conquest changes the owner, never private shareholdings. A prior stake weakens resistance and softens occupation; bombardment costs prize, cohesion and the attacker's own weariness.

## 2. State

New module `crates/sim/src/siege/`: `mod.rs` (data, read model), `step.rs` (resolver, forecast), `orbit.rs` (the only file touching `fleet::Fleet`/`Ship` and `tech::Research`), `conquest.rs`, `plugin.rs`. `sieges: Sieges` joins `sim_state!` (`crates/sim/src/state.rs`) and is built in `SimState::generate`. `SAVE_FORMAT` rises by one: new fields carry no serde defaults, postcard hashes every field.

```rust
struct Sieges {
    defenses: Vec<PlanetDefense>,                    // per planet, permanent
    active: BTreeMap<PlanetId, Siege>,               // at most one per planet
    assign: BTreeMap<FleetId, PlanetId>,             // a fleet besieges exactly one planet
    embarked: BTreeMap<FleetId, Embarked>,           // { troops: f64 (V), supply: f64 (V-days) }
    pending: Vec<Conquest>,                          // { planet, to, outcome, sack }
    budget: BTreeMap<PolityId, Money>,               // armament allowance left this month
    plans: BTreeMap<PolityId, AiPlan>,               // { target, staging, fleets, started_day }
    grievances: BTreeMap<(PolityId, PolityId), f64>, // (offender, wronged) → credits, read by M6
    stats: SiegeStats,                               // counts by outcome, days, prize kept, V lost
}
struct DefensePlan { stations: u8, guns: u8, shield: u8, bunkers: u8, regulars: f32, depot_days: u16 }
struct PlanetDefense {
    plan: DefensePlan,
    stations: f32, guns: f32,            // intact levels
    shield: u8, charge: f32, bunkers: u8,
    regulars: f64,                       // V; paid, mobile, raised from population
    militia: f64,                        // V; free, zone-bound
    depot: f64,                          // days of supply for the whole garrison
    posture: Posture,                    // Forward | Deep
    hold_fire: bool,
    scar: f64, scar_by: Option<PolityId>,             // war damage 0..1 and who caused it
    occupation: f64, occupied_from: Option<PolityId>, // 0..1
}
struct Siege {
    attacker: PolityId, defender: PolityId, started_day: u64,
    orders: SiegeOrders,                 // { fire: Fire, assault: bool, focus: u8, sack: bool }
    zones: Vec<Zone>,                    // { kind: ZoneKind, share: f32, militia: f64, taken: bool }
    army: f64, army_peak: f64, army_supply: f64, landed_day: Option<u64>,
    deep: f64,                           // regulars underground; reserve = regulars − deep
    garrison_start: f64, prize_start: f64,
    stakes: BTreeMap<PolityId, f64>,     // shares of local firm value, refreshed monthly
    report: SiegeReport,                 // yesterday: losses, damage, ammunition, blocked orders
}
enum Fire { Hold, Support, Precise, Area }           // ordered
enum ZoneKind { Outskirts, City, Center, Battery, ShieldWorks }
enum Outcome { Stormed, Starved, Capitulated, Lifted }
// resolver interface: plain data, hand-built in tests
struct PlanetFacts { population: f64, cohesion: f64, essentials_met: f64, forage: f64 }
struct OrbitView { control: bool, firepower: f64, ammo: f64, embarked: f64, att: TechView, def: TechView }
struct TechView { ground_attack, ground_defense, weapon_damage, shields, hull, armor: f64, gates: [bool; 4] }
```

Units. V („Verband") = `pop_per_strength` Mio. people. Firepower `F` = Σ over ships of hull fraction × Σ `WeaponDef::dps()` of weapons except point defence (kinetic and missile only with ammunition aboard): starting fleet 150, one „Bastion" 82.

Further fields:
- `economy::PlanetEconomy.disruption: f64`.
- `finance::Region.{besieged: bool, occupation, garrison_cover, war_damage: f64}`: mirrors written only by siege; the last three are copied into `CohesionFactors`.
- `Balance.siege: SiegeBalance` and `CohesionBalance.{occupation, occupation_hold, war_damage}` in `crates/sim/src/data/defs.rs`.

Zones are built when a record forms: `Outskirts`, `ShieldWorks` (shield > 0), `Battery` (guns > 0), `City` × (0 below 100 Mio., 1 up to 1000, 2 above), `Center`. Shares: Center `center_share`, each works `works_share`, the rest evenly. Militia is split by share.

## 3. Tick order and algorithms

**Schedule** (`siege/plugin.rs`). Each tuple is one `.chain()`; where M4 has systems in the same set, order against them explicitly, because `crates/sim/src/lib.rs` turns ambiguity into an error.
- `SimSet::Siege`, `day_start`: `siege_day`, `procure_day`.
- `SimSet::Aftermath`: `resolve_conquests` (`day_start`), `siege_month` (`month_start`).
- `SimSet::Ai`: `siege_defend`, `siege_plan` (month), `siege_conduct` (day), appended to the chain in `crates/sim/src/ai/plugin.rs`.

Nothing requests fine ticks. Commands act between day boundaries, hence the stored `landed_day`.

**Derived, never stored**
```
control  = an assigned fleet of the attacker is docked and not in battle,
           and no fleet at war with the attacker is docked in the system
invested = control ∧ stations = 0
will     = (will_base + (1−will_base)·cohesion/100) · (1 − will_stake·stake)     // stake = stakes[attacker]
phase    = Approach (stations > 0) | Investment | Bombardment (fire ≥ Precise)
           | Ground (army > 0) | Underground (surface lost)
```

**`siege_day`**

0. Lifecycle, the only place where records appear and vanish:
   - Drop `assign` entries whose fleet is dead, not docked in the planet's system, or no longer at war with the owner. Clamp `embarked.troops` to berths (`berth_per_segment` × `HullDef.segments` of living ships); the excess is lost. If M4 merges fleets, entries add up.
   - Close through `siege::close` (outcome Lifted) when owner ≠ defender (secession, `abandon_dead_colonies`), peace, attacker dead, or `¬control ∧ army = 0`. Surviving zone militia returns to `PlanetDefense.militia`.
   - Create, in FleetId order, for an assigned fleet with control and no record: `stakes` from `finance::ownership_of_region` (owner side = 1 − Σ foreign); `militia = militia_k·√population·clamp(cohesion/60, 0.3, 1.3)·(1−stake)`; zones; `deep = min(regulars, deep_per_bunker·bunkers)` under `Deep`, else 0; `garrison_start`; `prize_start` = Σ `finance::book_value` of the planet's firms + `prize_per_million`·population.
   - Station veto: remove from `Fleets.blockaded` every system in which a planet of `Galaxy::system_owner` has `stations > 0`. M4's blockade and the investment both need the stations gone.
   - `Region.besieged` = record exists.
1. Per siege in PlanetId order:
   - With `control` and an army ashore, unload `embarked.supply` of assigned fleets into `army_supply`.
   - If invested, the owner buys `forage_share` of the garrison's food on the system market; `facts.forage` = share obtained.
   - `step::resolve(&mut Siege, &mut PlanetDefense, &PlanetFacts, &OrbitView, &SiegeBalance) -> DayEffects`; the system applies the effects to ships, economy and finance.
2. Every planet: `disruption = max(target, disruption·(1−disrupt_decay))`, target = Σ share·(`disrupt.0` if fought over today, `disrupt.1` if taken).

**Resolver.** `ga`, `gd`, `wd`, `sh` = ground_attack, ground_defense, weapon_damage, shields of attacker `_A` or defender `_D`; `B = F·(1+wd_A)`; `a` = the `ammo` tuple; `sA`, `sD` = 1, or `unsupplied` when `army_supply` or `depot` is empty.

```
(1) stations, outside the shield; needs control:
      stations −= F/station_hp;   hull += stations·station_damage·(1+wd_D);   ammo += a.0
(2) ground guns reach low orbit only:   low = invested ∧ (fire ≠ Hold ∨ landed yesterday)
      guns_active = guns > 0 ∧ Battery held ∧ depot > 0 ∧ ¬hold_fire
      low ∧ guns_active:   hull += guns·gun_damage·(1+wd_D);   depot −= gun_depot·guns
(3) shield:   cap = shield·shield_cap·(1+sh_D);   full = shield·shield_regen·(1+sh_D)
      ShieldWorks taken: charge = 0
      else if depot > 0:   x = min(cap−charge, full);   charge += x;   depot −= shield_depot·shield·x/full
(4) bombardment; needs invested, fire ≥ Precise, ammunition, surface not lost:   ammo += a[fire]
      charge > 0:  charge −= B, nothing else is hit (report „Schild hält")
      else  R = min(bombard_cap, B·bombard_k/(population+bombard_pop0));   (m, c) = precise|area · R
            zone militia and reserve ×(1−m);   guns −= B·counter_battery·(2 if Precise)
            population ×(1−c), floor max(1 Mio., pop_floor·start);   every capital[r] ×(1−c)
            scar += c;   shock of local issuers ×(1−c)
(5) ground; zone z = Outskirts until taken, then orders.focus (if taken: next in list order):
      D  = z.militia + (reserve if Forward ∨ z = Center)
      support = 1 + support_max·min(1, B/(support_ref·D))
                if fire ≥ Support ∧ charge = 0 ∧ ammunition, else 1   (at Support a fighting day costs a.1)
      EA = army·(1+ga_A)·sA·beach·support;        ED = D·(1+gd_D)·fort[z]·will·sD
      assault:                                              loss_D = r·EA;   loss_A = r·ED
      sortie (¬assault ∧ Forward ∧ ED/fort[z] ≥ sortie_ratio·EA):   loss_D = r·EA;   loss_A = r·ED/fort[z]
      x = damage_per_loss·z.share·(loss_A+loss_D):   population, capital ×(1−x);   scar += x
      z falls when D < zone_fall·army
(6) underground; surface lost, deep > 0, assault; no support, no bombardment, no damage:
      loss_D = r·min(army, tunnel_front)·(1+ga_A)·sA·tA
      loss_A = r·min(deep, tunnel_front)·(1+gd_D)·(under_base+under_per_bunker·bunkers)·will·sD·tD
(7) supply:   invested: depot −= (1 − forage_share·forage)·(2 − essentials_met)    // underground: 1
      depot = 0:  all defenders ×(1 − starve_rate/will);   guns and shield regeneration stop
      army_supply −= army·(fought ? 1 : army_idle_supply);   empty: army ×(1−strand_rate)
      control:  fuel += station_fuel
```
`beach` applies for `beach_days` after a landing. Losses of `D` are shared pro rata by zone militia and reserve. Values below 1e-6 snap to 0.

Zone effects:
- Battery: guns silent, captured intact.
- ShieldWorks: shield down.
- Center: militia elsewhere ×`center_shock`; the reserve withdraws underground up to `deep_per_bunker·bunkers` (the rest disbands); stores beyond `bunker_days` become `army_supply`.
- Surface lost ⇔ `army > 0` ∧ (all zones taken ∨ zone militia + reserve ≤ `zone_fall·garrison_start`).

Verdicts, pushed to `pending`:
- Stormed: surface lost ∧ `deep = 0`.
- Starved: `depot = 0` ∧ defenders ≤ max(0.1, `capitulate_frac·garrison_start`) ∧ occupation force present, i.e. `army` + troops embarked on assigned docked fleets ≥ `occupy_k·√population` (they land without loss). Fire alone or a lone corvette never takes a planet.
- Landing repelled: `army ≤ zone_fall·army_peak`: army lost, zones revert, the record continues.

Ships (`orbit::apply`): each assigned ship loses the hull fraction `hull·g[class] / Σ_j g[class_j]·T_j`, with `T` = `DesignStats.hull + shield + Σ armor` under the attacker's technology; capital ships lose five times the fraction of corvettes. Ammunition: `ammo` × magazine capacity of every ship. Troops beyond the remaining berths are lost with their ships.

Weariness (`PolityFinance.war_weariness`; existing scale 0.6 per war month):
- `wear_loss` per own V lost;
- per Mio. inhabitants lost to fire: attacker `wear_fire.0·(1+wear_portfolio·stake)`, defender `wear_fire.1`;
- `wear_month` per siege month, `wear_planet` at conquest.
- Third parties: `grievances[(attacker, p)] += destroyed or looted credits · stakes[p]`.

Bound, checked in `GameData::resolve_and_validate` (`crates/sim/src/data/mod.rs`) together with key resolution, rates in (0, 1) and `fort ≥ 1`:
`depot_days_cap/(1−forage_share) + ln(1/capitulate_frac)/starve_rate ≤ max_siege_days` (400 + 95 ≤ 500). It holds under unbroken investment with an occupation force.

**`procure_day`**, planets not invested. New `economy::buy_for_state(market, treasury, budget, good, wanted, buy_share)`: buys min(`wanted`, `stock·buy_share`, budget, treasury above `population·state_reserve_per_million`) at `Market::ask`, pays into `Market.cash`, and adds the full `wanted` to `demand_today` (never `standing`). Daily: 1/30 of the depot gap (one day costs `(militia+regulars)·day_bundle`; when the garrison grows, `depot ×= old/new`), plus 1/30 of the bundles of missing regulars and levels as demand only.

**`siege_month`**: `budget[p] = budget_share·(treasury − reserve)⁺` (peace | war); capital first, then PlanetId:
- militia by the formula above with stake 0 (not while besieged);
- recruit towards `plan.regulars`, at most `recruit_per_month·militia + 1`; each V costs `regular_bundle` and `pop_per_strength` people (never below 2 Mio.);
- one level per kind towards the plan, all or nothing, level n for n × bundle, capped by technology; surplus levels and regulars go (people return up to `capacity`);
- upkeep `pay·regulars + level_upkeep·levels` from the treasury to the planet's households, for embarked and landed troops to the capital's (`economy::pay`); unpaid share: regulars −5 %;
- `scar` and `occupation` decay (below 0.01 → 0, `occupied_from = None`); mirrors to `Region`: `occupation`, `garrison_cover = min(1, regulars/(hold_k·√population))`, `war_damage = scar` if `scar_by` is the owner; `stakes` refreshed.

**`resolve_conquests`** (`conquest.rs`), per entry:
1. Read `stake` and the local issuers before the owner changes.
2. `polity::transfer_planet` (§6): all state shares to `State(new)`, `cohesion = clamp(conquest_base + stake_pts·stake − scar_pts·scar, 5, 70)`, `Stage::Restless`, shock ×`conquest_shock`.
3. `occupied_from = old`, `occupation = clamp(1−stake, 0.1, 1)`, × `capitulation_mercy` after a capitulation. Liberation (`new == occupied_from`): occupation 0, cohesion ≥ `liberation`, no sack.
4. `PlanetEconomy.founded_day = today`: the infant rule in `crates/sim/src/economy/capital.rs` keeps ruins from being scrapped.
5. Sack (ordered, not after capitulation): `sack_share` of household, firm and elite cash to the attacker's treasury; `scar += sack_scar`; shock ×`sack_shock`.
6. Installations and stores pass; the army becomes `regulars`; `plan` = current levels.
7. `fleet::update_blockades` again; `siege::close`; chronicle (`ChronicleKind::War`), e.g. „{Planet} ist ausgehungert und wird von {Gemeinschaft} besetzt."

Enclave rule: markets, blockade and freighter access stay per system (`Galaxy::system_owner`). A conquered minority planet shares the closure of its system until the main world falls or peace comes; the view says so, and the AI takes a system's most populous enemy planet first.

**Levers** (`assets/data/balance.ron`, new block `siege`):
```ron
siege: (
  // orbit
  station_hp: 2500, station_damage: 30, gun_damage: 30, gun_depot: 0.25, counter_battery: 0.0003,
  gun_weight: (0, 0.2, 0.4, 1, 1, 1),              // g per ShipClass
  ammo: (0.01, 0.01, 0.03, 0.05),                  // magazine share per day: station duel, Support, Precise, Area
  station_fuel: 0.003, berth_per_segment: 0.5, landing_loss: 0.04, landing_loss_cap: 0.4,
  // shield, bombardment
  shield_cap: 3000, shield_regen: 100, shield_depot: 0.1,
  bombard_k: 0.025, bombard_pop0: 50, bombard_cap: 0.05,
  precise: (0.6, 0.04), area: (3.0, 1.0),          // (military, collateral) × R
  pop_floor: 0.05,
  // ground, underground
  r: 0.03, beach: 0.6, beach_days: 3, unsupplied: 0.5, zone_fall: 0.15,
  fort: (1.0, 1.3, 1.8, 1.4, 1.5),                 // Outskirts, City, Center, Battery, ShieldWorks
  center_share: 0.4, works_share: 0.03, center_shock: 0.5,
  support_max: (0.3, 0.6), support_ref: 5, sortie_ratio: 2, damage_per_loss: 0.001,
  tunnel_front: 8, under_base: 3.0, under_per_bunker: 0.3, tunnel_tech: 1.25,
  deep_per_bunker: 15, bunker_days: 120,
  // supply, end
  forage_share: 0.4, starve_rate: 0.02, strand_rate: 0.02, capitulate_frac: 0.15,
  army_idle_supply: 0.3, depot_days_cap: 240, max_siege_days: 500,
  day_bundle: [("nahrung", 0.02), ("munition", 0.01), ("treibstoff", 0.01)],   // per V and day
  // forces, installations (level n costs n × bundle)
  militia_k: 0.7, pop_per_strength: 0.05, regular_bundle: [("munition", 1.0), ("waffensysteme", 0.2)],
  recruit_per_month: 0.1, regulars_cap: 3.0, occupy_k: 0.25, hold_k: 0.5, pay: 3, level_upkeep: 15,
  station: [("rumpfsegmente", 10), ("waffensysteme", 6)], station_max: 3,
  gun:     [("waffensysteme", 6), ("legierungen", 12)],   gun_max: 5,
  shield:  [("elektronik", 15), ("maschinen", 8)],        shield_max: (1, 3),   // without, with gate
  bunker:  [("legierungen", 30), ("maschinen", 4)],       bunker_max: (1, 3),
  gates: (bunker: "boden_2", support: "boden_3", tunnel: "boden_4", shield: "boden_5"),
  buy_share: 0.25, budget_share: (0.15, 0.35),
  home_start: (guns: 1, regulars: 10, depot_days: 60),
  // will, conquest, feedback
  will_base: 0.4, will_stake: 0.5,
  conquest_base: 30, stake_pts: 30, scar_pts: 25, liberation: 55, capitulation_mercy: 0.6,
  occupation_decay: 0.015, scar_decay: 0.015,
  sack_share: 0.3, sack_scar: 0.25, sack_shock: 0.7, conquest_shock: 0.8, elite_flight: 0.5,
  disrupt: (0.8, 0.3), disrupt_decay: 0.1, prize_per_million: 30,
  wear_loss: 0.03, wear_fire: (0.010, 0.005), wear_portfolio: 4, wear_month: (0.3, 0.5), wear_planet: (-3, 5),
  intel_spread: 0.5,
),
// finance.cohesion gains  occupation: 30.0, occupation_hold: 0.6, war_damage: 25.0
```

Technology gates (`TechView.gates`, known technologies of `techs.ron`): „Tiefbunkerbau" (bunker cap 1 → 3), „Orbitale Feuerleitung" (`support_max` 0.3 → 0.6), „Tunnelkriegführung" (`tA`, `tD` = `tunnel_tech` underground), „Schildgeneratoren" (shield cap 1 → 3).

## 4. Commands

New `SimCommand` variants in `crates/sim/src/command.rs`; `CommandContext` gains `sieges: ResMut<Sieges>`; rejections are German strings.

| Variant | Validation and effect |
|---|---|
| `SetDefensePlan { planet, plan }` | own planet; levels ≤ `station_max`, `gun_max`, gate caps; `regulars ≤ regulars_cap·militia`; `depot_days ≤ depot_days_cap` („Ausbaustufe nicht verfügbar") |
| `SetGarrisonOrders { planet, posture, hold_fire }` | own planet; posture changes only while the Center is held |
| `Capitulate { planet }` | own planet under siege; occupation force present („niemand da, der die Übergabe annimmt"); pushes `pending` |
| `EmbarkTroops { fleet, planet, strength, supply_days }` | own fleet docked in the system of an own planet, not in battle; planet not invested; `strength ≤ regulars`; free berths; `supply_days ≤ 120`, bought now from that market and `budget`, partial fill allowed |
| `BeginSiege { fleet, planet }` | own fleet docked in the planet's system; at war with the owner; no siege of another polity there („{Planet} wird bereits von {Gemeinschaft} belagert"); sets `assign`, replacing an earlier target |
| `SetSiegeOrders { planet, orders }` | siege with `attacker == me`; focus index valid. Orders are standing; what cannot act is listed in `report.blocked` |
| `LandTroops { fleet, planet, strength }` | own planet: fleet docked, no enemy control; troops join `regulars`, supply the depot. Enemy planet: fleet assigned, invested („keine Orbitalhoheit über {Planet}"), `strength ≤ embarked`; the wave loses `min(landing_loss_cap, landing_loss·guns)` if `guns_active`; sets `landed_day` |
| `LiftSiege { planet }` | attacker; the army re-embarks into free berths of assigned docked fleets, the rest is lost; with `army > 0` it needs control („ohne Orbitalhoheit kein Abzug"); clears `assign` |

`MakePeace` is rejected while the target besieges a planet of the issuer („{Gemeinschaft} belagert {Planet}: Frieden kann nur der Belagerer anbieten"). The attacker may always make peace; consent on both sides comes with M6.

## 5. AI behaviour

`crates/sim/src/ai/siege.rs`. All three systems skip `SimConfig.player` and issue a command only when the wanted value differs from the stored one.
- Enemy garrison, stores and bunkers are ranges `v·(1 ± intel_spread·(1−intel))`, offset from `Rng::stream(seed, Domain::Intel, viewer<<32|planet, month)`; the AI plans on the pessimistic bound and on `siege::forecast`.
- `Sieges::reserved(fleet)` (member of a plan) tells the M4e fleet AI to leave that fleet alone.
- Money comes only from `budget`, so share purchases (`steward_finances`), yards and armament cannot drain each other in one step.
- War and peace are not decided in M5; sweeps script them.

Defender, monthly:
1. threat(planet) = value of foreign fleets within 3 jumps (`Galaxy::hops_from`), enemies ×1, others ×0.25.
2. Plans: capital – stations 2, guns 3, shield 2, bunkers 2, regulars 0.5 × militia, 180 days. `chokepoint` systems and threatened planets in war – guns 1, bunkers 1, regulars 0.25 × militia, 120 days. Others – 60 days.
3. Besieged: `Deep` if bunkers exist and estimated `ED < 0.7·EA` in the Outskirts; `hold_fire` below 60 days of stores unless troops landed yesterday.
4. Relief: unreserved fleets worth ≥ 1.2 × the besieger move in, capital first; if none can, plan a counter-siege.
5. `Capitulate` only a non-capital planet, when no relief can arrive before the forecast fall and (stores < 15 days or will < 0.35).

Attacker, one saved plan per polity, kept until it ends; its stage is derived from `assign`, `embarked`, `active`:
1. Target: max `prize·(0.5+stake)/(1 + forecast own losses in credits)` among reachable enemy planets. Feasible if lift ≥ max(occupation force, 1.3 × Outskirts defenders), or fleet endurance ≥ forecast starving days with lift ≥ occupation force. Fleets: all docked at the capital except the weakest.
2. Muster at the capital: raise `plan.regulars` by the lift, `EmbarkTroops` with 90 days, `SetStance(Blockade)`, `MoveFleet`, on arrival `BeginSiege`.
3. Daily, pure `decide(&SiegeView) -> Decision`:
   - Station standing: stay if forecast hull loss ≤ 40 %, else lift.
   - Fire: `Hold` while guns would cost > 1.5 % of fleet toughness a day and no landing is due. `Precise` for counter-battery once the shield is down; against a shield only if `B` > 2 × regeneration and magazines > 60 %. `Area` only under time pressure (stronger relief before the forecast fall, or weariness > 60), never with stake > 0.25. Keep 40 % ammunition while an enemy fleet is within 3 jumps.
   - Land when forecast „Sturm" succeeds with ≤ 50 % army losses. Focus: Outskirts → ShieldWorks (if it wants to fire) → Battery (if it is being hit) → Center → Cities.
   - Otherwise starve, if the forecast days fit the fuel. Underground: hold; assault only if relief threatens and losses stay ≤ 50 %.
   - Sack if the treasury covers < 3 months of upkeep.
   - Lift at fuel < 15 %, before a fleet ≥ 1.2 × own, when the own capital is besieged, at weariness > 80 or `bond_yield > max_yield`.
4. After conquest: `plan.regulars` = hold need; next target in the same system.

## 6. Hooks into existing systems

- `crates/sim/src/lib.rs`: `pub mod siege;`; `SimPlugins` adds `siege::SiegePlugin` before `ai::AiPlugin`.
- `crates/sim/src/economy/production.rs::produce_and_consume`: `let staffed = capital * employment * (1.0 - local.disruption);`. Nothing else changes in production: idle works cut supply and wages; prices, `needs_met`, `grow_population`, profits and `reprice` follow.
- `crates/sim/src/economy/mod.rs`: `buy_for_state`. Every purchase of depot, troops and installations raises demand and prices of the arms goods.
- `crates/sim/src/finance/shares.rs`: `risk_discount` adds 0.25 if `finance.regions[seat].besieged` (hard-coded like its neighbours; signature unchanged). New `move_holdings(finance, from, to, fraction, issuers)`, because `Issuer::add`/`remove` are private to `finance`.
- `crates/sim/src/finance/cohesion.rs`:
  - `CohesionFactors::contributions` gains („Besatzung", `−occupation·occupation·(1 − occupation_hold·garrison_cover)`) and („Kriegsschäden", `−war_damage·war_damage`); „Aufbruchstimmung" × `(1 − occupation)`.
  - `update_cohesion` copies the three `Region` values into the factors.
  - `issuers_of_region` becomes `pub(crate)`; `apply_secession` calls `transfer_planet`.
- `crates/sim/src/polity/mod.rs`: `transfer_planet(galaxy, polities, economy, finance, logistics: &mut Logistics, data, planet, to, terms: TransferTerms { state_shares_to, state_share, cohesion, stage, shock }, day) -> Transfer { from, new_capital, eliminated }`, the single path from one owner to another.
  - Sets `Planet.owner`, `tax_exempt = false`, `Region.{owner_seen, since_day, stage, months_below, months_above, cohesion}`; moves state shares in the local issuers. Secession passes `(Public(planet), 0.5, base + honeymoon, Loyal, 0.85)`, today's behaviour.
  - Capital lost: `Polity.capital` = most populous remaining planet (ties: lower id); `TradeHouse.home` follows; that region is at most `Restless` and taxable. `elite_flight` of the holdings of `Elite(old capital)` in issuers on the loser's remaining territory (trade house included), and of its cash, move to `Elite(new capital)`. The rest of the centre's stakes now counts as the conqueror's „Fremder Besitz" in every remaining colony: intended merger pressure, metered by `elite_flight`.
  - No planet left: `alive = false`, `capital = None`.
- `siege::conquest::wind_up(dead, heir)`: new `diplomacy::drop_wars_of` (otherwise the victor stays at war for ever, `make_peace` rejects dead polities); treasury to the heir; `Bond.issuer = heir`; `State(dead)` holdings to `State(heir)`; trade house `polity` and `home` to the heir; fleets disbanded with their troops; its sieges and plan closed; `ChronicleKind::End`.
- `crates/sim/src/finance/plugin.rs::cohesion`: `ResMut<Logistics>`.
- Weariness needs no new consumer: `finance::bond_yield`, „Kriegsmüdigkeit" in every home region and M4 battle morale read it; the budget drains `Economy.treasuries`, `steward_finances` issues the bonds.
- Taxes: rule unchanged, `tax_exempt` untouched by the siege; disruption shrinks the base.
- `crates/sim/src/harness.rs`:
  - `check_invariants` adds: a living polity owns its capital; a dead one owns nothing and has no war; siege defender = owner and at war with the attacker; strengths and stores finite and ≥ 0; embarked ≤ berths; `disruption` in 0..1.
  - `RunSummary.sieges: SiegeStats`; `Scripted { day, polity, command }` issued as `CommandSource::Script`; `sweep_with(.., script)`; `border_war` (polity 0 declares war on the nearest capital's owner in year 25, offers peace in year 45).
- `crates/headless/src/main.rs`: `Command::Siege { seed, planet, days }` prints the daily report; `Sweep` gains `--war`.
- `orbit.rs` before M4d is merged (on `main` a `Fleet` is still the strength number s): F = 1.5·s, berths 0.175·s, hull 11·s, ammunition and fuel unlimited, `TechView` zero, intel 0.5.

## 7. Public API for the interface

`siege::view(refs, viewer, planet) -> Option<SiegeView>` and `siege::defense_view(refs, planet)`; pure; the AI reads the same structs. `siege::forecast(&SiegeView, Course) -> Forecast { falls, days, own_losses, hull_loss, prize_loss, limited_by }` iterates the resolver for „Aushungern", „Sturm", „Flächenbeschuss".

Show:
- phase („Anmarsch", „Einschließung", „Beschuss", „Bodenkrieg", „Untergrund"); enclave warning;
- planet: zones („Umland", „Stadt", „Zentrum", „Batterie", „Schildwerk") with share, holder, defenders, fort, focus; bunker level;
- orbit: fleets with firepower, hull, ammunition and fuel in days, berths, troops aboard; stations; guns (active, or why silent); shield charge, cap, regeneration;
- supplies: „Vorräte reichen N Tage" with contributions (Grundverbrauch, Markt vor Ort, Mangel, Schild und Geschütze); army supply days;
- „Durchhaltewille" with contributions (Zusammenhalt, Anteil des Belagerers), in the style of `CohesionFactors::contributions`;
- forces: army, reserve, underground, militia; ranges for the enemy;
- yesterday's `SiegeReport`, including blocked orders with reasons;
- „Wert der Beute" now against the start, split into Beschuss, Bodenkrieg, Plünderung; own stake;
- three forecasts; approaching hostile fleets with arrival day and value range;
- defence view: plan against levels, missing goods, monthly cost, armament budget, hold need.

Controls:
- Feuerstufe (kein Feuer, stützen, gezielt, flächig), Sturm/Halten, Schwerpunkt (click a zone), Plündern/Schonen → `SetSiegeOrders`;
- Belagern → `BeginSiege`; Einschiffen, Landen, Aufheben → `EmbarkTroops`, `LandTroops`, `LiftSiege`;
- Verteidigungsplan (six values) → `SetDefensePlan`; Vorwärts/Tiefe, Feuer frei/halten → `SetGarrisonOrders`; Kapitulieren → `Capitulate`.

Game side: `SimRefs` in `crates/game/src/ui/panels.rs` gains `sieges`; new `crates/game/src/ui/siege.rs`; actions leave as `UiAction::Command`.

## 8. Tests and sweep metrics

Resolver tests in `siege/step.rs` (hand-built inputs, no fleets):
1. `shield_blocks_bombardment`: shield 2, `B` 150, 30 days `Area` against a twin with `Hold`: population, capital, guns, defenders and scar identical, ammunition spent > 0. With shield 0: capital and population lower, scar > 0. With `B` 500 the charge reaches 0 and damage starts.
2. `underground_gets_no_orbital_support`: surface lost, `Support` with `B` 0 and `B` 1000: outputs bit-identical, ammunition 0. The same pair in a City: `loss_D` higher with support. Equal forces lose ≥ 2.5 : 1 underground, ≤ 1.3 : 1 in the Outskirts.
3. `starving_out_ends`: `Hold`, no landing, occupation force aboard, for depot ∈ {0, 60, 240} × forage ∈ {0, 1} × essentials ∈ {0, 1}: Starved within `max_siege_days`, attacker losses 0, capital and population untouched. Without occupation force: no verdict. Without control: stores do not fall.
4. `bombardment_lowers_the_prize`: 20 days `Area`, then storm, against 20 days of waiting and the same storm: prize at the fall strictly lower, fall earlier, attacker losses lower.
5. Termination: 200 random order sequences (`Rng::stream(seed, Domain::Siege, case, 0)`) under investment with occupation force all end within the bound.
6. Guns: a battleship loses ≥ 3 × the hull fraction of a corvette; no damage without low orbit, under `hold_fire`, with empty stores or a taken Battery.
7. A cut-off landing is gone within 100 days and the zones revert.
8. The forecast equals the resolver day for day.

World tests in `crates/sim/tests/siege.rs` (twin `Headless` runs on one seed, as in `crates/sim/tests/economy.rs`):
9. Starving by commands (`DeclareWar`, `MoveFleet`, `BeginSiege`, no fire): the owner changes within the bound, no ground losses. Twin with a station: `Fleets::is_blockaded` false, stores full. Twin with a relief fleet on day 50: Lifted, stores refilled against payment.
10. Twelve months after test 4 in the world: cohesion and Σ market value of the local issuers lower, attacker's `war_weariness` higher.
11. Conquest: `State(old)` holds no local shares, `State(new)` has them; private holders of the old realm appear in `ownership_of_region(..).foreign`; stage Restless; „Besatzung" negative. A 60 % prior stake gives militia × 0.4 and ≥ 15 points more cohesion at conquest.
12. Capital fall: capital and trade-house home move; `Elite(new capital)` gains exactly `elite_flight` of each holding; the new capital is not Autonomous; the conqueror's foreign share in a remaining colony is > 0.
13. Elimination: `alive = false`, the victor is not at war, treasury and bonds passed, no fleets left; `money_drift == 0` on every day, sack included.
14. Peace: the besieged defender's `MakePeace` is rejected, the attacker's accepted and the siege Lifted.
15. Disruption: ground war in the Center cuts the planet's output below 0.7 × twin; ≥ 0.9 × sixty days after the lift.
16. Determinism: save and load mid-siege, skipped against exact ticks, same seed twice give equal hashes; all existing tests stay green.

Sweep (`cargo sweep --seeds 0..32 --years 60 --war`): no violation; a conquest in at least half the seeds; Stormed and Starved both occur, Starved 20–70 % of conquests; no siege older than four years; `ai_rejected/(ai_applied+ai_rejected) < 5 %`. Reported: sieges by outcome, mean and longest days, prize kept, V lost per side, plan fulfilment at the start of the war, secessions to the conqueror within ten years of a capital fall.

## 9. Implementation order

1. Skeleton: `SiegeBalance`, RON block, validation; `Sieges` in `sim_state!`; the new `Region`, `CohesionFactors`, `PlanetEconomy` fields at neutral values; empty `SiegePlugin`; invariants; `SAVE_FORMAT`. No behaviour changes.
2. Ownership path: `transfer_planet`, `move_holdings`, `drop_wars_of`, `wind_up`, `apply_secession` on top of it; tests 12 and 13 by direct calls.
3. Resolver and forecast; tests 1–8.
4. `orbit.rs`, `siege_day`, lifecycle, station veto, the siege commands, `resolve_conquests`, the production, risk and cohesion hooks; tests 9–11 and 14–16.
5. `procure_day`, `siege_month`, plan commands, start values of the home worlds.
6. Read model, headless `siege`, harness script, `RunSummary`.
7. AI: defender, then attacker; sweep and tuning.
8. Game: siege view.

## 10. Risks and safeguards

- M4 names drift: only `orbit.rs` touches ships and technology; resolver tests need neither.
- Starving dominates: levers `depot_days`, `station_fuel`, `home_start`, `occupy_k`; the sweep reports the Starved share.
- Snowball after a capital fall: `elite_flight`, occupation, hold garrison; metered by the secession count.
- Colonies without regulars fall within days: `militia_k`, the defender's regulars rule; watched in sweeps.
- Arms goods are scarce: the Arms sector starts without capital, munition needs spaltstoffe that garden worlds lack, and a Kontor holds at most 60 units of an undemanded good. Budget and `buy_share` keep yards alive; plans fill over years.
- Ammunition is a limit of time and resupply, not a budget item (the starting magazines equal about 9 units of munition at `military.ammo_goods` 0.02). The lever lies with M4.
- Shared system market: sister worlds feed the population, the garrison only up to `forage_share`.
- Stations and guns do not take part in an M4 `Battle`; a relief battle ignores them. Deferred.
- About 75 levers: tune first `militia_k`, `r`, `fort`, `bombard_k`, `depot_days`, `station_hp`, `starve_rate`, using the daily report of `cargo headless siege`.

### Critical Files for Implementation
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/polity/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/fleet/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/assets/data/balance.ron

Memory: nothing from memory was used. The one stored note (checking the game window in a cloud container) does not bear on this task and was not opened. Possibly worth saving, learned from the repository: on `main`, `crates/sim/src/fleet/mod.rs` still models a fleet as one strength number; `finance::shares::risk_discount` takes no `GameData`, so its risk terms are hard-coded; `SimCommand::MakePeace` is unilateral.