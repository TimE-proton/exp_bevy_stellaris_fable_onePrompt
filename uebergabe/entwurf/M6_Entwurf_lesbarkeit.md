## 1. Core idea

Every AI community has a hidden mind: a blend of four archetypes (kooperativ, opportunistisch, paranoid, expansionistisch) that parameterises one shared decision function and drifts under four pressures computable from public data. Each decision point is an *occasion* with a public context: the AI chooses by softmax over utilities, and observers score that choice with the same function under each pure archetype, so the assessment is inference from behaviour that cannot drift away from the real AI rules and never reads the hidden weights. AIs read each other and the player through the same dossiers: nobody is omniscient, and the player has a public face too. Treaties are a six-rung ladder wired into existing mechanisms: mutual shareholdings are the peaceful road to union, one-sided foreign majorities remain the hostile one. A trend calculation plus geological decline forces every pair of sovereigns to bind or strike, and each game ends exactly once, as Metamorphose or Auslöschung.

## 2. State

Plain serde resources in `sim_state!` (`crates/sim/src/state.rs`); `SAVE_FORMAT` 2. Short paths are under `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/`; `crates/…` and `assets/…` are under the repository root.

```rust
// diplomacy/mod.rs (at_war, enemies_of, is_at_war stay as methods)
struct Diplomacy {
    relations: BTreeMap<(PolityId, PolityId), Relation>,  // a < b
    proposals: Vec<Proposal>,     // id, from, to, deal, expires_day, reason
    standing: Vec<Standing>,      // per polity: power share, trend, growth per year
    dossiers: BTreeMap<(PolityId, PolityId), Dossier>,    // (observer, subject)
    public_face: Vec<Dossier>,    // per polity, from public clues only
}
struct Relation {
    treaties: [Option<Treaty>; 6],  // by TreatyKind: since_day, ends_day
    war: Option<War>,             // aggressor, aim, since_day, score -100..100
    truce_until: u64,
    tension: f32,                 // 0..100
    affinity: f32,                // cultural proximity 0..1
    trade: [f64; 2],              // credits per month, each direction
    embargo: [bool; 2],
}
struct Dossier {                  // diplomacy/evidence.rs, must not import ai::mind
    logit: [f32; 4],              // decayed log-evidence per archetype
    weight: f32,                  // effective number of clues
    surprise: f32,                // nats above expectation
    break_day: Option<u64>,
    clues: VecDeque<Clue>,        // last 16: day, kind, toward, delta, source
}
struct Mind {                     // ai/mind.rs, Minds(Vec<Mind>): saved, hashed, fields private
    weights: [f32; 4],            // the hidden disposition, sums to 1
    anchor: [f32; 4],             // temperament
    pressure: [f32; 4],           // bind, gain, threat, want; 0..1
    plans: BTreeMap<PolityId, Plan>,  // Strike(aim) | Bind, prepared_days, cooldown
}
struct Endgame {                  // endgame.rs
    provenance: Vec<Path>,        // per planet: Own | Peaceful | Forced
    peak_population: f64, months_below: u32, free_states: u32,
    player_out_day: Option<u64>,
    outcome: Option<Outcome>,     // ending, survivor, day, peaceful, forced (Mio.)
}
// polity::Polity gains overlord: Option<PolityId> (Some = subjugated, not sovereign)
```

New `assets/data/dispositions.ron`, validated in `data/mod.rs`, holds every parameter below and four `Profile` rows:

```
                  arm(base,+threat) forward grab prevent merge honor spread open patience
kooperativ          .08 .15          .15    .05  .15     1.0   1.0   .9     .9   25 y
opportunistisch     .12 .20          .40    .8   .2      .5    .25   .5     .7    8 y
paranoid            .22 .45          .10    .1   1.0     .25   .7    .3     .15  12 y
expansionistisch    .25 .20          .70    1.0  .5      .15   .5    .1     .5   15 y
```

