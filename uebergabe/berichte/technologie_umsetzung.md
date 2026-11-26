# Technologie: Bericht der Umsetzung (wip/m4-tech, c64ee9c)

## summary

Technology and research are implemented in the simulation core and committed on wip/m4-tech as c64ee9c (worktree clean, nothing pushed). All required commands pass; the game crate was not built, as instructed.

What exists now:
- New module `sim::tech` (mod.rs, step.rs, intel.rs, plugin.rs) with `Research` in `sim_state!` directly after `finance`, and `TechPlugin` running monthly in `SimSet::Research`.
- Monthly research: budget = `budget_share` x `finance.polities[p].revenue`, paid as researchers' pay from the treasury to the households of the community's planets (money conserved exactly). Points = credits x efficiency x (population/1000)^0.25 x research bonus.
- Spillover: a technology known by communities in contact is cheaper, by up to 35 %, weighted by lane-hop distance between territories; no exchange between communities at war.
- Tree rules, automatic target choice (branch weight for peace/war divided by remaining cost), commands `SetResearchTarget` and `SetResearchBudget` with German error texts.
- Effects: Yield and FreighterSpeed are wired into economy and logistics; everything else is exposed through `Research::effects`. `best_design` unlocks `speer_2` once `waffen_2` is known.
- Partial visibility: `estimate()` gives per-branch ranges that always contain the truth and narrow with intel; intel comes from ownership ties and `add_intel` events that fade.
- Secession: a new Freistaat inherits the knowledge of the community it left; a sync step at the start of every research step and before every research command keeps all per-community vectors complete.
- Starting technologies per template (`start_techs` in polities.ron), validated at load.
- `headless research` subcommand for the per-community history and a multi-seed overview.

Balance outcome with the default 10 % budget (24 seeds, 144 communities): first technology after about 4 years; at year 90 the mean tiers are 4.0/3.9/3.5/3.2/2.8; nobody finishes the tree within 150 years (maximum 28 of 30).

The economy stays healthy with research on, and with research disabled it reproduces the pre-change sweep exactly.

Memory: no stored notes existed, so nothing in this report comes from memory. Possibly worth saving:
- `PolityFinance.revenue` is the smoothed positive monthly change of the treasury, not gross income, and it jumps with one-off inflows such as bond proceeds (learned from crates/sim/src/finance/plugin.rs and a measurement run).
- The machine is shared with other agents: a 100-year sweep took 355 s under load versus about 65-75 s otherwise (observed in this session).

## measurements

All figures are from the release binary at c64ee9c with the default budget share of 10 %. Logs are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/tech.

1) RESEARCH PACE OVER ALL COMMUNITIES
`headless research --seeds 0..24 --years 150 --every 10` (144 communities, no invariant violations). Technology counts include the one starting technology. Tiers are means in the order economy, propulsion, weapons, defence, ground.

Year | techs min/mean/max | mean tiers | highest tier | tree finished
10 | 2 / 2.9 / 4 | 1.0 1.0 0.3 0.5 0.1 | 1 | 0
20 | 3 / 4.5 / 6 | 1.6 1.0 0.8 0.9 0.1 | 2 | 0
30 | 5 / 6.4 / 7 | 2.0 1.9 1.0 1.0 0.5 | 2 | 0
40 | 7 / 7.5 / 10 | 2.4 2.0 1.1 1.0 1.0 | 3 | 0
50 | 7 / 9.7 / 12 | 3.0 2.1 1.9 1.7 1.0 | 3 | 0
60 | 10 / 11.9 / 14 | 3.0 2.9 2.2 2.0 1.7 | 3 | 0
70 | 12 / 13.8 / 16 | 3.1 3.0 3.0 2.8 2.0 | 4 | 0
80 | 14 / 15.3 / 18 | 3.8 3.3 3.0 3.0 2.2 | 4 | 0
90 | 15 / 17.4 / 20 | 4.0 3.9 3.5 3.2 2.8 | 5 | 0
100 | 17 / 19.1 / 23 | 4.2 4.0 4.0 3.8 3.0 | 5 | 0
110 | 18 / 20.5 / 25 | 4.8 4.4 4.1 4.0 3.2 | 5 | 0
120 | 19 / 22.2 / 25 | 5.0 4.8 4.4 4.2 3.8 | 5 | 0
130 | 20 / 23.6 / 26 | 5.0 5.0 4.8 4.5 4.3 | 6 | 0
140 | 22 / 24.5 / 27 | 5.1 5.0 4.9 4.8 4.7 | 6 | 0
150 | 22 / 25.1 / 28 | 5.2 5.0 5.0 4.9 4.9 | 6 | 0

