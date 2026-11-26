# KONFLUENZ M5 – Planetary siege

## 1. Core idea

1. A siege is one record per planet on top of a few standing defence numbers; the five phases are conditions on that state, not a script, so relief, re-investment and withdrawal underground move it both ways.
2. The attacker trades time, troops and prize against each other on three roads: starve (slow, no direct damage), assault (weeks, costs troops) or bombard (fast, at a published exchange rate against capital stock, population and later cohesion).
3. The defender buys time in peace (stations, guns, shield, bunkers, regulars, depot days) and spends it in war: fire or save supplies, fight forward or go deep, relieve or cover home, hold or capitulate.
4. Everything is physical: equipment and supplies are goods bought on markets, and every round fired at a planet is missing when the relief fleet arrives.
5. Conquest never resets ownership: prior shareholdings decide how hard a planet resists and how it coheres afterwards, so buying in is the cheap road to merger and bombardment the costly road to extinction.

## 2. State

New module `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/siege/` (all other paths are relative to that repository root): `mod.rs` (state, read model), `orbit.rs` (sole adapter to M4 `fleet::Ship`, `tech::Research`), `ground.rs` (pure combat math), `conquest.rs`, `plugin.rs`. `sieges: Sieges` joins `sim_state!` (`crates/sim/src/state.rs`). Planet parts are zones; `finance::Region` already means a planet.

```rust
struct Sieges { defenses: Vec<PlanetDefense>,              // per planet
                active: Vec<Siege>,                        // ≤1 per planet, sorted
                embarked: BTreeMap<FleetId, GroundForce> } // troops aboard fleets
struct GroundForce { strength: f64, supply: f64 }          // supply in strength-days
struct DefensePlan { guns: u8, stations: u8, shield: u8, bunkers: u8, // target levels 0..=5
                     regulars: f32, depot_days: u16 }
struct PlanetDefense {
    plan: DefensePlan,
    guns: f32, stations: f32,   // intact levels
    shield: u8, charge: f32,    // charge 0..shield·3000
    bunkers: u8,
    regulars: f64,              // paid, mobile
    militia: f64,               // free, zone-bound
    depot: f64,                 // strength-days
    posture: Posture,           // Vorwaerts | Tiefe
    hold_fire: bool,
}
enum ZoneKind { Hauptstadt, Stadt, Land, Batterie, Schildwerk, Untergrund }
struct Zone { kind: ZoneKind, share: f32,  // of population and capital
              militia: f64, attacker_held: bool, damage: f32 }
struct SiegeOrders { fire: Fire,           // Keins | Stuetzen | Gezielt | Flaechig
                     assault: bool, focus: u8, sack: bool }
struct Siege {
    planet: PlanetId, attacker: PolityId, defender: PolityId,
    started_day: u64, fleets: Vec<FleetId>,
    orders: SiegeOrders, zones: Vec<Zone>,
    army: GroundForce, landed_day: u64,
    invested: bool, hunger_days: u16,
    prior_share: f64,           // attacker-side holders' share of local firms
    prize_start: f64,
    report: SiegeReport,        // yesterday's losses, ammunition, damage
    outcome: Option<Outcome>,   // Fallen | Capitulated | Lifted
}
```

`finance::Region` gains `scar`, `scar_by`, `conquered_from`, `occupied_day`, `prior_share`, `besieged`. Zones are generated at `BeginSiege`: `[Hauptstadt, Land]` below 100 M inhabitants, one `Stadt` up to 1000 M, two above (shares: Hauptstadt 0.4, rest even); `Batterie`, `Schildwerk` (share 0.03) if guns or shield exist; `Untergrund` (share 0) if bunkers exist; militia is spread by share. Home worlds start with guns 1, regulars 10, depot 60. Parameters: `assets/data/balance.ron: siege` (`SiegeBalance` in `data/defs.rs`, checked in `GameData::resolve_and_validate`).

## 3. Algorithms

`A` landed strength; `ga`, `gd`, `wd`, `sh` multipliers (ground_attack, ground_defense, weapon_damage, shields) of attacker `_A` or defender `_D`. `F` = Σ ships hull × Σ dps of weapons except point defence (kinetic, missile only with `ship.ammo>0`); `B = F·wd_A`. Starting fleet F≈150, battleship ≈80. Prize = capital × `capital_cost` × base price of maschinen + 30 × population.

**Daily**, `SimSet::Siege`, `run_if(day_start)`, after SpaceCombat:

