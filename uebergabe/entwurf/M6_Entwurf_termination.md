# KONFLUENZ M6: diplomacy, hidden dispositions, endings

Paths are relative to `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/`.

## 1. Core idea in five sentences

1. Peace is never neutral: every pair of sovereigns is in exactly one mode, **Open** (default), **Closed** (embargo) or **War**, and each drifts through mechanics the simulation already has: open peace integrates (culture converges, shareholdings entangle), closed peace diverges (autarky, uneven depletion), war consumes (weariness, cohesion, ships).
2. Each AI has a hidden weight vector over four dispositions and one decision core, a trend calculation: a closing window means a preventive strike if it can win, flight into union or protection if it cannot.
3. Securities act outward along three peaceful absorption paths: union (mutual stakes), accession of single regions (existing `foreign_pull`), whole accession of a dependent community.
4. Every planet records how it came under its flag (`Founded`, `Joined`, `Taken`); when one sovereign remains, this tag count alone decides between Metamorphosis and Extinction, in a set-once `Outcome`.
5. AI and player read the same public dossier of acts, the truth lives in a crate-private resource, and every threshold is a RON lever that sweeps measure.

## 2. State

```rust
// diplomacy/mod.rs: Resource, already in sim_state!
struct Diplomacy {
    wars: BTreeSet<(PolityId, PolityId)>,        // exists
    pairs: BTreeMap<(PolityId, PolityId), Pair>, // (lower, higher), lazily created
    standing: Vec<Standing>, power: Vec<Power>, dossiers: Vec<Dossier>, // per polity; Power { weight, growth }
    culture: Vec<[f32; 4]>, bond: Vec<Bond>,     // per planet; culture axes 0..1
    proposals: Vec<Proposal>,
    low_water: u32,          // fewest sovereigns ever reached
    peak_population: f64,    // millions, running maximum
    collapse_months: u32,
    outcome: Outcome,
}
struct Pair {
    since: [Option<u64>; 6], // start day per TreatyKind
    embargo: [bool; 2],      // who declared; either closes the pair
    opened_until: [u64; 2],  // imposed access after defeat (day)
    trust: [f32; 2],         // 0..100
    stake: [f32; 2],         // 0..1 of market value on the other's territory
    distance: f32,           // culture gap 0..1
    ready_months: u32,       // fusion clock
    war: Option<War>,        // attacker, aim, score -100..100, pop0, fleet0
    truce_until: u64, rounds: u8,
}
enum Standing { Sovereign, Vassal { lord: PolityId, since: u64, voluntary: bool }, Gone }
enum Bond { Founded, Joined, Taken }
enum TreatyKind { NonAggression, Trade, Research, Investment, Defence, Union }
struct Dossier { entries: VecDeque<Evidence>, score: [f32; 4], mass: f32 } // decayed log-likelihoods
struct Outcome { ending: Option<Ending>, day: u64, player: PlayerFate }
enum Ending { Metamorphosis { community: PolityId, joined: f64, taken: f64 },
              Extinction { survivor: Option<PolityId>, collapse: bool } }

// ai/mind.rs: Resource `Minds`, new in sim_state!; fields pub(crate), no getters
struct Mind {
    weights: [f32; 4], // kooperativ, opportunistisch, paranoid, expansionistisch; sum 1
    pressure: f32,     // 0..1
    stance: BTreeMap<PolityId, StanceState>, // stance, since, baseline, barred_until
}
```

`polities.ron` gains `culture`; colonies inherit their capital's vector. Initial `weights`: 0.55 on a dominant disposition dealt from a shuffled deck (`Rng::stream(seed, Domain::Ai, polity, 0)`), the rest random.

## 3. Algorithms per step

Monthly (`month_start`) unless noted; levers in `balance.ron: diplomacy` and a new `dispositions.ron`. Slots: `SimSet::Ai` (decisions), `SimSet::Diplomacy` (all below), `SimSet::Aftermath` (absorption), `SimSet::Victory` (ending, daily).

