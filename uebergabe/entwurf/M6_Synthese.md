# KONFLUENZ M6: final implementation specification

Short paths are relative to `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/`; `crates/…` and `assets/…` are relative to the repository root. Identifiers are English, their `label()`s German.

## 1. Core idea

1. Each AI community has a hidden mind: a blend of four archetypes drifting around a fixed temperament under four publicly computable pressures.
2. One function, `choice_probs(profile, occasion)`, decides for the AI (blended profile, private picture) and scores every observable act for observers (pure profiles, public picture): assessment is inference, never reads a mind, and treats the player like any AI.
3. Securities act outward: mutual stakes, treaties and lived culture lead to union; one-sided majorities pull regions and whole communities over; war sequesters income owed to enemy holders.
4. Coexistence is a slope: capital crosses borders, growth differs, deposits run out, no war restores the status quo.
5. One pure `classify` writes one ending; new free communities are capped; exhausted deposits are the physical backstop.

## 2. State

`sim_state!` (`state.rs`) gains `minds: ai::Minds` and `endgame: endgame::Endgame`; `Diplomacy` is extended; `SAVE_FORMAT = 2`. Per-community vectors (`Polities.list`, `Economy.{treasuries, tax_rates, treasury_targets}`, `Finance.polities`, `Diplomacy.{standing, culture, public_face}`, `Minds.list`, M4 state) grow only in `polity::spawn`.

```rust
// diplomacy/mod.rs (pub fields; pair arrays: index 0 = lower id)
struct Diplomacy {
    relations: BTreeMap<(PolityId,PolityId),Relation>,  // key (lower, higher)
    standing: Vec<Standing>,
    culture: Vec<[f32;3]>,       // public: Offenheit, Gemeinsinn, Wehrhaftigkeit, 0..1
    proposals: Vec<Proposal>,    // id, from, to, deal, made_day, declined_day
    acts: VecDeque<Act>,         // public, newest 256: day, actor, toward, kind, option, value
    dossiers: BTreeMap<(PolityId,PolityId),Dossier>,    // (observer, subject)
    public_face: Vec<Dossier>,   // public clues only
    output_prev: Vec<f32>,       // per planet
}
struct Standing { overlord: Option<PolityId>, voluntary: bool, since_day: u64,
    econ: f32, trend: f32, scarcity: f32, capital_restless: u32 }
struct Relation { treaties: [Option<Treaty>;6],         // by TreatyKind: since_day, ends_day
    war: Option<War>, truce_until: u64, embargo: [bool;2], opened_until: [u64;2],
    tribute: Option<Tribute>,    // payer, rate, until_day
    grievance: [f32;2], tension: f32,                   // 0..1; 0..100
    stake: [f32;2], dep: [f32;2], tie: f32, merge_months: u32 }
struct War { attacker: PolityId, aim: WarAim, since_day: u64, pop0: [f64;2], fleet0: [f32;2],
    taken: [f64;2], lost: [f32;2], blockade: [f32;2], escrow: BTreeMap<HolderId,Money> }
enum TreatyKind { Trade, NonAggression, Research, Investment, Defence, Union }
enum WarAim { Limited, Subjugation }
struct Dossier { logit: [f32;4], weight: f32, surprise: f32, break_day: Option<u64>,
    clues: VecDeque<Clue> }      // 16: day, kind, toward, delta, source
// ai/mind.rs (fields pub(in crate::ai), no getters)
struct Minds { list: Vec<Mind> }
struct Mind { anchor: [f32;4], weights: [f32;4], pressure: [f32;4],
    plans: BTreeMap<PolityId,Plan> }                    // Strike(aim) | Bind, ready_day, until_day
// endgame.rs
struct Endgame { path: Vec<Path>, absorbed_pop: Vec<f64>, gap: Vec<f32>, seat: Vec<bool>, // per planet; Own | Joined | Forced
    peak_population: f64, months_below: u32, free_states: u32,
    player: PlayerFate,          // Active | Merged | Out
    outcome: Option<Outcome> }   // ending (Metamorphosis | Extinction), survivor: Option<PolityId>, day, joined, forced
```

*Free* = alive, no overlord; *head(p)* = overlord or p; *bloc* = head plus wards; *sovereign* = free with bloc population ≥ `sovereign_min_share` of all. `at_war`, `enemies_of`, `is_at_war` keep their signatures and resolve heads, so wards share the wars of their head. Wars are pairwise between heads; allies fight their own war and may make a separate peace.

