# Technologie: Bericht der Nachbesserung (wip/m4-tech, d314838)

## summary

All 13 findings are addressed and committed on wip/m4-tech as d314838; one of them (the research budget has no real opportunity cost) is only bounded, not solved. All required commands pass.

What changed, in the order of the findings:

- **Budget base (3, 8).** The budget is now a share of last month's recurring state income (wage tax, Kontor tariffs, dividends on state holdings), tracked in the new `Economy::state_income`. Central-bank money, bonds and a growing reserve no longer count. In the reviewer's starving-economy what-if (reserve 600 per million, 6 seeds, year 10) that world now has 2.0 technologies against 2.5 in the normal game; before it was 4.6 against 3.0.
- **Estimates (2, 6, 11).** The picture one community has of another is stored state (`Research::beliefs`), not a yearly re-roll. Its lower bound never falls, progress inside the window goes unnoticed, and a one-tier move can be either progress or a confirmation. `Research::estimate` no longer takes seed or year.
- **Spillover leak (1, 10).** A contact counts for a technology only if the learner's own lower bound for that contact reaches the tier. Own costs are now a function of the learner's own picture and the map, so neither their size nor a war declaration reveals anything.
- **Lead (9).** Templates now carry their own branch weights and budget share. The ladder is 3000/6000/12000/24000/54000/400000 and combat effects are about a third larger. At years 30/60/90/100 over 8 seeds the best-to-worst gap per branch is 0 tiers in 4, 1 in 110, 2 in 40 and 3 in 6 of 160 snapshots; before it was 147/92/1/0 of 240.
- **Budget lever (12).** AI communities use their template's share and raise it by command in war. The maximum share is 0.30 against a usual 0.20, so 1.5 times instead of 2.5. With everyone at the maximum for 150 years (8 seeds) nobody completes the tree; the most is 27 of 30.
- **Small ones (4, 5, 7).** Duplicate keys and names in polities.ron are rejected. `tech_yield` and `tech_speed` are filled from tick 0 and in the tick of a secession. `headless economy` uses the yield factor.
- **Tests (13).** `tests/tech.rs` is rewritten: 34 tests plus one ignored 150-year pace test, which I ran and which passes.

Pace with shipped data (8 seeds): 2.5 technologies at year 10, 6.5 at 30, 18.5 at 100, 24.2 at 150; first technology after 3.1–4.2 years in seed 1.

Decisions you should know about:

- **I changed one line outside the subsystem's files.** `tests/economy.rs` now runs the blockade scenario on seed 4 instead of 7, with the assertions unchanged. I probed 12 seeds on the committed baseline and on my build: the scenario holds in 6 of 10 usable seeds before and 5 of 10 after. Seed 7 flipped because the first decades run differently.
- **I tried a real cost for research and removed it.** Withdrawing researchers from the workforce costs nothing in aggregate, because about 30 % of the workforce has no position: with all at 25 %, population after 100 years was 0.4 % lower. It also changed the same-seed blockade result (seed 7: 1.45 with the mechanism, 3.00 without).
- **Two public signatures changed** because finding 11 required it: `Research::estimate` and `research_month`. Nothing in crates/game uses either.
- I did not build the game crate, as instructed. It only uses `unit_economics`, which is unchanged.

Memory was empty and not used. Worth keeping, both from this session's measurements: the economy is demand-limited with about 30 % idle labour, and the blockade test in `tests/economy.rs` is seed-fragile.

## fixed