Fingerprints follow from the rows: paranoid minds arm in proportion to threat and stay home, expansionist minds arm regardless and stand forward, opportunist pacts fail exactly when temptation appears, cooperative minds scatter small stakes and keep pacts under temptation.

## 3. Algorithms per step

Daily (`day_start`): proposals, notice periods and truces expire, AI answers and plans advance, finished battles move war scores (`SimSet::Aftermath`). Monthly (`month_start`): bookkeeping in `SimSet::Diplomacy`, minds in `SimSet::Ai` on last month's public figures (the AI sees what the player saw), endings in `SimSet::Victory`.

**One decision function** (`diplomacy/occasion.rs`): `choice_probs(profile, occasion)` = softmax(U / 0.25). The AI uses `Σ w_d·Profile_d` and draws with `Rng::stream(seed, Domain::Ai, actor, day)`; an observer evaluates `P_d` with each pure `Profile_d` on the occasion as it sees it: blurred by `Domain::Intel`, the actor's private beliefs replaced by `public_face`. Occasions:
- **Rüstung, Aufmarsch** (yearly, and at once when the fleet share facing one neighbour crosses 0.5, which that neighbour always sees; blur `0.25·(1 − intel)`, fallback arms-goods flows): military upkeep / revenue against `arm`, fleet value at foreign borders against `forward`.
- **Anlage** (yearly, from the Börse): dispersion (1 − HHI) of state stakes abroad against `spread`; foreign owners at home tolerated, bought out or expropriated against `open`.
- **Antwort**: accept or decline, with reason.
- **Vertragstreue** (when temptation, `U_strike` without its honour term, exceeds 0.3): keep, denounce with notice, break; honour a Beistandsfall.
- **Eröffnung** (also yearly "Zurückhaltung" under temptation): strike, bind, wait.
- **Bedingungen**: war aim stated, peace terms demanded.

**Trend (monthly).** `power = 0.5·E + 0.35·F + 0.15·T` (galaxy shares of firm revenue, fleet build value, technology; foreign F and T from `tech::Research::estimate`); `trend ← trend + (12·Δln power − trend)/60`; `growth` likewise on absolute output. For i facing j: `ratio = ln(power_i/power_j)`; `window = clamp((trend_j − trend_i)·patience, 0, 1)`: j is overtaking me; `dep` = share of i's imports and portfolio tied to j (`Logistics.flows` by `Galaxy::system_owner`, cross-holdings); `trust = Σ_d belief[d]·honor_d`; `win = logistic(4·ln(F_i/F_j))`; `gain = clamp(power_j/power_i, 0.3, 1)`.

```
U_strike = (grab·gain·(1 + want) + prevent·window)·win − dep − honor·rungs/5 − weariness/100
U_bind   = merge·(window + dep + affinity)·trust − 0.6·(1 − want)·(1 − window)
U_wait   = 0      act above 0.15, after prep_days = 120 of preparation; cooldown 720 days
```

**Pressures and drift (monthly).** `bind` = ½ trade share abroad + ½ portfolio share abroad; `gain = max_j clamp(ratio − 0.3, 0, 1)·(1 − dep)`; `threat = max_j (tension/100 + belief[exp] + ½·belief[opp])·clamp(0.3 − ratio, 0, 1)`; `want = clamp(σ + decline, 0, 1)`, σ = 1 − richness-weighted remaining reserve of own non-renewable deposits, `decline = clamp(−growth·patience, 0, 1)`, ×0.3 while planets are claimable. `target = normalize(0.3·anchor + pressure)`; `w ← normalize(w + 0.04·(target − w))`. Jolts: treaty broken against me `w[par] += 0.20`; ally honoured a pact `w[coop] += 0.10`. Start: seeded dominant archetype 0.55, others 0.15, independent of the `polities.ron` template: names tell nothing. Pressures use public inputs only, so the player's window shows the same four numbers as estimates: where a mind is being pushed, while clues show where it is.