Against the targets:
- Tier-1 pace: first completion at 4.0-5.1 years in seed 1, later tier-1 technologies about 3.7-4.5 years apart.
- Tier 4..5 in a few branches around year 80..100: met (year 90 mean 4.0/3.9/3.5/3.2/2.8, highest 5).
- Nobody finishes within 150 years: met, maximum 28 of 30. With the earlier tier-6 cost of 250000, two of 96 communities (seeds 8 and 13) did finish, which is why tier 6 now costs 400000.

2) PER COMMUNITY, SEED 1
`headless research --seed 1 --years 100`. Columns: smoothed revenue / research pay / points per month (decade means), technologies known, tiers, current target.

Year 10
- Orden der Tiefen Uhr: 518 / 51.8 / 48.4, 3 known, 1 1 1 0 0, Schottverstärkung 53 %
- Irrlicht-Syndikat: 450 / 45.0 / 42.0, 2 known, 1 1 0 0 0, Frachtleitsysteme 43 %
- Karawane von Meridian: 509 / 50.9 / 47.4, 3 known, 1 1 0 1 0, Zielrechner 27 %
- Sonnwend-Kommune: 504 / 50.4 / 47.0, 3 known, 1 1 0 1 0, Zielrechner 49 %
- Thalassische Liga: 516 / 51.6 / 48.2, 3 known, 1 1 1 0 0, Schottverstärkung 31 %
- Halde-Konkordat: 474 / 47.4 / 44.5, 3 known, 1 1 1 0 0, Schottverstärkung 17 %

Year 30
- Orden: 514 / 51.4 / 51.8, 6 known, 2 2 1 1 0
- Irrlicht: 551 / 55.1 / 55.2, 5 known, 2 1 1 1 0
- Karawane: 802 / 80.2 / 80.5, 7 known, 2 2 1 1 1
- Sonnwend: 653 / 65.3 / 65.3, 7 known
- Thalassa: 535 / 53.5 / 54.5, 6 known
- Halde: 550 / 55.0 / 55.2, 6 known

Year 60
- Orden: 1089 / 108.9 / 119.9, 11 known, 3 3 2 2 1
- Irrlicht: 1595 / 159.5 / 172.5, 12 known
- Karawane: 2129 / 212.9 / 235.4, 13 known, 3 3 3 2 2
- Sonnwend: 1414 / 141.4 / 153.7, 12 known
- Thalassa: 1064 / 106.4 / 120.9, 11 known
- Halde: 1363 / 136.3 / 149.7, 11 known

Year 90
- Orden: 2597 / 259.7 / 324.4, 17 known, 4 4 3 3 3, Fokussierkristalle 53 %
- Irrlicht: 3392 / 339.2 / 404.3, 18 known, 4 4 4 3 3
- Karawane: 5288 / 528.8 / 663.2, 19 known, 4 4 4 4 3, Schwarmlogistik 68 %
- Sonnwend: 3265 / 326.5 / 404.4, 18 known
- Thalassa: 3462 / 346.2 / 458.1, 18 known
- Halde: 3800 / 379.9 / 479.2, 18 known

Year 100
- Orden: 2545 / 254.5 / 325.3, 19 known, 4 4 4 4 3
- Irrlicht: 3253 / 325.3 / 392.0, 19 known
- Karawane: 5831 / 583.1 / 746.5, 20 known, 5 4 4 4 3
- Sonnwend: 3733 / 373.3 / 476.4, 19 known
- Thalassa: 4959 / 495.9 / 674.7, 20 known
- Halde: 4220 / 422.0 / 550.5, 20 known