New `assets/data/diplomacy.ron` → `GameData.diplomacy` (levers of §3, four profiles); `polities.ron` gains `culture: (o, g, w)`; `balance.ron` gains `economy.reserve_floor` and `military.counter` (6×6 class counter table from `cargo headless battle`).

```
               arm0 arm1 forward grab prevent merge honor spread open patience caution
Cooperative    .08  .15  .15     .05  .15     1.0   1.0   .9     .9   25       .5   „kooperativ“
Opportunistic  .12  .20  .40     .8   .2      .5    .25   .5     .7    8       .3   „opportunistisch“
Paranoid       .22  .45  .10     .1   1.0     .25   .7    .3     .15  12       .9   „misstrauisch“
Expansionist   .25  .20  .70     1.0  .5      .15   .5    .1     .5   15       .2   „expansionistisch“
```

Anchors: one shuffle of the archetypes from `Rng::stream(seed, Domain::Ai, 0, 0)`; community k (Freistaaten included) gets `order[k mod 4]` with 0.55, the others 0.15, independent of its template.

## 3. Tick order and algorithms

`SimSet::Ai`: daily answers and plans, monthly `think` (§5). `SimSet::Diplomacy`: daily expiry and evidence, monthly survey. `SimSet::Aftermath`: `WarEvents` every tick, monthly settlement. `SimSet::Victory`: monthly `judge`. No fine ticks.

**Survey** (`diplomacy/survey.rs`, per bloc):
- `econ`: share of `Firm.revenue`; `trend += (12·ln(R_now/R_prev) − trend)/trend_months` over the planets owned now (`output_prev`), so territorial jumps never read as growth.
- `scarcity` σ = 1 − Σrichness·reserve/Σrichness (own non-renewable deposits).
- `stake[a→b] = H_ab/V_b` (holders of a in issuers on territory of b; `finance::cross_holdings`, moved from `crates/game/src/ui/finance.rs`); `tie = min(H_ab,H_ba)/min(V_a,V_b)`, size-neutral.
- `dep_ab` = ½·share of b in the `Logistics.flows` of a + ½·H_ab/portfolio of a.
- Culture is lived policy: `c_a += culture_rate·(lived_a − c_a) + Σ_b contact_rate·dep_ab·m·(c_b − c_a)`; `lived = (bind, mean CohesionFactors.dispersion, min(1, upkeep/revenue/0.3))`, bind = mean of trade and portfolio shares abroad; m = 1.5 with Trade, 3 with Union, 0 in war or embargo; `aff = 1 − |c_a − c_b|/√3`.
- `tension += tension_adjust·(target − tension)`, target as point list: „Embargo“ +20, „Vertragsbruch“ +30·grievance (decaying by `grievance_decay`), „Aufmarsch an der Grenze“ +25·forward share, „Eroberer“ +20 while the other wages a Subjugation war, „Kulturelle Ferne“ +20·(1−aff), „Verträge“ −6·rungs, „Verflechtung“ −20·min(tie/merge_tie,1). `strain_p = max_j tension/100`.

**Picture** (pure, `diplomacy/occasion.rs`), i facing j. Foreign fleets F̂ and technology only via `diplomacy/sight.rs`: `estimate(observer, subject) → (lo, hi)`, read at `lo + caution·(hi−lo)`; `intel` = M4 intel + 0.25 Research + 0.2 Defence + 0.4 Union + 0.2·dep ≤ 1. `P = 0.5·econ + 0.35·fleet share + 0.15·technology share`; hops between capitals.

```
ratio  = ln(P_i/P̂_j)       gain = clamp(P̂_j/P_i, 0.3, 1)
cross  = max(0,ratio)/(trend_j − trend_i)                  // years until j overtakes i
window = [trend_j > trend_i]·clamp(1 − cross/patience, 0, 1), +0.3 if P̂_j > leader_share
win    = 1/(1 + (def_adv·(F̂_j + Σ_Defence(j) F̂_k)·(1+hops/8) / (F_i + Σ_Defence(i) trust_ik·F̂_k))²)
trust  = Σ_d belief_ij[d]·honor_d
threat = clamp((tension/100 + belief[Exp] + ½·belief[Opp])·clamp(0.3−ratio, 0, 1), 0, 1)
common = max_k min(threat_ik, threat_jk)
want   = clamp(σ_i + clamp(−trend_i·patience, 0, 1), 0, 1), ×0.3 while i can still claim a planet
U_strike = (grab·gain·(1+want) + prevent·window)·win − dep − honor·(rungs/5 + [truce]) − weariness/100
U_bind   = merge·(window + dep + aff)·trust + prevent·common − 0.6·(1−want)·(1−window)
```