0. Presence. `superior` = assigned fleet docked, no fleet at war with the attacker docked there, no `Battle` running; else nothing acts from orbit and no supply reaches the army. Peace or a new owner → `Lifted`.
1. Stations [phase 1], unshielded: `stations −= F/2500`; ships take `stations·30·wd_D`. `invested = superior ∧ stations=0`, required for fire and landing; the M4 blockade does the economic work; assigned fleets burn 0.3% of their tank per day. ⇄ A fleet pinned for months against relief or home defence under a visible depot clock.
2. Ground guns [2] reach only low orbit (`fire ≥ Stuetzen` or a landing today), unless `hold_fire` or Batterie is lost: ships take `guns·30·wd_D` (×0.25 on an empty depot); `depot −= 20·guns`. Damage splits by signature, capital ships ×2: `ship.hull −= part/(design hull+shield+armor, × tech)`. Counter-battery at charge 0: `guns −= B·0.0003` (×2 at Gezielt). ⇄ Low orbit can fire and land but takes losses; firing back uses up the depot.
3. Shield [2]: cap `shield·3000·sh_D`, regeneration `shield·100·sh_D` per day while Schildwerk is held; captured → 0. At `fire ≥ Gezielt` with charge>0: `charge −= B`, nothing else is hit. Landings pass.
4. Bombardment [3], charge 0: `R = min(0.05, B·0.025/(population+50))`; military effect m, collateral c: Gezielt (0.6R, 0.04R), Flaechig (3R, R) – 15:1 against 3:1. Surface defenders ×(1−m); `planet.population` and every `capital[r]` ×(1−c), floor 5% of the starting population; `region.scar += c`; local issuers `shock ×= 1−c`; weariness +40c attacker, +20c defender. Untergrund is untouched; bombardment alone never transfers a planet. Ammunition per day and magazine: 1% (Stuetzen, station duel), 3%, 5%. ⇄ Speed against prize and magazines.
5. Ground combat [4], focus zone, `r=0.03`, noise ξ ∈ [0.85, 1.15] from `Rng::stream(seed, Domain::Siege, planet, day)`: `D = zone.militia+regulars`; `EA = A·ga_A·sA·beach·support`, `ED = D·gd_D·fort·sD`; under `assault` `loss_D = r·EA·ξ`, `loss_A = r·ED·ξ′`. `support = 1+min(1, 0.05·B/D)` only at fire ≥ Stuetzen and charge 0; `beach=0.6` for 3 days after landing; `sA`, `sD` = 0.5 without supplies; fort: Land 1.0, Stadt 1.3, Batterie 1.4, Schildwerk 1.5, Hauptstadt 1.8. Each strength lost removes 0.1% of the zone's share of population and capital. A holding attacker is attacked if Vorwaerts has `D·gd_D ≥ 2·A·ga_A` (fort 1.3 for the attacker).
 A zone falls at `D < 0.15·A`: Batterie → guns captured; Schildwerk → shield down; Hauptstadt → militia elsewhere ×0.5, depot above `1500·bunkers` becomes army supply, Untergrund opens. Regulars withdraw there (capacity `15·bunkers`) under Tiefe or at `ED < 0.35·EA`. ⇄ Where to land; meet the landing or save the regulars.
6. Untergrund [5]: `loss_D = r·min(A,8)·ga_A·sA`, `loss_A = r·min(D,8)·gd_D·(3+0.3·bunkers)·sD`; no support, no bombardment, no economic damage. Hauptstadt held and nobody below → `Fallen`. ⇄ Storm at roughly 4:1 or wait.
7. Supply: while invested or Hauptstadt is lost `depot −= militia+regulars`, otherwise +5% of target per day. Empty: defenders −2%/day, `hunger_days += 1`, at 30 → `Capitulated`. Army: `supply −= A·(assault ? 1 : 0.3)`, topped up from assigned fleets while superior; empty: −2%/day.

Scale: depot 60 falls to pure investment in ~90 days, depot 180 in ~210; storming 40 regulars in a level-3 bunker costs ~150 attackers and five months.

**Monthly**, `SimSet::Aftermath`: `militia = 0.7·√population·clamp(cohesion/60, 0.3, 1.3)`, ×(1−prior_share) at siege start. One level per kind towards the plan; level n costs n× base (Batterie: waffensysteme 6, legierungen 12; Station: rumpfsegmente 10, waffensysteme 6; Schild: elektronik 15, maschinen 8; Bunker: legierungen 30, maschinen 4). Regulars cost munition 1, waffensysteme 0.2 (≤2× militia); depot target `depot_days·(militia+regulars)`, a strength-day costs nahrung 0.02, munition 0.01, treibstoff 0.01. All purchases use a new `economy::buy_for_state` (treasury → `market.cash`, stock down, `demand_today`/`standing` up); nothing arrives while invested. Upkeep `3·regulars+15·levels` credits to the planet's households; unpaid regulars −5%.