**Measure.** `weight = 0.5·income share + 0.3·fleet-value share + 0.2·population share` (`PlanetEconomy.income`, `DesignStats.value`, `Galaxy::population_of`); `growth += (12·ln(weight/previous) − growth)/36`; `window_pq = (growth_q − growth_p)·horizon_years` (15); `stake[a→b]` via `finance::polity_of_holder`, `polity_of_issuer`.

**Culture** per planet r of owner p (c̄: polity mean):
`c_r += k_own·(c̄_p − c_r) + Σ_q k_contact·x_rq·(c̄_q − c_r) − Σ_enemies k_war·(c̄_q − c_r)`
with exposure `x_rq = 0.5·q's share of r's firms + 0.3·share of r's wealth placed in q + 0.2·share of inflow from q` (`Logistics.flows`), ×1.5 under Trade, ×2 under Union; defaults 0.004, 0.02, 0.002.

**Integration and trust.** `I_ab = min(min(stake_ab, stake_ba)/union_stake, (1 − distance)/(1 − union_distance), 1)`. `trust_ab += 0.05·(target − trust_ab)`, `target = 40 + 6·treaties + 30·I + 25·(P_koop − P_opp − P_exp)(b) − 30·[embargo] − 10·rounds`; `P` is the public assessment, so reputation and the player's analysis are one function.

**Peaceful absorption.**
- *Union*: treaty ≥ 24 months, both stakes ≥ `union_stake` (0.15), distance ≤ `union_distance` (0.15), held 12 months → `absorb(smaller, larger, Joined)`; the larger slot is refounded as the new community (name, colour, population-weighted `weights`), player-led if the player is inside.
- *Accession*: a seceding region joins the sovereign maximising `foreign_share_q + 0.3·(1 − d(c_r, c̄_q))` if ≥ `foreign_pull`; a Freistaat forms only while sovereigns ≤ `low_water`.
- *Whole accession*: `stake_ab ≥ 0.5`, distance ≤ 0.30, b's capital `Restless` for 24 months → `absorb(b, a, Joined)`.
- *Protectorate*: vassalage by proposal; annexation after 20 years tags `Joined` if voluntary, else `Taken`.

**War.** `score = 50·Δtaken + 25·Δfleet loss + 15·Δblockaded + 10·Δweariness/100` (`Fleets::is_blockaded`, `PolityFinance.war_weariness`). A fallen planet changes owner at once via `transfer_planet(planet, to, Taken)`, M5's only entry point. The victor may demand at most: `|score| < 20` status quo; `< 55` reparations (10 % of income, 10 years) plus imposed opening (no embargo or expropriation against the victor: defeat becomes capital penetration); `≥ 55` or capital fallen, aim `Subjugation`: vassalage (15 % tribute, no diplomacy). Mutual weariness ≥ 85 forces status quo. Peace sets a 5-year truce, `rounds += 1`.

**Drift.** `pressure = smooth(clamp(max_q window_pq/0.5, 0, 1))`; `weights ← normalise(weights + 0.02·pressure·(weights·T − weights) + nudges)`, `T` a 4×4 row-stochastic matrix validated irreducible; nudges: betrayed → paranoid +0.10, war won → expansionist +0.06, indecisive war → cooperative +0.10.

**Evidence.** Public acts enter the actor's dossier: treaty proposed/accepted/refused/broken, ally honoured/ignored, war declared (odds, target already at war), terms harsh/mild, embargo, expropriation, stake build-up. `score_k = 0.995·score_k + strength·L[kind][k]`; `assess(&Dossier, intel, &data)` returns `p = softmax(score)`, `band = 0.45·(1.2 − intel)/√(1 + mass)` and the five strongest entries. Covert facts (fleets, technology, growth) never enter a dossier; observers get them only as `tech::Research::estimate()` ranges.

