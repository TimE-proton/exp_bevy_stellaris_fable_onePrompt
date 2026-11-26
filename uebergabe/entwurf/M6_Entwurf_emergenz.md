# KONFLUENZ M6: emergent diplomacy, hidden dispositions, endings

Paths are relative to `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/`.

## 1. Core idea

1. Each month every pair of communities is reduced to a few numbers read from existing state: power trend, military reach, wealth link (trade plus cross-held securities), cultural distance.
2. A hidden disposition is not a script but a slowly drifting mixture of four archetypes that weights one shared utility function; war, treaties, foreign share purchases and union are its arg-max.
3. Nobody reads a mixture: AIs and the player's analysis window run the same Bayesian assessment over a log of observable acts, so reputation, deception and misjudgement are mechanics.
4. Peace is a slope, not a plateau: capital crosses borders, one-sided stakes pull regions over (`foreign_pull`), mutual stakes plus contact mature into union, and diverging trends make the declining side strike early or flee into merger.
5. Depleting deposits shrink a restraint factor on every cost of leaving the status quo, so the dilemma cannot be postponed forever, and one `judge` function writes exactly one verdict.

## 2. State

```rust
// diplomacy/mod.rs: public, in sim_state! (today only `wars`)
struct Diplomacy {
    wars: BTreeMap<(PolityId, PolityId), War>,  // at_war()/enemies_of() keep their API
    pacts: BTreeMap<(PolityId, PolityId), Pacts>,
    overlord: Vec<Option<PolityId>>,            // Some = subjugated
    culture: Vec<[f32; 3]>,                     // per polity, -1..1, public
    survey: Survey,                             // exact; the UI reads it via diplomacy::view only
    beliefs: Vec<Vec<[f32; 4]>>,                // [viewer][subject] posterior over archetypes
    evidence: VecDeque<Evidence>,               // newest 600: day, actor, target, kind, needs_clarity
    proposals: Vec<Proposal>,                   // id, from, to, offer, expires_day
    via: Vec<Via>, absorbed_pop: Vec<f64>,      // per planet: how, with how many millions, it joined its bloc
    peak_population: f64,
    verdict: Option<Verdict>,                   // written once
}
struct War { aggressor: PolityId, aim: WarAim, start_pop: [f64; 2], taken: Vec<(PlanetId, PolityId)>, demand: [PeaceTerms; 2] }
enum WarAim { Tribute, Territory, Subjugation, Annexation }
enum TreatyKind { Trade, NonAggression, Research, Investment, Defence, Union }
struct Pacts { since: [Option<u64>; 6], ends: [Option<u64>; 6],  // day, by TreatyKind
               truce_until: u64, grievance_until: [u64; 2],
               tribute: Option<(PolityId, f32, u64)>,            // payer, share of revenue, last day
               union_months: u32 }
struct Survey {                     // monthly, shares 0..1
    scarcity: Vec<f32>,             // 1 - Σ richness·reserve / Σ richness, own non-renewable deposits
    power: Vec<f32>,                // 0.6 income share + 0.4 fleet share
    fast: Vec<f32>, slow: Vec<f32>, // EMAs of ln(power), 12 and 60 months
    stake: Vec<Vec<f32>>,           // [a][b] share of b's issuer market value held by a's holders
    trade: Vec<Vec<f32>>,           // [a][b] goods flow between a and b / a's market turnover
}
enum Via { Origin, Accord, Force }
enum Ending { Metamorphosis(PolityId), Extinction(Option<PolityId>) }
struct Verdict { ending: Ending, day: u64, merged: f64, forced: f64, player_lost: bool }

// ai/mind.rs: hidden, own sim_state! entry, fields pub(in crate::ai)
struct Minds { list: Vec<Mind> }
struct Mind { anchor: [f32; 4], lean: [f32; 4],  // mixtures: Cooperative, Opportunistic, Paranoid, Expansionist
              plan: Plan }                       // Idle, Strike(PolityId), Court(PolityId)
```