**Evidence (per clue and observer).** `ll_d = ln P_d`, or `−(y − μ_d)²/2s²` with `s = s0 + blur·(1 − intel)`; shifted to maximum 0, clamped to [−3, 0]. `logit += k·ll` (k: Rüstung .6, Anlage .6, Antwort .8, Bedingungen 1, Vertragstreue 1.5, Eröffnung 1.5); monthly `logit`, `weight` × `0.5^(1/60)`. Belief `b = softmax(logit)`, band `± 0.35/√(1 + weight)`. `surprise ← 0.7·surprise + 0.3·(−ln Σ b_d·P_d − H(b))`; above 1.2 nats: set `break_day`, halve `logit` and `weight`. `intel` = Research intel + 0.25 Forschungsabkommen (which also gives both +10 % research) + 0.4 Union + 0.2·dep, at most 1.

**Culture, tension (monthly).** `affinity += 0.004·(dep + cross_share + 0.1·rungs)·(1 − affinity)`, −0.01 per war month, −0.15 per breach. `tension`: expropriation +10, embargo +15, breach +25, harsh war aim up to +45 with everyone; −1 per month.

**Merger (monthly).** Union treaty for 3 years; `cross(a→b)` and `cross(b→a)` ≥ `merge_share` = 0.20 (holders' share of enterprise value on the other's territory; `cross_holdings` moves from `crates/game/src/ui/finance.rs` into `finance`); `affinity ≥ 0.70`; both confirm `Deal::Merge`. A helper extracted from `apply_secession` founds the new polity; `Planet.owner`, `Fleet.owner`, `TradeHouse.polity`, `HolderId::State`, `Bond.issuer` are remapped, treasuries added, minds averaged by population, planets tagged `Peaceful`; a member player leads the union.

**War.** Score, positive for the aggressor: battle `+20·destroyed value share`, occupied planets (M5) `+60·population share`, `−0.3·Δweariness`. Aims: Abwehr, Grenze (at most 3 named planets), Unterwerfung, Annexion. Terms cost `60·ceded population share + 80·[subjugation] + 5·tribute years` and never exceed the stated aim; the loser accepts when `cost ≤ lead + 0.4·weariness`; at 100 the winner dictates. Peace sets a 10-year truce. Subjugation: `overlord` set, 25 % of monthly state revenue moves treasury to treasury, no own treaties, follows the overlord's wars, planets tagged `Forced`.

**Aftermath, Victory (monthly).** `polity::settle_territory` moves a lost capital, sets `alive = false` at zero planets (nothing does today) and hands state assets to the absorber; conquest tags `Forced`, Übertritt (`Secession.joins`) `Peaceful`. The first matching rule sets `outcome` once; then `game_running` (`lib.rs`) is false and `Headless::run_days` returns:
1. no sovereign, or population below 30 % of its peak for 24 months: Auslöschung without survivor, alle verlieren;
2. one sovereign: Metamorphose if `pop(Peaceful) > pop(Forced)`, otherwise Auslöschung.

A player who stops being sovereign has lost: `player_out_day` is set, `config.player = None`, the run continues to its ending.

**Why it ends, unscripted.** (a) Sovereigns increase only by Freistaat; `apply_secession` caps Freistaaten ever founded (`max_free_states` = 6), so their number eventually only falls. (b) Reserves only fall (`economy/production.rs`); with the hard-coded floor 0.02 moved to `balance.ron` as 0, mining ends, machines need ore, capital decays: rule 1 is a physical backstop no stalling escapes. (c) Long before that, declining output drives `want` to 1 for everyone: the sovereignty premium disappears and blends drift toward expansionist unless `bind` holds them, so unbound pairs get `U_strike` and bound pairs `U_bind` above the threshold; a grid test over (ratio, window, dep, affinity, trust) fails the build if any cell leaves both sides waiting. (d) Geology differs, so trends differ: whoever falls behind sees `window` open. No timer exists; the clock is visible as reserve bars.

## 4. Commands

```rust
Propose { to: PolityId, deal: Deal }      // Treaty(TreatyKind) | Peace(PeaceTerms) | Merge
Answer { proposal: u32, accept: bool }
Denounce { with: PolityId, kind: TreatyKind, notice: bool }  // true: ends in 180 days; false: breach
DeclareWar { target: PolityId, aim: WarAim }                 // aim: #[serde(default)]
SetEmbargo { target: PolityId, on: bool }
```