**`choice_probs`** = softmax(U/`temperature`):

| Occasion (clue weight k) | Options | Utilities |
|---|---|---|
| Opening: yearly per ordered free pair, month (7i+3j) mod 12 (1.5; „Vertragstreue“ if rungs ≥ 2) | Strike, Bind, Wait | U_strike, U_bind, 0; both below `act`: (tremble, tremble, rest) |
| Answer (0.8; one clue per pair, kind, year) | accept, decline | U_accept, 0 |
| Terms (1.0) | Subjugation / Limited at declaration; full / mild as victor | `grab·gain·(1+want) + prevent·window − 0.5` / 0; `grab − honor/2` / `merge·aff` |

`U_accept`: Treaty k: `U_bind + trust − rung_cost[k]·(1−open) − grab·[sender is my Strike target]`; Swap: `U_bind + open − 0.5`; CallToArms: `honor + merge·trust − (1 − win against the aggressor) − dep on it`; Protectorate as ward: `max threat + merge·trust·aff − 0.8·(1−want)·[P ≥ minor_share]`, as lord: `grab + merge·aff − 0.2`; Merge: `U_bind − lead_cost·[lead ≠ me]·P_me/(P_me+P_j)·(1−window)`.
Continuous occasions (k 0.6, yearly, Gaussian, s = 0.08 + blur): Arming, upkeep/revenue against `arm0 + arm1·threat`; Massing, fleet share within 2 hops of systems of j against `forward` (also logged the day it crosses 0.5, which j always sees); Investment, 1 − HHI of state stakes abroad against `spread`.

**Evidence.** `command::execute`, blind to the source, logs every diplomatic act (Strike at the declaration, Bind at the first proposal of a year, Wait, „Zurückhaltung“, at year end if the uniform profile was tempted). Daily, `diplomacy/evidence.rs` (never imports `ai`) scores it per observer on the context of that observer: own estimates, private beliefs of the actor replaced by `public_face`, plan terms 0. `ll_d = ln P_d` (Gaussian: −(y−μ_d)²/2s²), shifted to maximum 0, clamped to [−3,0]; `logit += k·vis·ll`, `weight += k·vis`; vis = 1 for public acts, `intel` for Arming and Massing (y blurred by 0.25·(1−intel), `Domain::Intel`). Belief = softmax(logit), band ±0.35/√(1+weight), decay by `half_life_months`. `surprise` (smoothed surprisal minus its expectation) above `surprise_break` sets `break_day` and halves logit and weight.

**Minds** (monthly, on the survey of last month: the AI sees what the player saw). Pressures bind, gain = max_j clamp(ratio−0.3,0,1)·(1−dep), threat = max_j threat, want feed Cooperative, Opportunistic, Paranoid, Expansionist: `target = normalize(anchor_weight·anchor + pressure)`, `weights ← normalize(weights + drift·(target − weights))`. Jolts: breach suffered, Paranoid +0.20; ally honoured a call, Cooperative +0.10; subjugation won, Expansionist +0.06.

**War** (`diplomacy/war.rs`). `WarEvents`, a transient resource, is written by SpaceCombat (`BattleEnded { losses: Vec<(PolityId, lost_value, initial_value)> }` from `battle::Combatant`) and Siege (`PlanetTaken { planet, by }`) and drained by `SimSet::Aftermath` in the same tick. Attacker score, −100..100: `50·(taken_a/pop0_d − taken_d/pop0_a) + 25·(lost_d/fleet0_d − lost_a/fleet0_a) + 15·clamp(blockade_d − blockade_a, −1, 1) + 10·(weariness_d − weariness_a)/100` (blockade in population-share-years, `Fleets::is_blockaded`). Taken planets change owner at once; conquests stay. Terms favour the lead s: below 20 status quo; from 20 also reparations (`reparations` of monthly `PolityFinance.revenue`), equity (≤ `equity_max` of the holdings of the loser State, `finance::distribute_shares`), imposed opening (no embargo or expropriation against the victor), each ≤ `terms_years`, and the escrow of the loser side; from 55 or with the capital taken, subjugation (an attacker needs aim Subjugation). Diktat: both weariness ≥ `exhaustion` → status quo; score ±100 or `war_max_months` → band maximum. Peace returns remaining escrow and sets `truce_until` (`truce_years`). Subjugation: `overlord` = victor, planets `Forced`, tribute `vassal_tribute`, no diplomacy; its wards pass on. No independence in M6; after `annex_years` a ward is absorbed with its tags.