`assets/data/diplomacy.ron` (`data::DiplomacyBalance`) holds every number below, `benign = (1, .5, .4, 0)`, `menace = (0, .5, .6, 1)`, a likelihood row per `EvidenceKind` and four `Temper(security, power, wealth, bond, risk, honor, caution)` (weights; `risk` = win probability required at full restraint, `caution` = position assumed inside an estimate range): Cooperative/kooperativ (0.6, 0.2, 1.0, 1.0, 0.70, 1.0, 0.5), Opportunistic/opportunistisch (0.5, 0.8, 1.0, 0.3, 0.60, 0.2, 0.4), Paranoid/misstrauisch (1.2, 0.4, 0.5, 0.1, 0.55, 0.6, 0.9), Expansionist/expansiv (0.4, 1.2, 0.6, 0.2, 0.55, 0.4, 0.2). `polities.ron` gains `culture`. Anchors come from `Rng::stream(seed, Domain::Ai, u64::MAX, 0)`, independent of the template (dominant weight 0.6, each archetype dealt once before repeats).

## 3. Algorithms per step

All steps are gated by `day_start`/`month_start`; nothing new needs fine ticks.

**Monthly, `SimSet::Diplomacy`** (`diplomacy::survey`):
- `power` from `PlanetEconomy.income`, `Firm.profit`, `Fleet.strength`; `stake` from `Issuer.holdings` (`polity_of_holder`, `polity_of_issuer`); `trade` from `Logistics.flows`.
- Trend `g = clamp((fast−slow)/4, ±0.08)` per year; `rise_ab = clamp((g_b−g_a)·15, −1, 1)`. Restraint `ρ_a = (1−scarcity_a)²`. Link `link_ab = min(1, trade_ab+stake_ab)`.
- Culture: `c_a += 0.004·Σ_b contact_ab·(c_b−c_a)`, `contact_ab = trade_ab + stake_ba + 0.1·pacts_ab` (+1 under Union); war reverses the sign at 0.002; `d_ab = |c_a−c_b|/2√3`.
- Beliefs: evidence `e` about x reaches viewer v if `clarity_vx ≥ e.needs_clarity`: `post_k ∝ post_k·(1 + clarity·(L_e[k]−1))`; monthly forgetting `post = 0.99·post + 0.0025`. `clarity_vx = clamp(intel_vx + 0.15·pacts + 0.3·trade, 0, 1)`, `intel` from `tech::Research`.
- Evidence, public: BrokeTreaty, UnprovokedWar, PreventiveWar (target `rise > 0.1`), GrievanceWar, HonouredCall, RefusedCall, OfferedPact, RefusedPact, Expropriated, LenientPeace, HarshPeace, BroadInvestment, CornerStake. Needing clarity 0.4: ArmsBuildup, BorderMassing, Restraint (a year of `p ≥ 0.75` over a pact-less neighbour without striking).

**Daily, `SimSet::Aftermath`** (`diplomacy::aftermath`): drains a transient `WarEvents` resource written by SpaceCombat and Siege (pattern of `EquityFlows`). Captured planet: `via = Force`, `absorbed_pop`, `war.taken`; a lost capital moves to the most populous remaining world; a polity without planets is closed by `polity::succeed(dead, captor)` (today nothing clears `Polity.alive`). Monthly: exhaustion `e_x = war_weariness_x/100 + 0.5·lost_pop_x/start_pop_x`; at `e_x ≥ 1` x capitulates to the opponent's standing `demand`; both: status quo ante. Every peace starts a 5-year truce.

**Subjugation**: `overlord[v] = o`. v keeps planets and economy, loses diplomacy commands, shares o's wars (`at_war` resolves bloc heads), pays 25 % of `PolityFinance.revenue` monthly; its planets are tagged `Force`; suzerainty passes to o's successor. No rebellion in M6: the status is monotone.

**Merger**: Union in force, `stake_ab ≥ 0.20`, `stake_ba ≥ 0.20`, `d_ab ≤ 0.25` for 12 consecutive months. `polity::merge` creates community N (culture and `Mind` population-weighted, a human partner leads), then `succeed(a, N)`, `succeed(b, N)`; all non-`Force` planets of both become `Accord`. `succeed` is the one relabelling routine (planet owners, `TradeHouse.polity`, `Fleet.owner`, `Bond.issuer`, `HolderId::State` keys, treasuries moved, self-held bonds cancelled, `SimConfig.player`); `polity::spawn` is the one place that grows per-polity vectors (`apply_secession` pushes them by hand today). Regions joining through `foreign_pull` are tagged `Accord`.