`MakePeace`, today a unilateral end of war, becomes a status-quo `Propose`. Foreign stakes need nothing new: `BuyShares`, `TakeoverBid` and `OfferShares` already accept any issuer.

Validation extends `diplomacy::check_pair`, identical for `CommandSource::Player` and `Ai`: both alive and sovereign; ladder Handel → Nichtangriff → Forschung, Investitionsschutz → Beistand → Union, each rung needing the one below; `Merge` only while its conditions hold; one open proposal per pair and kind, lapsing as declined after 60 days, blocked 360 days after a refusal; `DeclareWar` is rejected while a Nichtangriffspakt stands (denounce first, so betrayal is an explicit, recorded act); war inside a truce counts as breach; `Expropriate{Foreign}` is rejected against Investitionsschutz partners. Clues and tension are written in `command::execute`, blind to the source. AIs answer within 10 days with a truthful `Reason`, their largest negative utility term (Misstrauen, ZuWenigNutzen, BindungZuEng, Bedrohung, Kriegsmuede): deception exists in deeds, never in stated reasons.

## 5. AI behaviour

`ai/strategy.rs`, in `SimSet::Ai` after `steward_finances`, polities in id order, no player-specific branch: the player is read through its dossier like everyone else.
- **Economy:** `steward_finances` as today; a Strike plan sets `SetTreasuryTarget` to 12 months of revenue.
- **Fleets:** M4 yard commands while `upkeep < revenue·(arm.0 + arm.1·threat)`, designs from `GameData::best_design`; paranoid blends `Guard` each `StarSystem.chokepoint`, expansionist blends buy capital ships, opportunist blends corvettes for `Raid`, cooperative blends `Escort`.
- **Deployment:** the `forward` share faces the neighbour with the highest `U_strike`; a Strike plan must hold it above 0.5 for `prep_days` before `DeclareWar`: every AI attack has an observable lead time.
- **War:** Grenze: `Blockade` and siege (M5) of the claimed systems; Unterwerfung, Annexion: battle where `win > 0.6`, then the capital; opportunist blends raid the largest lane in `Logistics.flows`; `win < 0.35`: `Guard` at home; peace offer at weariness above 50.
- **Treaties:** proposes the next rung when `U_bind > 0.15`; answers on `merge·benefit + trust − bind_cost[kind]·(1 − open) − grab·[sender is my target]`.
- **Foreign shares:** `0.15·open` of the surplus monthly, issuer score `spread·yield + (1 − spread)·closeness to foreign_pull`: cooperative minds scatter minority stakes and fill `merge_share`, expansionist minds concentrate until a region changes sides; low `open` buys out or expropriates foreign owners.

## 6. Feedback into economy, finance and cohesion

- `finance/cohesion.rs`: "Fremder Besitz" in `CohesionFactors::contributions` × `(1 − affinity)` under Investitionsschutz, which also suppresses Übertritt to that partner; without it `foreign_pull` stays the takeover lever. New contribution "Fremdherrschaft" on `Forced` planets, fading with `Region.since_day`.
- `finance/shares.rs`: `risk_discount += 0.10·tension/100` (markets price fear before war); `shock_polity` 0.9 on a treaty breaker.
- `finance/bonds.rs`: `bond_yield += 0.0004·tension`; in `issue_bonds` foreign appetite 0.35 × (0.3 + 0.7·[Investitionsschutz]), none under embargo.
- `finance/investors.rs`: `foreign_invest_share` per target × `(1 + rungs/3)·affinity`: peace grows cross-holdings.
- `command.rs`: `Expropriation::wronged`, returned but dropped today, feeds tension, affinity and a clue.
- `logistics/mod.rs`: `open_for` closes under embargo; without Handelsvertrag 5 % of sale proceeds go by `economy::pay` to `economy.treasuries[owner]`.