**Peaceful absorption.**
- Merger: Union for `merge_union_months`, `tie ≥ merge_tie`, `aff ≥ merge_affinity`, held `merge_hold_months` → `Deal::Merge { lead }` valid; both confirm → `absorb(other, lead, Joined)`; the lead slot is refounded („{lead.short}-{other.short}-Union“; culture, mind, dossiers population-weighted). A human accepting another lead gets `PlayerFate::Merged` and watches.
- Protectorate by proposal: `voluntary`, planets `Joined`, tribute `protectorate_tribute`.
- Whole accession, the capital analogue of `foreign_pull` (capitals never turn autonomous, `finance/cohesion.rs:423`): `stake[a→b] ≥ accession_stake`, `aff ≥ accession_affinity`, capital of b `Stage::Restless` for `accession_months`, no war or embargo → `absorb(b, a, Joined)`.
- Regional accession (`Secession.joins`) is `Forced` under war, blockade or imposed opening towards the gainer.

**Victory** (`endgame::judge`; pure `classify(&Census) → Option<Outcome>`; first match, written once, with a `ChronicleKind::End` entry):
1. Population below `collapse_share`·`peak_population` for `collapse_months`, or no sovereign: Extinction without survivor, alle verlieren.
2. Exactly one sovereign: joined and forced = Σ `absorbed_pop` over the planets of its bloc by path; `joined > forced` → Metamorphosis, else Extinction. Free rumps below the floor are ignored.
A player whose community is absorbed or becomes a ward: `PlayerFate::Out`, `config.player = None`; the run continues.

**Why every game ends once, unscripted.** (a) One total function, one write. (b) Free communities increase only by Freistaat, capped per game (`max_free_states`); absorption, subjugation and protectorate are irreversible. (c) Reserves only fall and `reserve_floor` is 0: mining tends to zero, machines need Erz, Silikate and Seltene Erden, capital depreciates 4 % a year, food needs capital, so rule 1 fires if rule 2 has not (README: deposits no longer follow „Engpässe sind nie endgültig“). (d) Earlier, `want` rises for all: `U_strike` grows, the sovereignty premium in `U_bind` vanishes, differing geology opens windows, `tremble` keeps every opening possible, and the loader rejects deadlock cells (§8). Targets: space full near year 40, median ending 100–160, physical horizon 200–240, sweep limit 250 years.

**Levers** (`diplomacy.ron`): act=0.15 temperature=0.10 tremble=0.002 plan_years=2 prep_days=180 anchor_weight=1.0 drift=0.04 def_adv=1.2 minor_share=0.06 leader_share=0.40 trend_months=60 culture_rate=0.004 contact_rate=0.02 tension_adjust=0.08 grievance_decay=0.03 strain_risk=0.10 strain_yield=0.04 half_life_months=60 surprise_break=1.2 notice_days=180 proposal_days=60 refusal_block_days=360 truce_years=5 rung_cost=(0.2,0.4,0.5,0.6,0.9,1.2) merge_union_months=24 merge_tie=0.15 merge_affinity=0.70 merge_hold_months=12 lead_cost=0.6 accession_stake=0.5 accession_affinity=0.6 accession_months=24 annex_years=20 vassal_tribute=0.15 protectorate_tribute=0.05 war_max_months=144 exhaustion=85 reparations=0.10 terms_years=10 equity_max=0.25 breach_shock=0.9 foreign_rule=20 foreign_rule_years=20 max_free_states=6 sovereign_min_share=0.02 collapse_share=0.30 collapse_months=24 tariff=0.0. `balance.ron`: `reserve_floor` 0.0, `depletion` 0.000001 (confirmed in step 1); `weariness_per_month` stays 0.6.

## 4. Commands