**Monthly, `SimSet::Victory`** (`diplomacy::judge`; first match, written once, then `game_running` in `lib.rs` turns false):
1. Population below 0.30 × `peak_population`: `Extinction(None)`.
2. Sovereign = alive, no overlord, bloc population at least 3 % of all (the loader enforces 3 % × `max_polities` < 1, so one always exists). Exactly one sovereign S: `Metamorphosis(S)` if `absorbed_pop` on `Accord` planets exceeds that on `Force` planets, else `Extinction(Some(S))`.
3. Player dead or subjugated: `Extinction(Some(victor))` with `player_lost`.

**Why every game ends, once**
- Wars are finite: weariness rises every war month (`weariness_per_month`, raise to 1.0); capitulation or status quo is forced within 100 months.
- The sovereign count rises only through a breakaway holding 3 % of all people (capitals cannot secede in `update_cohesion`); subjugation, annexation and merger are irreversible; one month at count 1 suffices.
- Reserves only fall, so mean ρ tends to 0: costs of leaving the status quo vanish, gains remain. The exits are complementary in the same variables: unlinked or distrusted pairs make `U_war > 0` for the stronger, linked and trusted pairs `U_union > 0`. `GameData::load` rejects a `diplomacy.ron` in which an archetype has neither exit at ρ = 0 and parity.
- Backstop: rule 1. Target: space full near year 40 (`space_runs_out`), `economy.depletion` 0.0000012 (four times today) for mean ρ near 0.25 around year 130, endings in years 150–250, sweep limit 400.

## 4. Commands

```rust
DeclareWar { target, aim: WarAim }      // extends today's variant
Propose { to, offer: Offer }            // Offer::{Treaty(TreatyKind), Peace(PeaceTerms), CallToArms { against }}
Respond { proposal: u32, accept: bool }
CancelTreaty { with, kind }             // effective after 12 months (Trade: 3)
SetDemand { enemy, terms: PeaceTerms }  // applied when the enemy capitulates
// PeaceTerms { cede: Vec<PlanetId>, tribute_rate: f32, tribute_months: u32, equity: f32, status: Option<WarAim> }
```
`MakePeace` (unilateral today) is removed; `SAVE_FORMAT` becomes 2.

Validation in `command::execute` (German reasons): parties alive and not subjugated; no duplicate or expired proposal; a refused offer is blocked for 12 months; ladder Trade, NonAggression, Research/Investment, Defence, Union (Union after 60 months of Investment and Defence); `cede` only own planets; `tribute_rate` and `equity` (share of State holdings) at most 0.5; `status` not above the war aim; `Respond` only by the addressee. War against a pact or truce is allowed but logs `BrokeTreaty`, calls `shock_polity` (`breach_shock` 0.8) and gives the victim a grievance. `Expropriate` feeds `Expropriation.wronged` (returned by `finance::expropriate`, dropped today) into evidence and grievances.

Flow: `Propose` stores a `Proposal`; an AI addressee answers in its next monthly step with `Respond`, the player from an inbox; expiry (90 days) is refusal, an unanswered `CallToArms` logs `RefusedCall`. Acceptance executes atomically: pact start, or peace (ceded planets tagged `Force`, tribute as monthly `economy::pay` between treasuries, equity through `finance::distribute_shares`).

## 5. AI behaviour

Monthly in `SimSet::Ai` (`ai::strategy`), ascending IDs, never for `config.player`; jitter ±0.03 from `Rng::stream(seed, Domain::Ai, a, month)`; `T = Σ_k lean_k·temper_k`. Inputs: last month's survey (`P` = power), own beliefs, `Research::estimate` ranges (`lo..hi`). `x⁺ = max(x, 0)`; `q_ab` is a's estimate of `p_ba`; `breach_ab` = 1 under pact or truce without grievance.