**Ending** (set once). `S` = alive sovereigns with population. `S = ∅`, or population below `collapse_share` (0.35) of `peak_population` for 36 months → `Extinction { survivor: None, collapse: true }`, everybody loses. `|S| = 1` → population on the survivor's planets by tag (vassals by `voluntary`): `joined > taken` → `Metamorphosis`, else `Extinction`. `game_running` (lib.rs) then returns false. `PlayerFate::Eliminated` when the player's community is absorbed.

**Why it terminates, unscripted.** No timer; the modes drift. *Open*: exposure and stakes accumulate, trust follows `I`, the pair fuses or the richer side absorbs regions (culture gap halves in ≈ 3 years under Union, ≈ 30 without treaties). *Closed*: deposits deplete unevenly, so one side always faces a positive window. *War*: weariness (+0.6/month) reaches 85 within 12 years, and peace never restores the status quo ante: conquests stay, defeat opens the loser's capital market, indecisive rounds push both minds toward cooperation. The AI closes the loop: it holds one stance toward every reachable sovereign, and a stance without progress on its own metric within `patience_years` (12) is barred for a period, so within 36 years it has declared war, raised `I` or a stake, or offers itself as protectorate (lordship is always accepted). `low_water` forbids re-fragmentation. The count ratchets down, typically one step per 8–15 years once space is full (≈ year 40): expected end in years 90–160, sweep limit 250. A human can prolong only by declining offered endings. Caveat: deposits bottom out at `reserve` 0.02 (`economy/production.rs`) and biomass is renewable, so depletion paces divergence but ends nothing alone; proposed `economy.depletion` ×3.

## 4. Commands

```rust
DeclareWar { target, aim: WarAim }  // Limited | Subjugation | Independence
Propose { to, deal: Deal }          // Treaty(kind) | Peace(terms) | Protectorate { lord }
Respond { proposal: u32, accept: bool }
BreakTreaty { with, kind }
SetEmbargo { target, on: bool }
```

`MakePeace` becomes `Propose { Peace }`. Rejections carry German reasons as today.
- All: both alive, distinct; a vassal may only `Respond` or declare `Independence` on its lord.
- `DeclareWar`: no truce; no NonAggression, Defence or Union in force (break first). `Limited` caps demands at reparations.
- `Propose`: one open proposal per pair and deal; two-year cooldown after refusal. Prerequisites: Trade – peace, pair open; Research – Trade; Investment – Trade for 24 months; Defence – NonAggression; Union – Trade, Investment, NonAggression, `I ≥ 0.5`; Peace – terms within the score band (milder always allowed); Protectorate – either side may ask.
- `Respond`: addressee only; prerequisites rechecked; lapses after 60 days without penalty.
- `BreakTreaty`: immediate; evidence, trust −30, `shock_polity(breaker, 0.9)`.
- `SetEmbargo`: rejected under Trade, Investment, Union or imposed opening.

AI answers use the same queue one day later; replay and `needs_fine_ticks` stay unchanged.

## 5. AI behaviour

Monthly per AI sovereign; parameters = `weights ·` rows of `dispositions.ron`:

```
                  need prevent greed affinity honour pessimism arms
kooperativ        2.0   0.3    0.1    +15     0.9     0.5     0.10
opportunistisch   1.4   0.5    0.8     +5     0.3     0.3     0.15
paranoid          1.2   1.0    0.2    -15     0.7     0.9     0.25
expansionistisch  1.15  0.6    1.0     -5     0.5     0.4     0.25
```

- **Picture.** Others' weight, growth and fleet are read at `pessimism` within the estimate range (noise: `Rng::stream(seed, Domain::Intel, pair, month)`), character from `assess`. `win_q = clamp((ratio_q − need + 0.3·pressure + 0.25)/0.5, 0, 1)`, allies included; `D_q = clamp(window_pq/0.5, 0, 1)`; `win*` is against the greatest threat.
- **Stance** toward q: argmax, switching margin 0.1.
  Strike `win_q·(prevent·D_q + greed·weight_q/weight_p) − honour·[treaty] − weariness/50`
  Bind `pressure·(1 − win*) + trust/100 + affinity/50 + I`
  Hedge `(1 − win_q)·D_q·(1 − trust/100)`
  Exploit `greed·[pair open]·(1 − cohesion_q/100)`
  Progress metrics: war declared; new rung or `I` +0.1; window smaller; stake +0.05.