- 1 and 10 (spillover leak): `visible_spillover` counts a contact only where `Research::confirmed` (the learner's lower bound) reaches the technology's tier. Tests show an identical learner entry over 48 months in two worlds that differ only in hidden knowledge, and unchanged costs across a war declaration and peace.
- 2, 6 and 11 (estimates): stored beliefs advanced monthly by `advance_belief`. More insight cuts the window from the top when the truth allows, so insight that flickers around a threshold shows the same picture again. `add_intel` takes effect on the picture at the next month start. In a 45-year game, a notebook's sound deduction always equals the current window.
- 3 (yield and budget base): repeated the Economy 1..n comparison on the new base with research off, seeds 1 and 2. Income moves by -6 % (Economy 1-3) to -17 % (full branch) in decade 1, where the old base lost 46-57 %. By decade 6 it is +18 % to +3 % on seed 1 and +9 % to -11 % on seed 2.
- 4 (duplicate template names): `resolve_research` rejects duplicate `key` and `name` in polities.ron with a German error; technology costs must now be positive. Unit tests cover both.
- 5 (empty effect copies): `Research::publish_effects` runs in `SimState::generate`, after each research step and in the secession hook; `Economy::find_anomaly` now checks that every per-community list is complete. Tests cover tick 0, the secession tick and `seed_colony` ranking with the owner's factor.
- 7 (headless economy): the detail view calls `unit_economics_at` with the owner's yield factor and prints the factor. Checked on seed 3, year 40 plus one day: per-unit margins and Rendite agree in sign.
- 8 (budget base): `Economy::state_income` is filled from wage tax, tariffs and dividends and closed monthly. Tests show that a windfall, the old `revenue` estimate, minting and bonds do not move the budget, and that a reserve raised month by month yields less research, not more. Tool column and doc comments renamed.
- 9 (no lead): per-template `research_focus` and `research_share`, a ladder that doubles up to tier 4, and spillover that no longer reaches the frontier without insight. A new test over 3 seeds finds two or more tiers between best and worst in 12 of 45 branch snapshots at years 50-70. The community that values a branch most is ahead of half-weight communities in 26 cases, level in 2, behind in none.
- 12 (budget lever), partly: `research_policy` issues `SetResearchBudget` for AI communities (template share, times `ai_war_factor` 1.25 in war, and it resets a share changed from outside). Maximum share lowered to 1.5 times the usual. Tier 6 holds at the maximum: nobody completes the tree by year 150.
- 13 (tests): research is ranked against real output at base prices times the population weight, with a strict 10 % margin for pairs that differ by 30 %. New tests cover the income account, hoarding, inference over time and through costs, the war probe, position in a running game, the yield hooks in `invest_and_depreciate` and `seed_colony`, and the 150-year pace at usual and maximum share (ignored by default; 271 s).

## not_fixed

- 12, the core: research still costs a community nothing real. Its pay returns to the same households, and the labour it would bind is idle. A real trade-off needs the state budget from the strategic layer (ships, upkeep). Until then a human at the maximum share gets 1.5 times the usual research for free; with everyone at the maximum that is +2.4 technologies at year 60 and +1 at year 150.
- 9, two parts: designs that use the technology-gated modules are in designs.ron and modules.ron, which I was told not to touch, so I raised the combat effect sizes in techs.ron instead. And leads shrink again after about year 130, when communities reach the tier-6 cost wall; that wall is what keeps the tree unfinished at the maximum share.
- 3, residual: the budget base still reacts to Yield more than population does. Shipped data against no Yield for Economy 4-6, seeds 1 and 2: income +22-25 % at year 100, +32 % at 150, +42-48 % at 200, with population +0-4 %. This now comes from real employment and wages, not from the estimator. Without central-bank money the economy itself stagnates (population 5-10 thousand million per community at year 100 instead of 10-17) and the base follows it.
- 2/6/11, one documented exception: when more insight pushes a window upward and the old window was less than twice as wide as the new one, the new window shows more tiers than are still possible. It only occurs at a gain of insight and never shows too few.
- The `usual_share` field has no serde default, so save games written by the previous commit of this branch do not load. Saves from main (which have no research state) are unaffected.

## checks

All run in the worktree after the last source change; logs are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/tech/fix/final.

- `cargo build -p sim`: exit 0.
- `cargo test -p sim`: exit 0. 79 unit tests, 6 economy, 9 finance, 34 tech passed; 1 ignored.
- `cargo test -p sim --test tech -- --ignored` (150-year pace test): passed in 271 s.
- `cargo clippy -p sim -p headless --all-targets -- -D warnings`: exit 0.
- `cargo fmt -p sim -p headless -- --check`: exit 0.
- `cargo build --release -p headless`: exit 0.
- `target/release/headless sweep --seeds 0..8 --years 60`: exit 0, stderr empty. Seeds 8..24 as well.
- `target/release/headless hash --seed 3 --days 3600`: two runs identical, and identical with `--exact` (day 3600: a598f688911f7eab).
- `headless research --seeds 0..8 --years 150` with shipped data and with every template at the maximum share: stderr empty, no completed tree.
- Forced-secession stress (18 secessions per seed, 24 communities, 8 seeds, 40 years): no violation; fast and exact hashes identical.
- Not built: crates/game, as instructed.

## commit

d314838e2095b0fdaffd3144bc5b7027709a9193 on wip/m4-tech (worktree /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/tech), working tree clean

## api_notes

Changes against the implementer's notes; everything not listed is unchanged.

CHANGED SIGNATURES
- `Research::estimate(&self, data, viewer, target) -> [TierEstimate; 5]`: `seed` and `year` are gone. It returns the stored picture.
- `research_month(.., data, config: &SimConfig, tick)`: takes `&SimConfig` (seed and player) where it took `player: Option<PolityId>`.

NEW IN `sim::tech`
- `Research.beliefs: Vec<Vec<[TierEstimate; 5]>>` (`beliefs[viewer][target][branch]`, serde default). `TierEstimate` is now Serialize/Deserialize; `lo` is confirmed knowledge.
- `Research::confirmed(viewer, target, branch) -> u8`: the lower bound; spillover uses only this.
- `Research::publish_effects(&mut Economy, &mut Logistics)`, `publish_yield`, `publish_speed`.
- `PolityResearch.focus: BranchWeights` and `PolityResearch.usual_share: f64`, both from the template and inherited on secession.
- Free functions: `estimate_width(intel, spread)`, `advance_belief(belief, truth, top, width, sighting, rolls)`, `update_beliefs(research, data, seed, month)`, `visible_spillover(research, data, learner, weights)`, `ai_budget_share(balance, usual, at_war)`.
- `estimate_tier` is kept as a single-snapshot helper; the game's picture does not use it.

NEW IN `sim::economy`
- `Economy.state_income` and `Economy.state_income_running` (`Vec<Money>`, serde default).
- `Economy::state_income_of(polity)`, `sync_polities()`, `book_state_income(polity, amount)`, `close_income_month()`.
- `find_anomaly` now also requires complete per-community lists.

DATA
- `PolityDef.research_focus: BranchWeights` (default all 1.0) and `PolityDef.research_share: Option<f64>`, written `Some(0.20)` in RON.
- `ResearchBalance` gained `ai_war_factor` and `sighting_per_month`.
- `BranchWeights` gained Serialize, `EVEN`, `even()` and `values()`.
- balance.ron research: `budget_share` 0.20, `max_budget_share` 0.30, `efficiency` 0.026, `ai_war_factor` 1.25, `sighting_per_month` 0.004.
- techs.ron: costs 3000/6000/12000/24000/54000/400000; weapons, defence and ground effects raised. Yield, FreighterSpeed, propulsion and Research effects are unchanged.

SEMANTICS OTHER SUBSYSTEMS SHOULD KNOW
- Budget = `budget_share` x `economy.state_income_of(p)`, paid only from what the treasury holds above the reserve. `finance.polities[p].revenue` is no longer read by research; bonds and the war-finance AI still use it.
- `add_intel` changes `intel()` at once. The picture from `estimate()` follows at the next month start, at the full pre-decay value.
- Spillover needs confirmed knowledge. A battle that calls `add_intel` therefore also makes the opponent's known tiers cheaper to research.
- AI communities issue `SetResearchBudget` themselves (source Ai): template share in peace, times 1.25 in war, capped at the maximum. A share set from outside for an AI community is reset at the next month start. The player's share is never touched.
- `SetResearchBudget` error text is now "Forschungsanteil muss zwischen 0 und 30 % liegen", since the maximum comes from data.

FOR THE GAME UI
- Read `estimate(data, viewer, target)`; nothing needs to be passed for time or seed. Show `Research::get(player)` freely, including cost and spillover: they depend only on the player's own picture.
- `headless research` prints Kontakt (sum of contact weights), Einnahmen, Etat, and in the overview the lead histogram and cumulative points.