```
trust_ab = belief_ab·benign    h_ab = belief_ab·menace    Mest_b = lo + T.caution·(hi−lo)
p_ab     = 1/(1 + (1.2·(Mest_b+allies_b)·(1+hops/8) / (M_a + Σ trust·allies_a))²)     fear_ab = h_ab·q_ab
U_war    = p_ab·(T.power·P_b + T.security·(fear_ab + h_ab·rise_ab⁺)) − (1−p_ab)·P_a/2
           − ρ_a·(T.wealth·link_ab + T.honor·breach_ab + weariness_a/100)
U_union  = T.bond·trust_ab·(min(stake_ab,stake_ba)/0.20 + 1 − d_ab + trade_ab)/3
           + T.security·trust_ab·max_c(h_ac·rise_ac⁺·[P_c>P_a]) − ρ_a·T.power·P_b/(P_a+P_b)
U_invest = yield_b·(1−risk_b) + T.bond·trust_ab·(0.20−stake_ab)⁺ + T.power·pull_ab
```

- War: `Plan::Strike` when `U_war > 0.05·ρ_a` and `p_ab ≥ 0.35 + (T.risk−0.35)·ρ_a`; six months of arming and massing (visible evidence) precede `DeclareWar`; the aim escalates with `p` and `1−ρ_a`. `rise_ab⁺` is the preventive strike.
- Pacts: propose or accept when benefit minus ρ × exposure is positive, e.g. NonAggression `T.security·fear_ab − T.power·U_war⁺`, Defence `T.security·trust_ab·max_c min(fear_ac, fear_bc)`: shared fear of a riser is the balancing alliance, `rise_ac⁺` in `U_union` the flight into merger.
- Foreign shares (`BuyShares`, `TakeoverBid`): `pull_ab` counts regions of b with cohesion below 35 where a is within 0.15 of `foreign_pull`. Cooperative minds spread thin (BroadInvestment), predatory ones corner (CornerStake); the target buys back, uses `DistributeShares` (existing `steward_finances`) or `Expropriate`.
- Peace: propose or accept when `e_enemy − e_own` exceeds a temper margin; demands grow with the gap (0.3 tribute and equity, 0.5 keep taken planets, 0.8 subjugation, 1.0 annexation).
- Means: arms share `clamp(0.15 + 0.5·max fear + 0.3·[Strike], 0.1, 0.8)` of the free treasury, the rest to foreign shares, research, cohesion. Only `ai/levers.rs` names M4/M5 commands: yard orders (`GameData::best_design`, class mix against the estimated enemy mix), `MoveFleet`, `SetStance`, siege orders. `p ≥ 0.6`: seek battle, then besiege the most valuable reachable planet; 0.4–0.6: `Stance::Blockade` on the system most dependent on imports (`Market.supply` against `demand`) or a `StarSystem.chokepoint`; below: `Raid`, `Guard` home, `Escort` when `Logistics.captured` rises.
- Drift: stress `s = (Σ_b trust·link, max(scarcity, yield stress), max_b fear, max_b (2p−1)⁺)`; `lean += 0.03·(normalize(anchor+s) − lean)`; shocks of 0.15 (betrayed: Paranoid; profitable victory: Expansionist; default: Opportunistic; honoured call: Cooperative).

The player is one more subject of `beliefs`: broken pacts make every AI arm and ally, a record of restraint attracts union offers.

## 6. Feedback into economy, finance, cohesion

Pact effects:
- Trade: no tariff; otherwise a foreign sale pays 8 % to the host treasury (`logistics/mod.rs::move_and_trade`, deducted in the route profit).
- NonAggression: war is a breach.
- Research: +10 % research points (`tech`), clarity.
- Investment: partner issuers rank as domestic in `finance/investors.rs::invest_holders`; partner holders count half in `foreign_share`; without it a State holds at most 10 % of a foreign issuer; expropriation is a breach.
- Defence: an attack on one issues `CallToArms` to the other.
- Union: all of the above; partner holders count under `Ownership.regions` (binding); merger clock.

Further hooks:
- `finance/cohesion.rs`: `ownership_of_region` takes `&Diplomacy`; new `CohesionFactors.foreign_rule` („Fremdherrschaft“, −20·d·(1−years/20)) for `Via::Force` planets; `honeymoon` only for `Origin` and `Accord`.
- `finance/plugin.rs::monthly_books`: tribute; vassals carry bloc-war weariness.
- `finance/shares.rs`: `shock_polity` on breach.
- `state.rs` registers `Minds`; `SimState::generate` sets both resources up; `data/mod.rs` loads and validates.