```rust
DeclareWar { target: PolityId, #[serde(default)] aim: WarAim }
Propose { to: PolityId, deal: Deal }
Answer { proposal: u32, accept: bool }
Denounce { with: PolityId, kind: TreatyKind }
SetEmbargo { target: PolityId, on: bool }
enum Deal { Treaty(TreatyKind), Swap { value: f64 }, Peace(PeaceTerms), CallToArms { against: PolityId },
            Protectorate { ward: PolityId }, Merge { lead: PolityId } }
struct PeaceTerms { tribute_years: u8, equity: f32, opening_years: u8, subjugate: bool }
```

`MakePeace` is removed. `command::execute` validates (extending `diplomacy::check_pair`; same for every `CommandSource`; German reasons): actor alive; diplomatic commands need both free.
- `DeclareWar` ends the treaties of the pair; a breach while NonAggression, Defence, Union or a truce holds; Defence partners of the target receive `CallToArms`.
- Breach (`diplomacy::breach`): grievance of the victim = 1, treaties of the pair end, `shock_polity(breaker, breach_shock)`. Also `Expropriate` hitting an Investment partner (via `Expropriation.wronged`, dropped today in `command.rs`). Declining a call only ends Defence.
- `Propose`: one open proposal per pair and deal kind; blocked `refusal_block_days` after a decline; lapses after `proposal_days` (a lapsed call counts as declined). Treaty: no war or embargo; NonAggression, Research (+10 % research points for both, M4), Investment need Trade; Defence needs NonAggression; Union needs Investment and Defence. Swap: needs Investment; each State gives the other shares worth `value` from its home holdings (`distribute_shares`). Peace: within band and aim. CallToArms: Defence, caller attacked. Protectorate: ward not at war. Merge: conditions of §3.
- `Answer`: addressee only, prerequisites rechecked. `Denounce`: the treaty and rungs needing it end after `notice_days`. `SetEmbargo`: rejected under Trade, Union or imposed opening.
- `BuyShares`, `TakeoverBid`: rejected across war or embargo, otherwise uncapped (one-sided majorities stay the hostile path).
- `MoveFleet`: every owned system on the route (`Galaxy::system_owner`) must be own bloc, enemy, or Defence or Union partner, else „keine Durchfahrt“.

AIs answer 3–10 days after `made_day`. A decline shows a `Reason` (Misstrauen, ZuWenigVerflechtung, KulturelleFerne, StufeZuHoch, Kriegsmüde): the most negative term of `U_accept` under the uniform profile on public data, hence the same for every mind and useless for probing.

## 5. AI behaviour

`ai/think.rs`, chained after `steward_finances` in `SimSet::Ai`, for every free community except `config.player`; wards run only `expand`, `steward_finances` and posture. No branch on the player. Foreign military data only via `sight`.
- Opening: draw from `choice_probs(blend, private occasion)` with `Rng::stream(seed, Domain::Ai, i<<16|j, year)`; the plan (Strike with aim from Terms, or Bind) stands `plan_years`.
- Strike: forward share ≥ 0.5 for `prep_days`; declare only with `win ≥ 0.35`, local superiority and stocks ≥ 70 %, else lapse.
- Bind: next rung; `Swap` (tranches of 2 % of the smaller economy) and purchases until `tie ≥ merge_tie`; the stronger proposes `Merge { lead: self }`. Minors (`P < minor_share`) ask the most trusted larger community for protection once that utility reaches `act`.
- Peace: propose at score ≤ −20 or weariness ≥ 50, or as victor from 55; accept terms within band on the same conditions; status quo only after 12 months, without lead and with `win < 0.5`.
- Posture Frieden/Rüstung/Krieg (replaces the war block of `steward_finances`): `SetTaxRate` 0.15/0.20/0.26 (−0.02 a month while a region is below `restless_below`+5), `SetTreasuryTarget` 0/12/24 months of revenue, `IssueBonds` (6 months) below 3 months of treasury while `bond_yield < 0.20`.
- Fleets via `ai/levers.rs`, the only file naming M4/M5 commands: build while upkeep < revenue·(arm0 + arm1·threat), ×1.5 with a Strike plan, ×2.5 at war, at most 0.8; class = argmax Σ_e mix_e·`counter[c][e]` among `GameData::best_design`. In peace the forward share waits at the own system nearest the most tempting target (prefer `StarSystem.chokepoint`), the rest `Guard` or `Escort`; in war: battle where local `win ≥ 0.6`, then `Blockade` and siege of the most populous reachable system, `Raid` on the largest lane in `Logistics.flows`, docking below 30 % supplies.
- Shares: `0.15·open` of the surplus monthly. Bind: spread over issuers of the partner (`BuyShares`, `TakeoverBid` at 0.12). `U_strike > U_bind`: concentrate on the least cohesive non-capital region of j up to `foreign_pull`+0.05 (bid 0.4). Defence: buy back and `DistributeShares` to the local fund where a non-partner holds over 0.25; `Expropriate` (scope `Foreign`) only if `open < 0.3`.