**Conquest**, `conquest::resolve`, Aftermath:
1. `polity::transfer_planet` (new): owner changes; a lost capital moves to the most populous remaining world; a polity without planets is eliminated, treasury and bonds (`Bond.issuer`) pass to the conqueror, its fleets disband.
2. `finance::shares::seize_state_shares` (new): `State(old)` holdings in issuers seated there go to `State(new)`. Elite and Public holdings stay: locals now belong to the conqueror, their stakes in the old realm become foreign there; old-realm holders become foreign owners here.
3. Region: `owner_seen`, `since_day`, `occupied_day`; `cohesion = clamp(30+30·prior_share−25·scar, 5, 70)`.
4. Sack if ordered: 30% of household, firm and elite cash moves to the conqueror's treasury; scar +0.25; local shock ×0.7.
5. Depot and installations pass over; the army becomes `regulars`.

Capitulation: no sack, occupation penalty ×0.6. Retaking a world from `conquered_from` clears the occupation and lifts cohesion to ≥55.

## 4. Commands

New `SimCommand` variants (`CommandContext` gains `sieges`); rejections in German as in `crates/sim/src/command.rs`.

| Variant | Validation |
|---|---|
| `SetDefensePlan { planet, plan }` | owner; levels ≤5 |
| `SetGarrisonOrders { planet, posture, hold_fire }` | owner |
| `Capitulate { planet }` | owner; siege active |
| `EmbarkTroops { fleet, planet, strength, supply_days }` | own planet in the docked fleet's system; `strength ≤ regulars`; berths = hull segments × 0.5; supplies bought now, ≤100 days per berth; troops beyond remaining berths are lost with their ships |
| `DisembarkTroops { fleet, planet }` | own planet; not invested |
| `BeginSiege { fleet, planet }` | at war with the owner; `superior`; fleet in no other siege; one besieging polity per planet; own fleets may join |
| `SetSiegeOrders { planet, orders }` | attacker; focus = defender-held surface zone, or Untergrund once Hauptstadt is held; stored, acts when conditions hold |
| `LandTroops { fleet, planet, zone, strength }` | assigned fleet; invested; surface zone; outside `Land` active guns cost `min(0.3·wave, 0.5·guns)` |
| `LiftSiege { planet }` | re-embarks the army if superior (−10% while assaulting), else rejected |

## 5. AI

`crates/sim/src/ai/siege.rs` in `SimSet::Ai`, commands only. Both sides decide on `siege::forecast(view, option) → (days, own losses, prize loss)` – the numbers the interface shows – fed with `estimate()`d values.

Defender. Plan by planet value × hostile fleet value within three jumps, budget 15% of revenue (35% at war): capital guns 3, stations 2, shield 2, bunkers 2, regulars 0.5× militia, depot 180; chokepoints guns 1, depot 120; others depot 60. Besieged: Tiefe if bunkers exist and forecast `ED < 0.7·EA`; hold fire below 60 depot days unless a landing runs; capitulate if the fall precedes any possible relief. Relief: go if estimated own value ≥1.2× the besieger's and arrival precedes the fall, capital first; otherwise besiege the attacker's nearest world.

Attacker. Target = max `prize·(0.5+prior_share)/forecast cost` among reachable enemy planets, if liftable troops ≥1.3× estimated effective defenders or fleet endurance ≥ estimated depot days; recruit at the capital, embark with 90 supply days, `BeginSiege`.
- Fire: Keins while stations or guns would cost >1.5% of fleet toughness per day; Gezielt for counter-battery once the shield is down, against a shield only if `B > 2·regeneration` and magazines >60%; Flaechig only under time pressure (stronger relief before the fall, weariness >60, treasury below six months), never where own holders own >25%; keep 40% ammunition while an enemy fleet is within three jumps.
- Ground: land at `EA ≥ 1.5·ED`, into Land first if guns are active; focus Schildwerk while the shield stands, Batterie if guns hurt, else Hauptstadt; storm Untergrund only if forecast losses ≤ half the army and relief threatens.
- Lift at fuel <15%, before a stronger fleet, or to save the own capital. Sack when the treasury holds under three months of revenue.

## 6. Feedback