Completion years for Orden: 4.1, 7.8, 12.2, 21.8, 29.7, 32.2, 44.5, 48.8, 52.1, 58.9, 61.3, 66.4, 70.3, 78.3, 84.8, 87.4, 92.8, 98.2.

Research pay equals 10 % of revenue in every row, so the reserve rule does not bind for AI communities in peace. After 30 years the highest-revenue community knows at least as many technologies as the lowest in all 10 seeds probed (0..9).

3) SWEEP `--seeds 0..8 --years 100`, BEFORE AND AFTER

BEFORE (binary built from d883061):
Seed | Hash | free | alive | population (M) | largest community
0 | 2e2551a3b8a4bf85 | 0 | 6 | 79092 | Halde-Konkordat (22 %)
1 | 106bb9206b29debe | 0 | 6 | 76132 | Thalassische Liga (21 %)
2 | 22a1bdfd342a4b59 | 0 | 6 | 78995 | Sonnwend-Kommune (19 %)
3 | a2110877172a9dfb | 0 | 6 | 77734 | Halde-Konkordat (19 %)
4 | 6e21ac537549ac51 | 0 | 6 | 80964 | Karawane von Meridian (24 %)
5 | 29c8cac7b0d69912 | 0 | 6 | 76595 | Thalassische Liga (21 %)
6 | 2d0552ab284522a | 0 | 6 | 78667 | Thalassische Liga (20 %)
7 | e2c4226a31bff414 | 0 | 6 | 80729 | Aschgraue Räte (21 %)
Mittel: Versorgung 0.99, Auslastung 0.75, Stellen besetzt 0.67, Preisniveau 0.71, Preisspanne 5.1, Preise am Anschlag 5 %, Frachter 1026. No violations.

AFTER (c64ee9c):
0 | 118554a7f4b7e439 | 0 | 6 | 84316 | Halde-Konkordat (22 %)
1 | 1c0a318af64af839 | 0 | 6 | 79346 | Thalassische Liga (21 %)
2 | daa802ab0624ea87 | 0 | 6 | 81739 | Karawane von Meridian (18 %)
3 | d5185653069d2dee | 0 | 6 | 79509 | Halde-Konkordat (21 %)
4 | fa6eaf4491b422f3 | 0 | 6 | 83352 | Karawane von Meridian (22 %)
5 | 4d01cc7596b07003 | 0 | 6 | 80018 | Thalassische Liga (20 %)
6 | 1519623edf8bb99f | 0 | 6 | 84219 | Thalassische Liga (21 %)
7 | 19a7c29053e74425 | 0 | 6 | 82798 | Aschgraue Räte (21 %)
Mittel: Versorgung 1.00, Auslastung 0.75, Stellen besetzt 0.76, Preisniveau 0.73, Preisspanne 4.6, Preise am Anschlag 5 %, Frachter 916. No violations.

Means over the 8 seeds at year 100, from `headless research --seeds 0..8 --years 100`. The middle column is a variant with every Yield effect set to 0.0001, to isolate the Yield effect.

Metric | no research | research, Yield neutralised | full research
Population (M) | 78614 | 78404 | 81912 (+4.2 %)
Supply of essentials | 0.993 | 0.992 | 1.000
Utilisation | 0.752 | 0.762 | 0.751
Jobs filled | 0.673 | 0.668 | 0.759
Price level | 0.711 | 0.678 | 0.733
Cohesion | 68.6 | 68.8 | 67.5
Restless regions | 2.6 | 2.2 | 0.8
Autonomous regions | 0.0 | 0.0 | 0.0
Secessions | 0.75 | 0.75 | 0.50
Freighters | 1026 | 1052 | 916
Technologies per community | 0 | 18.4 | 19.2
Invariant violations | 0 | 0 | 0

Reading: research pay alone barely moves the economy. The Yield effect adds about 4 % population, full supply and higher employment, costs about one point of cohesion, and reduces the freighter count.