- **Economy.** Posture peace/arming/war sets `SetTaxRate` 0.15/0.22/0.30 (less if "Steuern" threatens `restless_below`), `SetTreasuryTarget`, `IssueBonds` while `bond_yield < 0.25`; `steward_finances` stays and buys back above 0.3 foreign share.
- **Fleet.** Target value `max_q(need·fleet_q) − allies`, capped by `arms`(×2 arming, ×4 war)`·income/upkeep_per_month`; classes against the observed mix via `battle::matchup`.
- **War and peace.** Stage at the own chokepoint nearest the target (`StarSystem.chokepoint`, `Galaxy::shortest_path`); declare only with local ratio ≥ need and stocks ≥ 70 %; main fleet `Stance::Blockade` on the richest reachable system, then siege; light group `Raid` on bridge lanes, one `Escort`; dock at home below 30 % ammunition or fuel. Sue at score ≤ −20, weariness ≥ 60 or yield > 0.25; cooperative victors demand one band less.
- **Treaties.** Bind proposes the next rung; a rung is accepted if `trust + affinity + 30·pressure·(1 − win*) ≥` 35/40/50/55/60/70; Union also requires being the larger partner or under pressure. Hedge seeks NonAggression and Defence with the threat's other neighbours and embargoes below trust 20. A treaty is broken when Strike beats the held stance by more than `honour`.
- **Foreign shares.** Bind buys the partner's trade houses and Kontore (realm-wide, no regional pull) up to `union_stake`; Exploit concentrates on the target's weakest non-capital region up to `foreign_pull`, using `TakeoverBid` where funds dominate.
- **Minors, leaders, player.** Weight < 0.06: Bind to the most trusted large neighbour, else protectorate. A weight > 0.4 raises every other `D` toward it. The player gets no special rules; their dossier feeds trust like anyone's.

## 6. Feedback into economy, finance and cohesion

Treaties act physically:
- NonAggression blocks `DeclareWar`.
- Trade: in `logistics::move_and_trade` a foreign house pays `tariff` (0.08) on sales to the system owner's treasury (`economy::pay`), zero under Trade or Union; `open_for` closes embargoed pairs.
- Research: each side gains 10 % of the partner's research points (`tech::Research`), intel +0.2.
- Investment: expropriating or embargoing the partner is a breach; `invest_holders` sends the foreign budget ×3 to partners; no purchases across a closed pair. `shares::expropriate` already returns `Expropriation.wronged`, which `command.rs` drops today: it becomes evidence.
- Defence: declare on the aggressor within 30 days or lose the treaty.
- Union: `cohesion::ownership_of_region` counts the partner's holders as `regions`, not `foreign`: mutual ownership binds across the border as inside a realm.

Further: `CohesionFactors` gains `culture_gap` ("Kulturelle Ferne", −20·gap) and `taken` ("Besatzung" −15 replaces "Aufbruchstimmung"); `update_cohesion`/`apply_secession` use the generalised join and `low_water`. Tribute is a monthly treasury transfer. `diplomacy::absorb(from, into, bond)` is the only way a community ends: planets and tags, treasury, `HolderId::State` holdings, bonds, `TradeHouse.polity`, `Fleet.owner`, research, wars; it maintains `Polity.alive` and `capital`, which nothing updates today. `state.rs`: `minds: Minds` in `sim_state!`, `SimConfig.player` remapped on union. `data/mod.rs::resolve_and_validate` checks the new data (`T` irreducible, a likelihood row per evidence kind, `union_stake` < 0.5).

## 7. What the user interface needs to show and control