- Economy: state purchases raise demand and prices of munition, waffensysteme, nahrung in the buyer's system market, so arms firms earn. Bombardment and fighting remove `PlanetEconomy.capital` and population, never money: `state::money_drift` stays 0.
- Finance: `finance::shares::risk_discount` +0.25 while `region.besieged`; `shock` on bombardment and sack; `book_value` falls with capital, so attacker-side holders of local shares lose with every volley. New `Finance::add_weariness`: +0.05 per strength lost (both sides), +0.4/+0.6 per siege month (attacker/defender), +40c for bombardment; it drives the existing `bond_yield`, „Kriegsmüdigkeit“ in every home region and M4 battle morale.
- Cohesion (`CohesionFactors::contributions` in `crates/sim/src/finance/cohesion.rs`): new „Besatzung“ = `−25·(1−prior_share)·max(0, 1−years/20)`; new „Kriegsschäden“ = `−25·scar` while `scar_by` owns the planet (scar decays 1.5% per month in `update_cohesion`); „Aufbruchstimmung“ is zero while `occupied_day` is set. Existing „Fremder Besitz“ and `foreign_pull` now bite on conquered worlds: buy the old owners out (`TakeoverBid`), `Expropriate { scope: Foreign }`, or risk the region drifting back. A stake bought before the war shrinks the militia, softens occupation and can end the siege by secession to the attacker.
- Budget: upkeep and supplies drain `Economy.treasuries`; the bond rule in `crates/sim/src/ai/plugin.rs` finances the war at rising yields.

## 7. Interface

Read model `siege::view(world, viewer, planet) → SiegeView`: phase label (Annäherung, Einschließung, Beschuss, Bodenkrieg, Untergrund); zones (kind, share, holder, damage, defenders); shield charge, cap, regeneration; guns, stations; fleet firepower, hull, ammunition, fuel; army strength, supply days; yesterday's `SiegeReport`; prize now against start; forecasts for Aushungern, Sturm, Flächenbeschuss. Enemy garrison, depot days and bunkers are ranges `value·(1 ± 0.5·(1−intel))`, offset from `Domain::Intel`; approaching fleets show arrival day and estimated strength. Chronicle entries (`ChronicleKind::War`) are German.

Controls. Attacker: Feuerstufe (four steps), Landen (zone, strength), Schwerpunkt (click a zone), Sturm/Halten, Plündern/Schonen, Aufheben. Defender: Verteidigungsplan (six values with monthly cost), Vorwärts/Tiefe, Feuer frei/halten, Kapitulieren.

## 8. Tests

`crates/sim/tests/siege.rs`, unit tests in `ground.rs`:

1. Shield blocks bombardment: 15 days Flaechig against shield 2 and shield 0, same seed; with shield population, capital, guns, garrison equal the start bit for bit while charge and fleet ammunition fell; without it capital and population are lower, scar >0.
2. Underground gets no orbital support: the Untergrund round returns identical losses for F=0 and F=1000, for Stadt `loss_D` rises; over 30 days equal forces lose ≥2.5:1 underground, ≤1.2:1 in Land.
3. Starving out ends: Keins, no landing, depot 90, no relief → owner changes within 130 days; the siege reports show zero ground losses and zero removed capital. With a surviving station, or a relief fleet contesting the orbit (depot refills, army supply stops), it does not.
4. Bombardment lowers the prize: same seed and timing, 15 days Flaechig versus 15 days waiting before the same assault → with bombardment the prize at conquest is strictly lower and the assault shorter and cheaper; a year later cohesion and local share prices are lower.
5. Guns hit only in low orbit, capital ships hardest; a captured Batterie is silent.
6. Conquest: `State(old)` holds no local shares, `State(new)` gained them, old-realm elites appear in `ownership_of_region().foreign`; a 60% prior stake yields ≥10 more cohesion points after a year.
7. Daily through recruiting, embarking, upkeep, sack and a polity's elimination `money_drift == 0`; `harness::check_invariants` extended (strengths and depots finite, ≥0; one siege per planet; attacker at war with the owner); save/load mid-siege and skipped idle ticks give equal hashes.
8. AI sweep, seeds 0..16, war scripted in year 25: conquests in most seeds, no siege over four years, at least one lifted by relief; `harness::RunSummary` reports sieges begun, won, lifted, mean days, retained prize.

## 9. Risks

- M4 names drift: only `siege/orbit.rs` touches ships and technology; the rest works on F, B and multipliers.
- Endless sieges: both sides are finite (depot plus 30 days; magazines, fuel, army supply); the sweep reports the longest siege.
- Starving dominates: levers `depot_days`, siege weariness, fuel burn; a long blockade also erodes the prize through the existing economy; the real counter is the relief rule, hence the sweep metric.
- Depopulation: population floor, 5% daily cap; weariness and scar fall on whoever fires.
- Snowballing: occupation, scar, foreign pull, upkeep; thin militia on low-cohesion conquests makes reconquest cheap.
- Finished milestones: production untouched; finance gains two cohesion lines and one risk term.
- Hash and saves serialize every field with postcard and RON: no `skip_serializing_if`, maps only as `BTreeMap`.
- A polity can be eliminated before M6 defines endings: the minimal rule (assets and debts pass on) is covered by test 7.

### Critical Files for Implementation
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/state.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/fleet/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/assets/data/balance.ron