Neutrality check: with research disabled (budget_share 0, no start_techs) the new binary reproduces the pre-change sweep CSV in every column except the hash (population per community, supply, utilisation, employment, price level, spread, pinned prices, freighters).

150 years, 24 seeds, research on: population 119662 M, supply 1.000, utilisation 0.783, cohesion 65.9, 0.1 restless regions, 0.54 secessions, no violations.

Runtime: the after-sweep's 354.8 s was caused by other agents loading the machine. The same binary ran the 60-year sweep in 53.6 s, and the research-off 100-year sweep took 62.6 s against 74.8 s for the old binary.

## api_notes

MODULE `sim::tech` (crates/sim/src/tech/)

Types
- `#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)] pub struct Research { pub polities: Vec<PolityResearch>, pub intel: Vec<Vec<f32>> }` with `intel[viewer][target]`.
- `pub struct PolityResearch` (Clone, Debug, Default, PartialEq, serde) with fields:
  - `known: Vec<bool>`, `progress: Vec<f64>`, `target: Option<u16>`, `auto: bool`, `budget_share: f64`
  - `points_last_month: f64`, `spent_last_month: Money`, `effects: TechEffects`
  - `spillover: Vec<f64>` (discount share per technology), `carry: f64` (overflow waiting for a target)
  - `spent_total: Money`, `points_total: f64`, `completed: u32`
  - methods `new(data)`, `prerequisites_met(data, tech)`, `cost(data, tech)` (after discount), `remaining(data, tech)`
- `#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)] pub struct TechEffects { yield_factor, freighter_speed, fleet_speed, ship_speed, weapon_damage, weapon_range, shields, armor, hull, ground_attack, ground_defense, research }`, all f64 multipliers. `Default` is all 1.0. `TechEffects::of(data, known)` sums the additive bonuses.
- `pub struct TierEstimate { pub lo: u8, pub hi: u8 }` with `exact`, `is_exact`, `contains`, `width` and `Display` ("3" or "2–5").