Money only moves (`economy::pay`); no goods are created.

## 7. User interface

`game` reads only `diplomacy::view` (pure functions; foreign fleets as estimate ranges), never `ai::Minds`.
- „Diplomatie“: per community status (Krieg, Waffenruhe, Vasall), pacts with notice dates, grievances, cross-stakes both ways, trade share, cultural distance, power share with trend and uncertainty band, scarcity („Vorkommen“).
- „Lagebild“: bars kooperativ, opportunistisch, misstrauisch, expansiv from `beliefs[player][x]` with „Sicherheit“; „Indizien“ (date, act, reading it supports); „überholt uns in etwa N Jahren“; expected answer to each offer (nimmt vermutlich an, lehnt vermutlich ab, unklar), computed by the AI's own utility functions fed with the believed mixture and estimated numbers.
- Offer composer (pacts, peace terms), inbox with expiry, war dialog with aim and warnings (pact breach, hostages, lost trade), standing demand, union progress (Vertragsdauer, gegenseitige Beteiligung, kulturelle Nähe).
- `AppState::GameOver`: „Metamorphose“ or „Auslöschung“, merged against forced population.

## 8. Tests

1. `exactly_one_ending`: seeds 0..8, limit 400 years: `verdict.is_some()`; 360 further days leave `hash()` unchanged; no war outlasted 112 months.
2. `judge_partition`: constructed states per rule; vassals and a 2 % state do not count; merged = forced gives Extinction.
3. `assessment_is_blind`: permuting every `Mind.lean` (test-only setter) leaves `hash_value(&view::player_view(world))` identical; at day 0 every posterior is exactly ¼.
4. `evidence_reveals`: 16 seeds, year 60: mean posterior mass on the true dominant archetype ≥ 0.45, and ≥ 0.35 again 20 years after a forced flip.
5. `pressure_shifts_disposition`: a scripted five-year blockade of an AI capital raises its `lean[Paranoid]` by 0.15 against the same seed without it.
6. `drives`: raising only `rise_ab` turns `u_war` positive; removing rival c turns `u_union(a, b)` negative; `u_war` falls monotonically in `link`.
7. `no_idle_pair_without_restraint`: grid over archetype pairs, strength ratio 0.5–2, link, distance at ρ = 0: war for one side, or union or the next pact for both, is positive.
8. `succession_conserves`: after merger, annexation, capitulation: `money_drift == 0`, per-polity vectors equally long, no reference to a dead `PolityId` (new in `check_invariants`).
9. `stakes_act_outward`: a scripted 45 % foreign stake in an autonomous colony makes it join the holder as `Accord`; an Investment pact halves the „Fremder Besitz“ points in `cohesion_target`.
10. `breach_is_public`: war under NonAggression raises every third party's posterior on Opportunistic plus Expansionist.
11. Determinism, save-and-continue and replay tests span a pending proposal and a war.

`RunSummary` gains: ending, rule, end year, timeout (balance defect), merged and forced population, wars and mean length, pacts signed and broken, mergers, accessions, subjugations, mean ρ, archetype flips, assessment accuracy at years 20/60/100 (feature `reveal`, only in `headless`), start rank and archetype of the survivor. Targets: no timeouts, Metamorphosis in 30–60 % of runs, no archetype prevailing in over 40 %.

## 9. Risks

- Endless coexistence: restraint ratchet, loader check, grid test, survival rule, sweep timeout.
- Early snowball: shared fear of the riser forms defence pacts; weariness; „Fremdherrschaft“. Watch the survivor's start rank.
- Leaks: hints use the believed mixture, `Minds` is private, blind test. Save files stay readable (accepted).
- Succession bugs: single `succeed`/`spawn`, exhaustive destructuring, invariant.
- M4/M5 names still moving: `ai/levers.rs` outbound, `WarEvents` inbound, nothing else.
- `depletion` also moves the M2 economy: tune with `cargo headless economy`; the ρ exponent is the second lever.
- Share conquest too cheap for a human: `price_impact`, the 10 % cap, AI buy-back; scripted bot players in `harness.rs`.

### Critical Files for Implementation
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/diplomacy/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/ai/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/harness.rs