- **Diplomatie**: per community the mode (Krieg, Offen, Embargo), treaty ladder with the rejection reason on greyed rungs, attitude in five steps, inbox with expiry, the five commands.
- **Verflechtung** per pair: union treaty, both stakes against the threshold, cultural proximity, fusion countdown; warning when a region nears `foreign_pull`.
- **Lagebild**: four disposition ranges ("kooperativ 35–60 %"), the five strongest dated indications, strength and trend as ranges, trade data (stakes, flows, tariffs); labelled "Einschätzung aus Indizien"; built solely from `diplomacy::view::PlayerView`.
- **Krieg**: score, admissible terms, weariness (the enemy's as range).
- **Endspiel** strip: sovereigns, population against the survival threshold, bar "beigetreten / genommen".
- Ending screen in the existing `AppState::GameOver`: "Metamorphose" or "Auslöschung", player fate; an eliminated player may keep watching. Chronicle entries use the existing `ChronicleKind::Diplomacy` and `End`.

## 8. Tests

1. `every_game_ends_once`: seeds 0..8, AI only, limit 250 years: `ending.is_some()`; five more years change neither ending nor hash.
2. `classification_is_total`: table test of the pure `classify`; tie → Extinction.
3. `hidden_state_does_not_leak`: swap two AIs' `weights` after 30 years; the digest of `PlayerView`, dossiers and chronicle is unchanged; no chronicle text names a disposition.
4. `assessment_ignores_minds`: equal dossiers give equal assessments; one `Broke` entry raises `p[opportunistisch]`; `weights` stay on the simplex over 500 months of pressure.
5. `union_needs_all_three`: fusion only when treaty, stakes and proximity all hold, within 13 months; the partner's holders count as domestic in `ownership_of_region` until the union is broken.
6. `absorb_conserves`: `money_drift == 0`, share totals and population unchanged; save/load mid-fusion reproduces the hash.
7. `tags_decide`: scripted accession → Metamorphosis; conquest → Extinction; two subjugations → Extinction.
8. `treaty_blocks_war`; `proposal_roundtrip` (AI answers next day, log replay gives the same hash); `tariff_and_embargo` (treasury gain equals house payments).
9. `stance_rules`: declining and stronger → Strike; declining and weaker → Bind; barred after `patience_years`.
10. `collapse_is_an_ending`; `player_elimination_is_final`; `CommandLog.ai_rejected` < 2 %.

**Sweep columns** (`harness::RunSummary`): ending type and year (median, p10, p90); limit hits as balance defects; sovereigns at years 25/50/75/100; longest span without a change in sovereign count; pair-months per mode; wars, unions, accessions, whole accessions, conquests, vassals; Freistaat lifetimes; joined versus taken; survivor share per starting disposition; disposition flips; Brier score of `assess` against the truth at years 20/40/80; opportunists rated cooperative before their first breach. Targets: no defects, median end 100–140, Metamorphosis 35–60 %, Brier 0.12–0.20.

## 9. Risks and how the design contains them

- **Stall or runaway**: one lever per drift (`k_contact`, `union_stake`, `horizon_years`, `patience_years`, `need`, `depletion`); the quiet-span column locates stalls.
- **Human holdout**: meets capital penetration, balancing unions and repeated strikes; can decline, not stall.
- **Fragmentation**: bounded by `low_water`; minors seek protection.
- **Leaks**: `Minds` is crate-private, the UI reads only `PlayerView`; save files still contain the weights (accepted).
- **Assessment too sharp or useless**: recalibrate `L` from sweep frequencies.
- **Per-polity vectors**: unions create no ids; `check_invariants` asserts equal lengths.
- **M4/M5 names**: one adapter `ai/sight.rs`; conquest only through `transfer_planet`.
- **M2/M3 balance**: `tariff: 0` restores today's trade; capital stays open as today.
- **Indecisive sieges**: blockade, weariness and imposed opening still move territory through cohesion.

Memory: no stored note was relevant or used.

### Critical Files for Implementation
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/diplomacy/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/ai/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/harness.rs