# M5 Kern: Nachfolge auf neuem Kern

## summary

The succession work (wip/m6-nachfolge 816d69c) is merged onto the milestone-4 core as a real merge commit on wip/m5-kern, the seams to milestone 4 are closed, and all standard checks pass on the pushed final commit d0ab5bd. AI-only play is unchanged against the main branch: both sweeps (peaceful and grenzkrieg, seeds 0..8, 60 years) match a main-branch binary in all 41 common CSV columns; only the hash column and the two new ending columns differ.

Of the four review findings, 2, 3 and 4 were real and are fixed; finding 1 (old saves) was left alone as instructed. I also fixed a third case of the same battle-loss problem that the findings did not name: sides of the ended community that had already left a running battle were relabelled to the heir with their losses.

Nothing in this report comes from stored memory except the branch context in the handover note (project_konfluenz_handover.md), which matched what I found. I did not read M6_Synthese.md; the implementation report and the findings file were enough.

## done

- Merge commit 0cc09c2 (parents 10d4a50 and 816d69c), German message. Conflicts resolved keeping both sides in polities.ron (main's research_share values plus culture), headless main.rs (all main CSV columns, then ending and ending_year last), command.rs, data/mod.rs, diplomacy/mod.rs (truces plus heads/standing/culture), finance/plugin.rs and harness.rs. The succession branch added no SimCommand variants, so the enum order is main's.
- Founding (spawn) now grows wage_bill_running, wage_tax_running, budgets and staff, and sizes Fleets.tally, Battles.fleets_lost and Battles.won; these lists are also in the equal-length invariant.
- End of a community: Economy::staff, the running wage bill and wage tax go to the heir; its Budget is reset (no grants, not opened); its truces are dropped (new diplomacy::drop_truces_of); AiState loses its mode entry, its plan is reset (only the offensives counter is kept as history) and its fleets leave AiState::reserved.
- succession::check now also rejects, for an ended community: staff money, an open budget grant, an AI mode or plan with operations or station, a truce; and a reserved fleet that no longer exists.
- Finding 2 fixed: DistributeShares rejects a receiver that is an ended or unknown state or an unknown planet ("Empfänger besteht nicht (mehr)").
- Findings 3 and 4 fixed in crates/sim/src/fleet/succession.rs and fleet/battles.rs (battle/** untouched): before fleets pass or are dissolved, destroyed ships are written off and counted for the ended community, and the lost value of every side of the ended community moves into debris. A dissolved side is recorded as departed with the new cause Departure::Dissolved; conclude() skips such sides for war weariness, price shock and fleets_lost.
- Tests: the 14 succession tests of the branch pass on the new core (helpers now rest the military AI; one test moved off a month start because the share offer is otherwise gone within the command's tick). Three new whole-game tests in crates/sim/tests/succession.rs: the seam test with active AI (fleet, yard order, open budget, staff money, truce, war, AI plan; Joined and Forced; money drift 0, invariants, nothing left, save/load keeps the hash, two further months), the share-receiver test, and the battle-loss test for both paths.
- README: two new sections, on ownership change/succession/wards and on the two endings.

## not_done

- SAVE_FORMAT stays 5 as instructed, although the state layout changed (see open issues).
- M6_Synthese.md was not read.
- crates/game was not built or changed, as instructed.
- The Departure::Dissolved skip in conclude() has no isolated mutation probe: the one mutation run I did (removing the truce drop, the book closing and the Dissolved skip together) made the two relevant new tests fail, but the battle test stopped at an earlier assertion.
- The reviewer's exact scenario for finding 3 (carriers destroyed with fighters in the air) is not staged; the test simulates the late write-off by setting lost_value on the dissolved side by hand.
- The scripted-war regression sweep from the old report (nine --war flags) was not rerun; the grenzkrieg preset, which has 4.8 secessions per run on average, served instead.

## api_notes

All in crates/sim, worktree /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/m5.

sim::polity::succession (types re-exported as sim::polity::{Succession, SuccessionParams, Founding, TransferTerms, Transfer, Absorbed}):
- Succession<'a>: struct of borrows with pub fields data, config, galaxy, polities, economy, finance, logistics, fleets, battles, research, diplomacy, endgame, chronicle, and NEW ai: &mut AiState.
- Getting one: from a system take `mut params: SuccessionParams` and call `params.borrow()` (it now also holds ResMut<AiState>, so that system must not take AiState or any of the other state resources again); from command execution `ctx.succession()` on command::CommandContext; from a test `sim.succession(|s| ...)` on harness::Headless; from a snapshot `Succession::of_state(&mut SimState, &GameData)`.
- transfer_planet(planet: PlanetId, to: PolityId, terms: TransferTerms, tick: u64) -> Result<Transfer { from, new_capital: Option<PlanetId>, eliminated: bool }, String>. TransferTerms { state_shares_to: HolderId, state_share: f64, cohesion: f64, stage: Stage, shock: f64, elite_flight: f64, path: Path }; TransferTerms::secession(planet, &CohesionBalance, path) gives the secession terms. check_transfer(planet, to: Option<PolityId>, &TransferTerms) -> Result<PolityId, String> is the pure pre-check.
- What ends a community: (a) absorb(from: PolityId, into: PolityId, path: Path, tick) -> Result<Absorbed { planets, treasury, fleets }, String>, Path::Own rejected; (b) transfer_planet of its last planet, which ends it in the same call (Transfer.eliminated == true, `to` is the heir on terms.path). Both run the private conclude().
- Also: spawn(parent, Founding, tick) -> Result<PolityId, String>; subordinate(ward, lord, voluntary: bool, tick) -> Result<(), String>; succession::check(&World) -> Result<(), String> (first line of harness::check_invariants).
- Taking a planet by force: transfer_planet(planet, victor, TransferTerms { state_shares_to: HolderId::State(victor), state_share, cohesion, stage: Stage::Restless, shock, elite_flight, path: Path::Forced }, tick), then read new_capital and eliminated.
- sim::endgame::Path { Own, Joined, Forced }. On Joined, fleets and build orders pass to the heir; on Forced they are dissolved.

Merge guard still applies: Succession, Economy, Diplomacy, AiState, Fleets, Battles, BuildOrder, Fleet and the other structs listed in the old report are destructured without `..` in polity/succession.rs and fleet/succession.rs. Any new per-community or per-planet state (for example siege state) stops the build there until its fate at spawn, transfer and end is decided, and should be added to check().

New or changed helpers: diplomacy::drop_truces_of(&mut Diplomacy, PolityId); fleet::succession::pass_fleets(fleets, battles, polities, data, from, into, tick) (two new parameters, crate-internal); fleet::Departure::Dissolved.

## game_api_changes

Found by grep over crates/game/src; the game was not built.

Likely compile breaks: none found. The game does not match on Departure, does not build Diplomacy, PolityDef, RunSummary, Balance or EconomyBalance by literal, and does not use CommandContext or apply_secession.

Changed or added public items:
- fleet::Departure has a new last variant Dissolved. A Departed entry with that cause, and a SideReport with left_early set and held_field false, now also occurs for a side whose community ended. battle_view/campaign.rs reads left_early, held_field and has_departed and should treat such a side as gone, not beaten.
- A side in a running battle can change its owner (relabelled to the heir) and have its lost_value reset to 0 in the middle of the battle; Battles::departed can gain entries outside the morning muster.
- command::CommandContext has new fields config, endgame, ai; logistics is now ResMut.
- polity::Succession and SuccessionParams have the new field ai.
- Diplomacy has new fields standing and culture next to truces; at_war, is_at_war and enemies_of now resolve heads (a ward shares its head's wars). declare_war and make_peace reject wards.
- SimState has the new field endgame; new resource sim::endgame::Endgame (outcome, player, path_of, is_seat) and functions census/classify for an end screen.
- PolityDef.culture is required in polities.ron; Balance.endgame and EconomyBalance.reserve_floor are new and required in balance.ron.
- harness::RunSummary.outcome is new; finance::apply_secession now takes (&Secession, &mut Succession, tick).
- DistributeShares is rejected for a receiver that does not exist.

Behaviour the game must cope with:
- SimConfig.player can become None in the middle of a game (the player's community ended or was subordinated; Endgame.player is then PlayerFate::Out).
- After an outcome the simulation stands still (clock stops, commands ignored); show the end from Res<Endgame>.outcome. ChronicleKind::End, already matched in ui/panels.rs, is now actually written once per game.
- Polity.capital is None and alive is false for an ended community; its treasury, staff money and budget are empty; new chronicle texts appear (capital moved, community ended, subordination, build order lapsed).
- Scheduling: the monthly cohesion system and apply_commands now also take ResMut<AiState>, ResMut<SimConfig> and ResMut<Endgame>; any game system that runs inside the sim schedule and touches those resources needs an order.

## checks

All on the final commit d0ab5bd with a clean worktree, output to log files under /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/m5, judged by exit code. CARGO_TARGET_DIR was the integ target directory.

- cargo build -p sim -p headless: exit 0, no warnings.
- cargo test -p sim -p headless (whole suite, one run): exit 0. 359 passed, 0 failed, 1 ignored (an existing tech test): 3, 180 (sim lib), 8, 45, 7, 6, 10, 48 (military), 17 (succession), 35 + 1 ignored.
- cargo clippy -p sim -p headless --all-targets -- -D warnings: exit 0. It ran right after the commit; no source changed afterwards.
- cargo fmt -p sim -p headless -- --check: exit 0.
- cargo build --release -p headless: exit 0.
- headless sweep --seeds 0..8 --years 60: exit 0, stderr empty. "Enden: Keine Partie endete in der Laufzeit."
- headless sweep --seeds 0..8 --years 60 --preset grenzkrieg: exit 0, stderr empty. 4.8 secessions per run on average, 11 decided battles.
- headless hash --seed 3 --days 3600, twice, and once with --exact: all exit 0, outputs byte-identical. Day 3600: 93b542607009e648.

Extra:
- Comparison with the release binary in the main checkout (built 26 Nov 21:13, two minutes before commit 10d4a50, so very probably from that source; run with the main checkout's data directory): both sweeps identical in all 41 common CSV columns (everything except hash, ending, ending_year); 48 and 82 data rows.
- Mutation probe: with the truce drop, the book closing and the Dissolved skip removed together, the seam test and the battle-loss test fail as they should; sources restored afterwards.

## commit

d0ab5bd955353feb5efb3d323c47bff39ecc4677 on wip/m5-kern, pushed (origin/wip/m5-kern is the same commit), worktree clean. Two commits on top of 10d4a50: the merge 0cc09c2 and d0ab5bd (seam tests, closing the books of all battle sides, README). Both messages are German and end with the two required trailer lines.

## open_issues

- SAVE_FORMAT is still 5 although the state layout changed (Diplomacy.standing and culture, the Endgame resource, per-community lists sized from the start). A format-5 save written by the main branch still loads and then fails succession::check on list lengths, and can panic at the first change of owner (finding 1). The format number has to be raised by whoever is allowed to.
- In AI-only play still nothing but secession calls the succession operations, so no community ends and no ward arises; the siege and diplomacy steps have to call transfer_planet, absorb and subordinate.
- The heir's budget shows an inherited treasury as 'other movements' in its next monthly close, not as revenue. Staff money of the ended community goes to the heir's staff account on both paths, also on the forced one where the fleets are dissolved. Both are my decisions; change them in conclude() in crates/sim/src/polity/succession.rs if another rule is wanted.
- The ended community's budget history is cleared with its budget, so RunSummary and the sweep CSV show zeros for its last twelve months.
- On the peaceful path a side keeps its lost-ship counts (Combatant.lost) under the heir's name in the battle report while its lost_value restarts at 0; war weariness, price shock, ships_lost and fleets_lost are attributed correctly, only the report line mixes both owners.
- A side of the ended community that left a battle earlier without an opponent (Departure::NoOpponent) stays in the report as holding the field under the heir's name on the peaceful path, and can count as a won battle for the heir if the battle ends decided.
- subordinate() leaves truces and the AI plan of the ward untouched; a ward cannot declare war, but the military AI still plans for it. Not examined further.
- AI plans of other communities may still hold operations aimed at systems of an ended community; I relied on the planner's existing handling of targets that are no longer hostile and did not test it beyond the two months in the seam test.
- Carried over from the old report and unchanged: several fleet rules are not bloc-aware (escorts, 'defended' in blockades, resupply only in own systems); claiming a free planet and abandoning a dead colony change the owner outside succession.rs; headless print_aftermath may panic for an unknown community index with --war.
- Worth remembering for later sessions (learned by running it here): the release headless binary in the main checkout serves as a baseline for main-branch behaviour, and the grenzkrieg preset is a quick regression sweep that contains secessions.