## 6. Hooks into existing systems

- New `polity/succession.rs` owns identity changes, with exhaustive destructuring. `spawn(parent)`: culture of the parent, mind from the deck, empty dossiers. `succeed(from, into)` relabels `Planet.owner`, `TradeHouse.polity`, `Fleet.owner` (disbanded if forced), `Bond.issuer` and `holder`, `HolderId::State` keys in `Issuer.holdings` and `offered`; moves the treasury (added to `PolityFinance.last_treasury`); cancels self-held bonds; passes wards; merges research (M4); drops proposals, plans, dossiers; settles escrow; clears `alive` and `capital`. `absorb(from, into, path)` and `transfer_planet(planet, to, path)` are the only ways a community ends or a planet changes owner; they set `path`, `absorbed_pop` (population then; immune to `economy::migrate`) and `gap` (cultural distance then). Joining retags only `Own` and `Joined` planets: `Forced` survives every merger. A joined capital becomes a `seat`; a lost capital moves to the most populous remaining world, none left → `absorb(.., Forced)`.
- `finance/cohesion.rs`: `apply_secession` uses `spawn` and `transfer_planet`; a Freistaat needs `free_states < max_free_states`. `ownership_of_region` takes `&Diplomacy`: holders of a Union partner count under `regions`. „Fremder Besitz“ ×(1−aff) for Investment partners, who exert no pull. New: „Fremdherrschaft“ −`foreign_rule`·(0.5+0.5·gap)·(1 − years/`foreign_rule_years`) on `Forced` planets, which get no „Aufbruchstimmung“; seats count as capitals („Bundeswelt“).
- `finance/shares.rs`: `risk_discount` += `strain_risk`·strain; `pay_dividend` and `bonds.rs::service_bonds` book amounts owed to holders at war with the payer into `War.escrow`; `state::money_drift` adds `Diplomacy::escrow_total()`.
- `finance/bonds.rs`: `bond_yield` += `strain_yield`·strain; `issue_bonds`: foreign appetite 0.35 → 0.525 under Investment, 0 under embargo. `finance/investors.rs`: foreign candidates ranked by yield·(1+rungs/3)·(0.5+aff), none across war or embargo. `finance/plugin.rs::monthly_books`: tribute (reparations, wards) by `economy::pay`.
- `logistics/mod.rs`: `open_for` also closes under embargo, ports and transit alike; `move_and_trade` charges `tariff` on foreign sales without Trade (0 = today). `fleet/mod.rs::order_move`: access predicate instead of `|_| true`; fleets losing access are routed home. `economy/production.rs:186`: `.max(0.02)` → `.max(balance.reserve_floor)`.
- `command.rs`: `CommandContext` gains `minds`, `endgame`, `config`, mutable `logistics`. `lib.rs::game_running` requires `outcome.is_none()`. `harness.rs`: `Headless::run_days` and `run_days_exact` return once the outcome is set.
- `data/mod.rs` validates `diplomacy.ron`: ranges, `sovereign_min_share·max_polities < 1`, no deadlock cell. New systems are chained inside their `SimSet`.

## 7. Public API for the user interface