## 7. User interface

`ui/panels.rs::top_bar` gets "Diplomatie" and "Analyse"; the Krieg/Frieden buttons leave `polities_panel`; new `ui/diplomacy.rs`.
- **Diplomatie:** per community treaty rungs, Krieg or Waffenruhe, Spannung, kulturelle Nähe, Beteiligungen and Handelsabhängigkeit in both directions; Vorschlag (valid rungs only, with "Annahme geschätzt 40–60 %" = `Σ b_d·P_d` and band), Kündigen (mit Frist, sofort), Krieg erklären with Kriegsziel, Frieden anbieten, Embargo; Eingang with deadline and the sender's reason.
- **Analyse** (reads only `dossiers[(player, x)]`): four bars with bands ("kooperativ 45–65 %"), Stand der Aufklärung; Indizien, dated, with source and direction ("Hielt den Pakt trotz Gelegenheit: spricht für kooperativ"); Druckbild: Verflechtung, Gelegenheit, Bedrohung, Knappheit as estimated arrows; banner "Das jüngste Verhalten passt nicht mehr zur bisherigen Einschätzung"; Trend with band; "Was wir nicht wissen": occasions never observed and what would show them; "Unser öffentliches Bild" (`public_face`). Never weights, profiles or utilities; no reveal at the end.
- **Ende** (`AppState::GameOver`): Metamorphose or Auslöschung with the population tally by path.

## 8. Tests

1. `every_game_ends_once`: 24 AI-only seeds, `harness::run_until_end(300 years)`: outcome set, hash frozen afterwards, no year-limit hit.
2. `truth_never_leaks`: permuting `Minds` without stepping leaves `dossiers`, `Chronicle` and the UI view model hash-identical; `Mind` fields are private to `ai`.
3. `behaviour_identifies_archetype`: one pure mind, drift off, 40 years, 8 seeds: the true archetype ranks first in at least 75 % of foreign dossiers, more clearly at intel 1 than at intel 0.
4. `pressure_shifts_and_is_noticed`: a scripted player breaks a pact with a cooperative AI: `weights[par]` +0.10 within 12 months; the player's cooperative share in `public_face` halves; the player's dossier on that AI sets `break_day` within 36 months.
5. `no_attack_without_tell`: every AI `DeclareWar` in 16 seeds follows an Aufmarsch clue in the target's dossier by 60 days to 16 months.
6. `merger_thresholds_exact`: no merger at each threshold minus ε, merger at it; `money_drift == 0`; old polities dead, planets `Peaceful`, player remapped.
7. `subjugation`: a vassal is not sovereign, cannot `Propose`, pays tribute without money drift; subjugating the last rival gives Auslöschung.
8. `collapse`: depletion × 1000 gives Auslöschung without survivor.
9. `no_deadlock_grid` (3c); existing determinism, replay and save/load tests run 60 years with diplomacy commands.

Sweep columns (`RunSummary`, `headless sweep`): ending, end year (median, p10, p90), year-limit hits (a defect), collapse share (below 10 %), wars, mergers, subjugations, breaches, initial archetype of the final sovereign, dominance changes, assessment accuracy at years 20, 50, 100 (target 0.45–0.75: 0.25 is unreadable, 0.9 trivial), months from a dominance change to dossier majority.

## 9. Risks

- Minds too easy, too opaque or overconfident; AIs too passive or too warlike: sweep bands; levers `k`, clamp, blur, half-life, temperature, cooldown.
- Faster depletion and a zero reserve floor disturb the tuned M2/M3 economy: retune with `cargo headless economy` first.
- M4/M5 names unsettled: one adapter `ai/military.rs` (fleet value, build, deploy, siege), fallback `Fleet.strength`.
- Player exploits: proposal spam (block), cheap peaceful annexation (`open` response, `price_impact`), reputation laundering (same half-life for all).
- The save file contains the truth: serialise `Minds` as an opaque blob; no further anti-cheat.

### Critical Files for Implementation
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/diplomacy/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/ai/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/harness.rs