`impl Research`
- `effects(&self, polity: PolityId) -> TechEffects`: neutral if the polity has no entry.
- `known(&self, polity) -> &[bool]`: empty slice if no entry. Also `knows(polity, tech: u16) -> bool` and `known_count(polity) -> usize`.
- `tier(&self, data, polity, branch: TechBranch) -> u8`: highest tier known in the branch, 0 = none.
- `available(&self, data, polity) -> Vec<u16>`: unknown technologies whose prerequisites are all known, ascending.
- `cost(&self, data, polity, tech) -> f64`: cost after spillover discount.
- `get(&self, polity) -> Option<&PolityResearch>`: for UI (target, progress, budget, last month's figures).
- `intel(&self, viewer, target) -> f32`: 0..1, 1 for oneself, 0 if there is no entry.
- `add_intel(&mut self, viewer, target, amount: f32)`: adds, capped at 1; ignores self, NaN and non-positive amounts.
- `estimate(&self, data, viewer, target, seed: u64, year: u64) -> [TierEstimate; 5]`: in `TechBranch::ALL` order.
- `grant(&mut self, data, polity, tech) -> bool`: grants a technology and all its prerequisites, recomputes effects.
- `inherit(&mut self, data, parent, child)`, `sync(&mut self, data, polities: usize)`, `is_consistent(data, polities)`, `find_anomaly(data, polities)`.

Free functions
- `setup_research(data: &GameData, polities: &Polities) -> Research`
- `research_month(research, economy, galaxy, polities, finance, logistics, diplomacy, chronicle, data, player: Option<PolityId>, tick)`
- `research_points(balance, credits, population, bonus) -> f64`
- `contact_weights(galaxy, polities, diplomacy, balance) -> Vec<Vec<f64>>`, `spillover_discount(balance, weight_sum) -> f64`
- `choose_target(data, entry: &PolityResearch, at_war: bool) -> Option<u16>`
- `set_target(research, data, polities, diplomacy, polity, tech: Option<u16>) -> Result<(), String>`, `set_budget(research, data, polities, polity, share: f64) -> Result<(), String>`
- `ownership_ties`, `intel_from_ties`, `update_intel`, `estimate_tier(truth, top, intel, spread, roll)`, `tier_of`, `top_tier`, `available_of`
- `pub struct TechPlugin`, registered in `SimPlugins` after `FinancePlugin`.

COMMANDS (inserted directly after `ClaimPlanet`, match arms likewise)
- `SimCommand::SetResearchTarget { tech: Option<u16> }`
  - `Some(t)` sets a manual target and turns `auto` off.
  - Rejections: "Technologie unbekannt", "<Name> ist bereits erforscht", "Voraussetzung fehlt: <Name>", "Gemeinschaft besteht nicht mehr".
  - `None` turns `auto` back on and chooses immediately.
- `SimCommand::SetResearchBudget { share: f64 }`: accepted in 0..=`max_budget_share` (0.25), otherwise "Forschungsanteil muss zwischen 0 und 25 % liegen".
- `CommandContext` gained `pub research: ResMut<Research>`, placed after `finance`.

SEMANTICS OTHER SUBSYSTEMS SHOULD KNOW
- Spending: budget = `budget_share` x `finance.polities[p].revenue`, capped by what the treasury holds above the state reserve (`economy::state_reserve` = max(population x state_reserve_per_million, treasury_targets[p])). No target means no spending.
- Automatic mode re-evaluates its target every month; progress is kept on a switch.
- A manual target stays until it completes. After that the player's community pauses: no spending, and the overflow waits in `carry`. An AI-led community falls back to automatic at the next step.
- Effects take hold from the day after completion.
- Chronicle: entry "<Gemeinschaft> erforscht <Technologie> (<Zweig>, Stufe n)." with `ChronicleKind::Tech`. With `SimConfig.player == None` every completion is written; with a human player only the player's own completions are written.
- `estimate`: window width = ceil((1 - intel) x 2 x estimate_spread), i.e. 4/3/2/1/0 tiers for intel below 0.25 / 0.5 / 0.75 / 1.0 / exactly 1.0. The window always contains the truth. At the edges of 0..6 it is shifted, not clipped. Its position comes from `Rng::stream(seed, Domain::Intel, viewer<<32|target, year<<8|branch)`. For the same seed, pair, branch and year, the range at lower intel always contains the range at higher intel.
- Intel level from ties = intel_base 0.15 + (0.8 - 0.15) x min(1, share/0.10), where share is the viewer's holders' part of the market value of all issuers seated in the target's territory (directional). The level is applied immediately; anything above it loses 2 % of the excess per month.

WIRED EFFECTS
- Yield: `Economy::tech_yield: Vec<f64>` (serde default; getter `Economy::yield_factor(polity)`) is copied from `Research` by the production system every day. It is used in `produce_and_consume`, `invest_and_depreciate` and `seed_colony`. New `economy::unit_economics_at(data, planet, wage, market, recipe, tech_yield)`; `unit_economics(..)` is unchanged and equals factor 1.0.
- FreighterSpeed: `Logistics::tech_speed: Vec<f64>` (serde default; `Logistics::freighter_speed(polity, base) -> f32`) is copied daily by the trade system and used for movement and trip planning in `move_and_trade`.
- No existing public signature changed.

EXPOSED ONLY (not wired; `research.effects(owner)` gives the multiplier)
- `fleet_speed`: multiply the map speed in fleet movement.
- `ship_speed`, `weapon_damage`, `weapon_range`, `shields`, `armor`, `hull`: battle resolver.
- `ground_attack`, `ground_defense`: ground combat.
- `research` is already applied inside research itself.
- Shipyards should pick designs with `data.best_design(class, research.known(owner))`.

HOW TO CALL `add_intel`
- After an observed battle, for each side: `research.add_intel(viewer, opponent, 0.2..0.4)`.
- Intel is one value per pair, not per branch.
- The call is safe for ids without an entry (the matrix grows; the next sync completes the entries).

FOR THE GAME UI (not done here)
- Read `Research::get(player)` for target, progress, cost and spillover; `available()` for the pick list; `estimate()` and `intel()` for other communities.
- Pass `clock.day() / DAYS_PER_YEAR` as `year` and `SimConfig.seed` as `seed`.
- For displayed economics use `unit_economics_at(.., economy.yield_factor(owner))`.
- For freighter interpolation in crates/game/src/traffic.rs use `logistics.freighter_speed(house.polity, data.balance.logistics.speed)`.
- New helpers `TechEffect::amount()` and `TechEffect::label()` give German effect names.

OTHER ADDITIONS
- economy: `pay_households(galaxy, economy, polity, budget) -> Money` (state pays households by population, never more than the treasury holds, returns the exact amount), `state_reserve(economy, data, polity, population) -> Money`, `factor_of(&[f64], polity) -> f64`.
- harness: `Headless::research()`, `PolitySummary { techs, tiers }`; `check_invariants` now also runs `Research::find_anomaly`.
- data: `BranchWeights`; `ResearchBalance` gained `max_budget_share`, `spillover_max`, `spillover_half_hops`, `auto_peace`, `auto_war`, `intel_base`, `intel_ties_max`, `intel_ties_share`, `intel_decay`, `estimate_spread`; `PolityDef { start_techs, start_tech_ids }`.
- headless: `research --seed N --years Y [--every K]` and `research --seeds A..B --years Y [--every K] [--jobs J]`.

## files_changed

- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/tech/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/tech/step.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/tech/intel.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/tech/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/tests/tech.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/data/tech_loader.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/data/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/data/ships.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/data/defs.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/lib.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/state.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/harness.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/economy/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/economy/capital.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/economy/production.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/economy/setup.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/economy/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/logistics/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/logistics/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/sim/src/finance/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/headless/src/research.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/crates/headless/src/main.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/assets/data/techs.ron
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/assets/data/balance.ron
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech/assets/data/polities.ron

## checks

Required commands, run in the worktree with output redirected to log files and exit codes checked:
- `cargo build -p sim`: exit 0.
- `cargo test -p sim`: exit 0. 62 unit tests, 6 in tests/economy.rs, 9 in tests/finance.rs, 21 in tests/tech.rs, 0 failed. Rerun after the last data edit.
- `cargo clippy -p sim -p headless --all-targets -- -D warnings`: exit 0.
- `cargo build --release -p headless`: exit 0.
- `target/release/headless sweep --seeds 0..8 --years 60`: exit 0, stderr empty, no invariant violations. Means: Versorgung 1.00, Auslastung 0.70, Stellen besetzt 0.71, Preisniveau 0.60, Frachter 569.
- `target/release/headless hash --seed 3 --days 3600`: two runs byte-identical, and identical with `--exact`. Last line: `Tag 3600 2ab40573f0a37e11`.
- `cargo fmt -p sim -p headless --check`: exit 0.

Order of the last steps: clippy, both builds and the 60-year sweep ran before two final comment-only edits in techs.ron and balance.ron. The test suite and the hash run were repeated after those edits and the hash output was unchanged.

Additional verification:
- `headless research --seeds 0..24 --years 150`: exit 0, no violations in 24 runs; the yearly check now includes research consistency.
- `headless sweep --seeds 0..8 --years 100`: exit 0, no violations.
- Research disabled (budget 0, no start techs): sweep CSV identical to the binary built from d883061 in every column except the hash.

One existing test failed along the way: `takeover_expropriation_and_bonds_work_as_commands` in tests/finance.rs. Cause: 300000 credits injected into the treasury count as "revenue", and research spent 10 % of the spike although the test binds the money as reserve. I did not touch the test; research now pays only from what lies above the state reserve, and the test passes.

Not built, as instructed: crates/game. A grep of crates/game shows it only constructs `SimCommand` values and reads fields of the types I extended. It has no exhaustive match on `SimCommand` and no struct literals of `Economy`, `Logistics`, `PolityDef`, `PolitySummary` or `SimState`, and no existing public signature changed.

Test coverage against the list in the task (crates/sim/tests/tech.rs plus unit tests):
1. Tempo: `more_money_and_more_people_mean_more_points`, `richer_communities_know_at_least_as_much_after_thirty_years` (seeds 1-3; also guards first completion in 2.5-7 years).
2. Money: `research_pay_moves_money_from_the_treasury_to_households` (treasury drop equals household gain, empty treasury, reserve, ordered war chest), `the_money_sum_holds_while_everyone_researches`.
3. Effects: `a_yield_technology_raises_output_by_exactly_its_factor`, `freighters_of_an_advanced_community_travel_faster`, `a_new_design_unlocks_once_its_technology_is_researched`.
4. Tree: `research_commands_enforce_the_tree`, `progress_is_kept_when_the_target_changes`, `the_automatic_rule_continues_and_a_manual_target_ends_in_a_pause`, `a_scripted_target_for_an_ai_community_holds_until_done`, `the_research_budget_is_validated_and_sets_the_spending`.
5. Estimates: `estimates_bracket_the_truth_and_sharpen_with_insight`, `insight_follows_ownership_and_events_and_fades`, plus unit tests on `estimate_tier`.
6. Spillover: `knowledge_of_neighbours_makes_a_technology_cheaper_the_nearer_the_more`.
7. Secession: `a_seceding_region_takes_the_knowledge_of_its_old_realm_along`.
8. Determinism: `the_same_seed_gives_the_same_research`, `skipping_idle_ticks_changes_no_research`, `a_save_in_the_middle_of_a_project_continues_identically`, `research_commands_replay_from_the_log`.
Extra: `a_human_player_reads_only_their_own_breakthroughs`, and loader validation tests in data/tech_loader.rs.

## commit

c64ee9cde0974b93fdda51f25240fa45c90ba0b1 on branch wip/m4-tech in /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech ("Meilenstein 4: Technologie und Forschung im Simulationskern"). Not pushed, no pull request.

## deviations

Decisions that go beyond or differ from the task text:

1. Files outside my list. Load-time validation needed a hook, so I added `crates/sim/src/data/tech_loader.rs` and two lines in `crates/sim/src/data/mod.rs` (`mod tech_loader;` and the call after `resolve_ships`). It checks for rings in prerequisites, positive effects, the tier bound, starting technologies and all research balance values. ship_loader.rs is untouched.

2. Research respects the state reserve. Spending is capped by the treasury above `state_reserve` (population base or the ordered `treasury_targets`), not by the whole treasury. Without this an existing finance test failed (see checks). Consequence: an ordered war chest that is not yet full suspends research.

3. Chronicle visibility. Completions of other communities are not written to the chronicle when a human plays, because the game's chronicle panel shows every entry unfiltered and would reveal the exact tree. In observer mode all completions are written.

4. techs.ron costs changed from 3000 x 2^(tier-1) to 3000 / 6000 / 13000 / 30000 / 70000 / 400000. The research rate grows roughly tenfold over a game, so with the old ladder every community finished the whole tree by about year 120-135.

5. Balance values: efficiency 0.65 (was 1.0), max_budget_share 0.25, spillover_max 0.35 with spillover_half_hops 3, intel_base 0.15, intel_ties_max 0.8, intel_ties_share 0.10, intel_decay 0.02, estimate_spread 2. Auto weights: peace 1.8 / 1.3 / 0.8 / 0.8 / 0.5, war 0.6 / 0.9 / 1.8 / 1.5 / 1.0.

6. Yield also enters the investment calculation (`unit_economics_at`), not only daily production. Otherwise investors would scrap works that are profitable with the technology.

7. Effects reach economy and logistics through state fields (`Economy::tech_yield`, `Logistics::tech_speed`) that the plugins copy from `Research` each day. This keeps every existing signature and avoids touching `monthly_books` in finance/plugin.rs, where only the secession hook was added.

8. `settle_public_accounts` now calls the extracted helpers `pay_households` and `state_reserve`. The arithmetic is identical, confirmed by the bit-identical research-off sweep.

9. Starting technologies (the optional point) are implemented: one tier-1 technology per template in polities.ron. A community is matched to its template by name, because `Polity` stores no template key and polity/mod.rs is not my file.

10. Target handling:
- When the player's manual target completes, research pauses (no spending) until a new command arrives; the overflow waits in `carry`.
- An AI-led community never idles. A scripted manual target for it is honoured until completion, then automatic resumes.
- Automatic mode re-evaluates its target monthly, so it follows a war or new spillover.

11. `grant()` also grants prerequisites. `inherit()` copies progress and the budget share as well as known technologies.

12. Estimate windows are shifted at the tree edges instead of clipped, so the width never reveals the truth; a window can therefore read 0-4 for a true tier 0.

13. "In contact" for spillover means both communities alive with territory and not at war with each other. Distance is the smallest lane-hop count between any two systems where they own planets.

14. Intel from ties is directional: only the viewer's holdings in the target's issuers count.

15. No new `Domain` variant: `Domain::Intel` already existed and is used for the estimate stream. Research itself uses no randomness.

16. README.md is not updated; it is outside my file list.

## open_issues

- Game crate (M4e): there is no research UI yet. It should prompt when the player's manual target completes, because research then pauses.
- Game crate: crates/game/src/ui/economy.rs still calls `unit_economics` without the yield factor, and crates/game/src/traffic.rs interpolates freighters with the base speed. The replacements are `unit_economics_at(.., economy.yield_factor(owner))` and `logistics.freighter_speed(house.polity, base)`.
- Nobody calls `Research::add_intel` yet. The battle and fleet code should call it after observed battles.
- FleetSpeed, ShipSpeed, WeaponDamage, WeaponRange, Shields, Armor, Hull, GroundAttack and GroundDefense are aggregated and exposed but not applied; fleet, battle and ground code must multiply by `research.effects(owner)`.
- Shipyards must pick designs with `best_design(class, research.known(owner))`.
- `PolityFinance.revenue` is a noisy base for the budget: it is the smoothed positive monthly change of the treasury, about 15-35 % of gross tax income, and it jumps with one-off inflows. Bond proceeds raise the research budget for a few months unless a treasury target binds them. A gross-revenue account in finance would be cleaner; finance was outside my files.
- The AI's war-chest rule (`SetTreasuryTarget` = 24 x revenue when at war) now suspends research until the chest is full. That fits "war slows science" but may be too harsh once fleets cost money; tune with the military AI.
- AI communities research almost in lockstep: same weights, similar economies and spillover catch-up. Leads are typically 1-3 technologies (15-20 known at year 90, 22-28 at year 150) and the starting differences vanish after about 25 years. Per-community dispositions and varying budgets are left for milestone 6.
- The budget lever is strong: 25 % instead of 10 % gives 2.5 times the points, and the state would otherwise hand most of its surplus to households anyway. A player at maximum budget can probably finish the tree before year 150; the 150-year target was only calibrated for the default share.
- Research cadence is uneven: expensive economy technologies leave gaps of 10-12 years (seed 1: years 12-22 and 32-44) because state revenue is flat for the first 40-50 years.
- Yield raises state revenue through the existing money rule (more output, lower prices, more money created for the state), which in turn raises research budgets. The loop is bounded by the cost ladder and looked healthy in 150-year runs, but it is worth watching when the economy is rebalanced.
- Merge points with other branches: crates/sim/src/data/mod.rs (2 lines), the `SimPlugins` list in lib.rs, `CommandContext` and `SimCommand` in command.rs, the `Economy` literal in economy/setup.rs, the `Logistics` destructuring in logistics/mod.rs, the body of `settle_public_accounts` in economy/capital.rs, and `PolitySummary` and `check_invariants` in harness.rs.
- State hashes and long-run economy figures change with this branch (population about +4 % at year 100, different price levels). Tests on other branches that pin hashes or tight economic thresholds may need a look after the merge.
- README.md does not describe research or the `headless research` subcommand yet.
- Old save games without a `research` entry load with an empty `Research`. The first monthly step fills it with blank entries, without starting technologies. SAVE_FORMAT was left at 1.