`diplomacy::view::build(.., observer) → PlayerView` is pure and takes no `Minds`. `crates/game` (paths below under `crates/game/src/`) never names `sim::ai` and shows foreign fleets and technology only through the view, replacing the exact strengths in `ui/fleets.rs`.
- „Diplomatie“ (new `ui/diplomacy.rs`, button in `ui/panels.rs::top_bar`; Krieg/Frieden leave `polities_panel`): per community Krieg/Embargo/Offen, Schutzgebiet/Vasall, treaty ladder with the reason on locked rungs, Spannung as point list, kulturelle Nähe, Beteiligungen both ways, Verflechtung against the threshold, Fusionsfortschritt. Actions: Vorschlag (valid deals; „Annahme geschätzt 40–60 %“ = Σ belief_d·P_d with band), Anteilstausch, Kündigen, Krieg erklären (aim; warnings Vertragsbruch, Sperrguthaben), Frieden (score components, admissible terms), Embargo; Eingang with deadlines.
- „Analyse“ from `dossiers[(player, x)]`: four bars with bands, Stand der Aufklärung, dated Indizien with source and direction, Druckbild (Verflechtung, Gelegenheit, Bedrohung, Knappheit), banner „Das jüngste Verhalten passt nicht mehr zur bisherigen Einschätzung“, „überholt uns in etwa N Jahren“, „Was wir nicht wissen“, „Unser öffentliches Bild“. Never weights or utilities; no reveal at the end.
- Endspiel strip: souveräne Gemeinschaften, Bevölkerung gegen Überlebensschwelle, beigetreten/genommen, Vorkommen. Warnings: region near `foreign_pull`, Gesamtbeitritt, Aufmarsch an unserer Grenze.
- `time_control.rs` pauses when the inbox grows, war is declared on the player or a merger becomes possible. `PlayerFate::Out`: „Die Partie ist verloren“, then „Beobachter“. `outcome` → `AppState::GameOver`: „Metamorphose“ or „Auslöschung“, tally, fate of the player.

## 8. Tests and sweep metrics

`harness::fast_data()` (4 clusters and communities, depletion ×8, drift ×4, waits ÷4, short `battle.time_limit`) keeps every test within 40 simulated years; 250-year runs on default data belong to `cargo sweep` and `#[ignore]`.
1. `classify_is_total`: table with collapse, tie → Extinction, wards by path, rumps ignored, two sovereigns → `None`.
2. `drives`: `U_strike` rises in window, falls in dep and rungs; `U_bind` rises in trust, dep, aff, common. `no_deadlock_grid` (the loader check): anchors × ratio (to 1:10) × window × dep × aff × trust × third party or none, at `want = 1` with stationary weights: one side has an exit ≥ `act`.
3. `truth_never_leaks`: permuting `Minds` without stepping leaves `Diplomacy`, `Chronicle` and the view hash-identical; beliefs are ¼ at day 0.
4. `succession_conserves`: union, forced absorption, whole accession, Freistaat keep `money_drift == 0`, share totals and population; `check_invariants` newly demands equal vector lengths and no reference to a dead `PolityId`; `Forced` survives a merger; accession under war, blockade or opening is `Forced`.
5. `merger_thresholds_exact`: no merger at a threshold −ε or without both confirmations; a junior human becomes `Merged`.
6. `war_bounded_and_banded`: no war outlasts `war_max_months`+1; terms beyond the band are rejected; a leading AI declines status quo in month 1.
7. `capital_is_hostage`: enemy dividends sit in escrow (drift 0), return at status quo, are forfeited from 20; `BuyShares` on an enemy issuer fails; no fleet route through a neutral; embargo stops freighters.
8. `wards`: cannot propose, pay tribute, share the wars of their head; subjugating the last rival → Extinction; a protectorate with joined majority → Metamorphosis.
9. `no_attack_without_tell`: every AI `DeclareWar` follows a Massing clue in the dossier of the target by 60 days to 16 months.
10. `pressure_is_noticed`: a scripted breach raises the Paranoid weight of the victim by 0.10 within a year (new sim feature `reveal`, enabled only by `crates/headless`) and halves the cooperative share of the breaker in `public_face`.
11. `behaviour_identifies_archetype`: pure minds, drift off, 8 seeds: the true archetype leads ≥ 60 % of foreign dossiers, more often at intel 1 than 0.
12. `collapse_is_physical`: depletion ×1000, passive AIs → Extinction without survivor; `run_days` returns; hash frozen; one End entry.
13. `every_game_ends_once`: 6 seeds AI-only plus script players `Holdout`, `ShareRaider`, `PactBreaker` (new in `harness.rs`); determinism, replay, save/load across a pending proposal and a war. Ignored: `merged_union_holds` (no `Joined` planet autonomous within 50 years).

Sweep columns (`RunSummary`, CSV): ending and year, limit hit (a balance defect), collapse (< 10 %), sovereigns at years 25/50/75/100, longest span without change in free communities (< 40 years), pair-months per mode, wars, mergers, accessions, subjugations, joined vs forced, `ai_rejected` (< 2 %), median tie of neighbours at year 60; with `reveal`: Brier score, top-archetype accuracy (0.45–0.75), opportunists rated cooperative before their first breach. Targets: median end 100–160, Metamorphosis 30–60 %, no start archetype prevailing above 40 %.

## 9. Implementation order

1. Seams and calibration: `reserve_floor`, `finance::cross_holdings`, reserve and tie columns in `cargo headless economy`; pick `depletion` so passive runs collapse in years 200–240 with existing tests green; `run_days` guard; `SAVE_FORMAT` 2.
2. Identity and endings without AI: `succession.rs`, `Endgame`, `classify`, invariants, `apply_secession`, `game_running`; `harness::inject_war_event` (captures testable before M5). Tests 1, 4, 12.
3. Relations, treaties, proposals, embargo, access, escrow, finance and cohesion hooks, minimal „Diplomatie“. Tests 5, 7.
4. War: `WarEvents`, score, bands, Diktat, wards, whole accession. Tests 6, 8.
5. Minds, discrete occasions, `diplomacy.ron`, loader check. Test 2.
6. Evidence, continuous occasions, view, „Analyse“, auto-pause. Tests 3, 9, 10, 11.
7. Operational AI: posture, `levers.rs`, shares.
8. Sweeps: script players, metrics, tuning, README. Test 13.

## 10. Risks and safeguards

- Stall, or collapse as the default ending: stall column, collapse share, loader check; levers `act`, `temperature`, `tremble`, `merge_tie`, `depletion`.
- Zero floor and faster depletion move the M2/M3 economy: step 1 first; if its tests cannot stay green, ship 0.02 and report the missing backstop as a defect.
- `merge_tie` is a guess until step 1 measures cross-holdings. Holders follow their planet, so conquest shifts stakes at once: hold period, double confirmation and escrow blunt it.
- Erratic choices or misleading evidence: yearly draws, persistent plans, visible tell; `temperature`, `k`, blur, half-life; discrete occasions ship first; recalibrate from sweep frequencies.
- Leaks: view signature, private `Minds`, uniform reasons, test 3; the save file contains the truth (accepted).
- M4/M5 names move and `tech` is absent in the checkout: `sight.rs` inbound, `levers.rs` outbound, `WarEvents`; fallbacks `Fleet.strength`, intel 0.3. Tick-by-tick battles (up to 9000 ticks) slow war-heavy sweeps.
- Exploits: truce shields (breakable; AIs decline early status quo), proposal spam (blocks, one clue a year), protecting a losing side (not while at war), share raids (purchase ban, forced tag).

## Notes

- Length: about 3500 words, above the 3200 asked for; cutting further would have removed rules the engineers need.
- Decisions where the judges or designs disagreed: union refounds the lead slot (no new id) but still goes through `spawn`/`succeed`; no 10 % cap on state stakes (purchase ban in war and embargo plus forced tags instead); war under a pact is allowed and counts as breach; conquests stay, so peace terms carry no planet lists; tariff starts at 0; culture is per community.
- Checked against the checkout (commit 0b4777c), read-only, no cargo run: every existing file, type, field and function named above exists; everything else is new. Absent today: a `tech` module, ships inside `Fleet` (still one `strength`), any siege code. Defaults and the 200–240 year horizon are untested estimates; step 1 and the sweeps decide.
- Also touched: `polity/mod.rs`, `state.rs`, `lib.rs`, `data/defs.rs`, `assets/data/balance.ron`, `assets/data/polities.ron`, `crates/sim/Cargo.toml` (feature `reveal`), `crates/headless/src/main.rs`, `crates/game/src/ui/{panels,finance,fleets}.rs`, `crates/game/src/time_control.rs`.
- Memory: one stored note is listed (checking the game window in a cloud container); not relevant, not opened, nothing from memory used. Worth remembering, learned from the code: the 0.02 reserve floor is hard-coded at `economy/production.rs:186`; nothing ever clears `Polity.alive` or `capital`; `crates/game` gates `SimRoot` on `AppState::InGame` and never names `sim::ai`; `command.rs` drops `Expropriation.wronged`.

### Critical Files for Implementation
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/diplomacy/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/ai/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/harness.rs