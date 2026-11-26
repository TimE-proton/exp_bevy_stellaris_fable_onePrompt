# Besitzwechsel, Nachfolge und die beiden Enden im Kern (Stand 816d69c)

## summary

Package "Nachfolge" (groundwork for M6) is implemented completely, with tests, on branch wip/m6-nachfolge. HEAD is 816d69c9e6e5f6c9e5a475596a00bbef1bcbf299 (9 commits on top of 5ac904d), pushed to origin, worktree clean. Every command of the required final state succeeds on exactly that commit (details under checks).

What exists now:
- A. /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/polity/succession.rs is the one place where a planet passes from one community to another, where a community is founded after the start and where one ends. Four operations on a struct of borrows (spawn, transfer_planet, absorb, subordinate), each Result<_, String> with German reasons, each checking first and changing afterwards, with exhaustive destructuring (no '..') of every per-community and per-planet struct. SystemParam twin, CommandContext helper and Headless helper exist. Fleet-side helpers live in the new file crates/sim/src/fleet/succession.rs; crates/sim/src/fleet/mod.rs got exactly one line.
- B. Diplomacy has standing and culture, head/is_free/wards_of/bloc/affinity; at_war, enemies_of and is_at_war resolve heads; declare_war and make_peace are open to free communities only; drop_wars_of is new. polities.ron carries a culture per template.
- C. crates/sim/src/endgame.rs: Endgame state (in sim_state! directly after diplomacy), Census, the pure total classify, the monthly judge in SimSet::Victory, standstill after the outcome (game_running, Headless::run_days/run_days_exact/issue), RunSummary.outcome, 'run' and 'sweep' output, two CSV columns at the end.
- D. finance::cohesion::apply_secession is rebuilt on spawn and transfer_planet with unchanged behaviour plus the cap free_states < max_free_states; issuers_of_region is the shared function for the issuers seated on a planet.
- E. Lever block 'endgame' (first block of balance.ron, first field of Balance, validated) and economy.reserve_floor replacing the hard-coded 0.02.
- F. succession::check(world), called by one added first line of harness::check_invariants.
- G. All eight required tests plus further ones: 14 world tests in crates/sim/tests/succession.rs and unit tests next to the code (endgame, diplomacy, fleet/succession, polity/succession, tech, data).

Evidence that AI-only play is unchanged: the sweep CSV of seeds 0..8 over 60 years is identical to the one of a binary built from 5ac904d in all 20 common columns; the same holds for a scripted-war sweep that contains 29 foundings of free states. Only the hash column differs (the hashed state has new fields) and the two new columns are empty.

Two defects found and fixed in my own work during the final review, both now covered by tests: (1) a battle side dissolved by a forced end was relabelled to the heir and would have charged the heir with the ended community's earlier losses when the battle concluded (commit d773925); (2) when the last planet of a community founded a new community, its capital was wrongly flagged as a 'seat' although nobody had joined (commit 816d69c; the new test fails against the old rule, verified).

Limits to know before building on it: in AI-only play no community ends and no ward arises yet, because nothing calls transfer_planet/absorb/subordinate besides secession (endings happen only through direct calls or physical collapse); several fleet rules are not bloc-aware because the existing fleet files were left untouched; SAVE_FORMAT must be raised by the integrator. crates/game was not built, as instructed.

Nothing in this report comes from stored memory (the one stored note is unrelated and was not used). Worth remembering, all learned by running things in this session: (a) peaceful AI sweeps contain no secession at all, even 16 seeds over 150 years, so the regression check for ownership changes is the scripted-war sweep 'sweep --seeds 0..8 --years 40 --war 5:0:1 --war 5:2:3 --war 5:4:5 --war 10:1:2 --war 10:3:4 --war 10:5:0 --war 15:0:2 --war 15:1:3 --war 15:4:2 --peace 20:0:1' (29 foundings); (b) the machine is shared and often loaded: a dev build of sim took up to 4 minutes, a full 'cargo test -p sim' 4 to 10 minutes; (c) baseline artefacts of 5ac904d (binary, data directory, both sweep CSVs) and the mutation probe script are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/nachfolge (headless_base, data_base/, base_sweep.csv, base_war.csv, mut/run.py); the scratch directory can be cleaned at any time.

## api_notes

ALL PATHS below are under /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge.

1. sim::polity::succession (crates/sim/src/polity/succession.rs; types re-exported as sim::polity::{Succession, SuccessionParams, Founding, TransferTerms, Transfer, Absorbed})

Succession<'a>: struct of borrows, all fields pub: data: &GameData, config: &mut SimConfig, galaxy, polities, economy, finance, logistics, fleets, battles, research, diplomacy, endgame, chronicle (all &mut).

How to get one:
- From a system: take the SystemParam twin and borrow it.
    fn my_system(clock: Res<SimClock>, mut params: SuccessionParams) {
        let mut succession = params.borrow();
        let _ = succession.transfer_planet(planet, to, terms, clock.tick);
    }
  SuccessionParams holds Res<GameData> and ResMut of the 12 state resources; a system that takes it must not take any of those resources a second time (read them through params.galaxy and so on). Put the system into a SimSet of the chain, ambiguity detection is an error. Live example: finance::plugin::cohesion.
- From command execution: ctx.succession() on command::CommandContext (inside command::execute). CommandContext gained config: ResMut<SimConfig> and endgame: ResMut<Endgame>; logistics is now ResMut.
- From a test or tool on a world: sim.succession(|s| s.absorb(from, into, Path::Joined, tick)) on sim::harness::Headless; signature pub fn succession<R>(&mut self, run: impl FnOnce(&mut Succession) -> R) -> R.
- On a state snapshot without a world: Succession::of_state(&mut SimState, &GameData).

Operations. All check first and change afterwards; an Err(String) carries a German reason and leaves the state untouched.
- spawn(parent: PolityId, founding: Founding { name, short, motto, color, capital }, tick) -> Result<PolityId, String>. The only place where per-community lists grow (Polities.list, six lists in Economy, Finance.polities, Fleets.arrears, Battles.ships_lost, Diplomacy.standing and culture, Logistics.tech_speed, the three Research lists). founding.capital must belong to the parent at that moment; the caller transfers it in the same move with path Own. The child inherits tax rate, culture and the parent's whole technology; it starts with treasury 0, no debt, no fleet, no trade house, free and at peace. Founding::free_state(&Polity, &Planet) builds the usual free state. spawn does not count Endgame.free_states (apply_secession does).
- check_transfer(planet, to: Option<PolityId>, &TransferTerms) -> Result<PolityId /* current owner */, String>. Pure pre-check of transfer_planet; None stands for a community that is yet to be founded.
- transfer_planet(planet, to, terms: TransferTerms, tick) -> Result<Transfer { from, new_capital: Option<PlanetId>, eliminated: bool }, String>. TransferTerms { state_shares_to: HolderId, state_share: f64, cohesion: f64, stage: Stage, shock: f64, elite_flight: f64, path: Path }; TransferTerms::secession(planet, &CohesionBalance, path) gives today's secession terms (the planet's public, 0.5, base + honeymoon, Loyal, 0.85, elite_flight 0). Effects: owner; tax_exempt = (stage == Autonomous); region bookkeeping; endgame tag (path, population and cultural distance at that moment; Own clears) and seat flag cleared; state_share of the old state's holdings in the issuers seated on the planet moves to state_shares_to and their shock is multiplied; build orders of the old owner at that yard are cancelled; trade-house homes leave a system in which the community holds no planet any more. Capital: the most populous remaining planet becomes capital (tie: lower id), trade houses follow, the region is at most Restless and taxable, elite_flight of the old capital elite's holdings in all issuers of the loser and of its cash moves to the new capital's elite. Last planet: the old owner ends in the same call (eliminated = true), 'to' is the heir on the path of the terms.
- absorb(from, into, path: Path, tick) -> Result<Absorbed { planets, treasury, fleets }, String>. Path::Own is rejected. Planets change owner and keep their region state; tags as designed (Joined retags only Own/Joined, Forced tags all); joined capital becomes a seat, a forced end clears seats; trade houses, bonds (self-owed ones cancelled), state holdings and offers, treasury, research pass on; fleets and build orders pass on a peaceful path and are dissolved on a forced one; wars of 'from' end; its wards go to head(into); player out if it was the player; alive = false, capital = None.
- subordinate(ward, lord, voluntary: bool, tick) -> Result<(), String>. Both alive and free. Wards of the ward go to the lord, the ward's wars end, its planets are tagged Joined (voluntary, Forced survives) or Forced, player out if it was the player.
- check(world: &World) -> Result<(), String>: the invariants of F; runs first in harness::check_invariants.

Typical callers of later packages:
- Taking a planet by force: transfer_planet(planet, victor, TransferTerms { state_shares_to: HolderId::State(victor), state_share, cohesion, stage: Stage::Restless, shock, elite_flight, path: Path::Forced }, tick), then read transfer.new_capital and transfer.eliminated.
- Union command: ctx.succession().absorb(from, into, Path::Joined, tick)?; absorb sets PlayerFate::Out and removes the player if 'from' was the player, a union command that wants Merged sets ctx.endgame.player = PlayerFate::Merged itself afterwards.
- Subordination: ctx.succession().subordinate(ward, lord, voluntary, tick)?.
- New community: spawn, then transfer_planet(capital, id, TransferTerms::secession(capital, &balance, Path::Own), tick); see finance::cohesion::apply_secession.

MERGE GUARD (deliberate): these structs are destructured or filled exhaustively. A field added to one of them on another branch stops the build after the merge until it is named there and its fate at a change of owner is decided. polity/succession.rs: Succession, Founding, TransferTerms, Polity, Planet, PlanetEconomy, Region, CohesionFactors, Economy, Finance, PolityFinance, Diplomacy, Logistics, TradeHouse, Research, Endgame, Bond, SimConfig. fleet/succession.rs: Fleets, Battles, Fleet, BuildOrder. finance/shares.rs (pass_holdings): Issuer. tech/mod.rs (inherit, absorb): PolityResearch. endgame.rs (tag, clear_planet): Endgame. Any NEW state that stores a PolityId or is indexed per community must be grown in spawn, handed over in the private conclude (the end of a community) and checked in check; new per-planet state belongs into transfer_planet.

2. sim::diplomacy (crates/sim/src/diplomacy/mod.rs)
- Standing { overlord: Option<PolityId>, voluntary: bool, since_day: u64 }; NEUTRAL_CULTURE = [0.5; 3]; Diplomacy { wars, standing: Vec<Standing>, culture: Vec<[f32; 3]> } (culture = openness, public spirit, readiness).
- Methods: overlord_of(p), head(p), is_free(p), wards_of(p) (ascending), bloc(p) (head first, then its wards), culture_of(p), affinity(a, b) = 1 - distance / sqrt(3), at_war(a, b) (resolves heads; false inside one bloc), enemies_of(p) (enemy heads, each followed by its wards), is_at_war(p).
- setup_diplomacy(&GameData, &Polities) -> Diplomacy (all free, cultures from polities.ron). declare_war / make_peace: same signatures as before, both sides must be alive and free, otherwise Err("<name> untersteht <head>: Über Krieg und Frieden entscheidet das Oberhaupt"). drop_wars_of(&mut Diplomacy, polity) -> Vec<PolityId> (former opponents, ascending). Wars are stored between heads only.

3. sim::endgame (crates/sim/src/endgame.rs)
- Path { Own, Joined, Forced } with label() ("eigen", "beigetreten", "genommen"); PlayerFate { Active, Merged, Out }; Ending { Metamorphosis, Extinction } with label() ("Metamorphose", "Auslöschung"); Outcome { ending, survivor: Option<PolityId>, day, joined, forced } with year().
- Endgame (resource, in sim_state!): path, absorbed_pop, gap, seat (per planet), peak_population, months_below, free_states, player, outcome. Methods: new(planets), ended(), path_of(planet), is_seat(planet), tag(planet, path, population, gap), clear_planet(planet).
- Census { population, peak, months_below, blocs: Vec<BlocCensus { head, population, joined, forced }> }; Verdict { ending, survivor, joined, forced }; is_sovereign(&BlocCensus, total, &EndgameBalance); has_collapsed(&Census, &EndgameBalance); classify(&Census, &EndgameBalance) -> Option<Verdict> (pure, total); total_population(&Galaxy, &Polities); census(&Galaxy, &Polities, &Diplomacy, &Endgame) -> Census; judge(&mut Endgame, &Galaxy, &Polities, &Diplomacy, &mut Chronicle, &EndgameBalance, tick) -> Option<Outcome> (writes once, one ChronicleKind::End entry); EndgamePlugin (judge in SimSet::Victory at month start, added as the last plugin).
- A UI can show how close the end is with census(..) and classify(..) and the end itself from Res<Endgame>.outcome.

4. Other public items
- sim::finance: apply_secession(&Secession, &mut Succession, tick) -> Option<PolityId> (signature changed); issuers_of_region(&Finance, &Galaxy, &Logistics, capitals: &[Option<PlanetId>], planet) -> iterator of (IssuerId, &Issuer) (now pub, the shared function); move_holdings(&mut Finance, issuers: &[IssuerId], from: HolderId, to: HolderId, share: f64) -> u64 (floor per issuer); pass_holdings(&mut Finance, from: HolderId, to: HolderId) -> u64 (holdings and open offers).
- sim::tech::Research::absorb(&mut self, &GameData, from, into): union of what both know, larger progress, larger carry-over, better intel, intersection of pictures. Research::inherit is unchanged in behaviour.
- sim::harness: Headless::outcome() -> Option<Outcome>; Headless::succession(..); RunSummary.outcome: Option<Outcome> (last field); Headless::issue ignores commands after the end; run_days and run_days_exact return early.
- sim::data: Balance.endgame: EndgameBalance { max_free_states: u32, sovereign_min_share: f64, collapse_share: f64, collapse_months: u32 }; EconomyBalance.reserve_floor: f32; PolityDef.culture: (f32, f32, f32) (required).
- Command execution rejects every command of a community that has ended or does not exist: "Gemeinschaft besteht nicht mehr".
- crate-internal, crates/sim/src/fleet/succession.rs: size_for_polities(fleets, battles, count); per_polity_lengths(fleets, battles); cancel_orders_at(fleets, galaxy, chronicle, data, planet, owner, tick) -> usize; pass_fleets(fleets, battles, polities, from, into) -> usize; dissolve_fleets(fleets, battles, data, from, into, tick) -> (fleets, orders); sync_hostility(battles, diplomacy) (recomputes who is hostile in running battles; a battle without enemies is called off by the calculator); reference_to_ended(fleets, battles, polities) -> Option<String>.

5. Head resolution at the existing callers (audit for B)
Through at_war / is_at_war / enemies_of these now follow heads, so a ward is treated like its head: logistics (open_for, the raiders in raid_pressure), fleet (besiegers in update_blockades, hostility in open_battles / join / conclude, retreat_target, contested yards in advance_yards), finance (risk_discount, issue_bonds, invest_holders, monthly war weariness), tech (contact_weights, war target and budget), AI war financing, and the war shock at a declaration of war (now both blocs). Not bloc-aware, because the existing fleet files were not touched: see open_issues.

## decisions

Where the design left room, or where I deviated, with reasons:

1. Battles is part of the borrow struct although the design list did not name it: running battles store community ids per side, and dissolving or handing over fleets needs them.
2. The four operations are methods on Succession, not free functions; check_transfer is public so that apply_secession can make sure a founding is never followed by a failing transfer.
3. What a child knows: the parent's whole technology (known technologies, begun work, spillover, focus, budget share, the parent's intel and pictures of others) through the existing Research::inherit. That is exactly what free states got before, so behaviour is unchanged, and a child that knows nothing could not defend itself. It inherits tax rate and culture and starts with treasury 0, no debt, no fleet, no trade house.
4. Tax exemption: tax_exempt = (terms.stage == Autonomous) instead of always clearing. Identical for every caller of today (secession passes Loyal); it keeps the existing rule 'only an autonomous region pays no tax' true if terms hand a planet over as autonomous.
5. Build orders at a transferred shipyard are cancelled at once without refund: parts and yard fee were already paid to the market and the arms firm, so money stays where it is and nothing is created or destroyed; one chronicle entry per order. Exception: on a peaceful end the orders pass to the heir together with the fleets. On a forced end all orders of the ended community are removed.
6. Found by searching for uses of the planet's owner: a trade house whose community holds no planet any more in its home system moves to the system of the capital (besides following a moved capital).
7. Elite flight: floor(share x holding) per issuer, cash truncated; 'issuers on the loser's remaining territory' are all issuers whose community is the loser after the transfer (firms, the system offices, its trade houses). Secession terms use elite_flight 0.
8. Last planet: the community ends through the same internal routine as absorb. If the last planet founds a new community (path Own), the new one inherits peacefully, nothing is tagged and no seat is recorded; the public absorb rejects Path::Own.
9. Seat rule: the former capital becomes a seat when a community joins (absorb with Joined, or its last planet accedes); existing seats survive peaceful unions; a forced end clears the seats of the ended community's planets; a single transfer clears the flag of that planet.
10. On absorb the regions keep stage, cohesion, month counters, since_day and tax exemption; owner_seen is updated so that the month end does not read the change as a new owner and reset them; factors.foreign_polity pointing at the ended community is redirected.
11. Books of the heir: treasury moved and last_treasury raised by it (not revenue of the month); the ended community's revenue estimate and state income (last and running month) are added to the heir, because the research budget is measured by them; its risk and reputation values are reset; its counter of defaults stays as history.
12. Bonds the heir owes itself are removed; a bond is a liability, not money, so money_drift stays 0.
13. Research merge beyond the design: larger carry-over, a target that is now known is dropped, effects recomputed, intel = the better of both, pictures of others = intersection, then the pictures others hold are settled. Budget and focus stay the heir's.
14. Fleets on a peaceful path are renumbered into the heir's numbering ("{n}. Flotte ({short})"). On a forced path ships of a fleet in battle count as escaped (not as losses, ships_lost untouched), their shots are removed, the battle side is relabelled to the heir so that nothing points at the ended community, and what that side had already lost goes into the system's debris at once and is zeroed, so the heir is not charged with it when the battle concludes.
15. History may name ended communities and is exempt from the invariant: chronicle, command log, queued commands, reports of finished battles, dissolved fleets, whole-game counters. To make queued commands harmless, command execution centrally rejects commands of ended or unknown communities.
16. Wards of an ended community go to head(into), not blindly to 'into', because the heir may itself be a ward (no chains); if the heir was a ward of the ended community it becomes free. absorb and transfer_planet into a ward are allowed; only subordinate requires both to be free.
17. subordinate tags only the ward's own planets; wards handed on to the lord keep their tags and their voluntary flag. The ward's wars end without a peace entry.
18. enemies_of returns enemy heads each followed by its wards, so callers that iterate enemies also see the wards. The war shock at a declaration of war hits both blocs.
19. The end entry uses the existing ChronicleKind::End, which was in the enum but never written, instead of a new variant: crates/game matches the enum exhaustively and must not be built or edited here. The end of a single community is written as Diplomacy (peaceful), War (forced) or Cohesion (capital moved).
20. classify: a sovereign needs population > 0 (an empty galaxy has none); collapse needs the month counter AND the current population below the threshold; in rule 1 the two sums are galaxy-wide; a tie is Extinction.
21. Standstill: game_running is false once an outcome exists, so no system runs and the clock stops (it advances once more within the tick that writes the outcome); Headless::issue ignores commands afterwards.
22. Blockades are not recomputed inside succession calls; the next day start does it as before. Recomputing would have changed resupply within the same tick against the baseline.
23. Colonies abandoned for lack of people clear their endgame data (Endgame::clear_planet, one hook in the migration system), so a resettled planet is 'own' again. Abandoning and claiming stay where they were: a planet becoming free or being settled is not a change between communities, capitals are never abandoned and no community can end that way.
24. Fleets.arrears and Battles.ships_lost are sized at generation (they used to grow lazily), so 'all per-community lists have the same length' holds from the first tick.
25. reserve_floor is f32 like the reserve of a deposit; 0 is allowed (deposits run dry), 1 is not. max_free_states may be 0.
26. PolityDef.culture is required without default. Values chosen from name, motto and research focus: halde (0.45, 0.80, 0.35), vesper (0.30, 0.70, 0.65), irrlicht (0.85, 0.25, 0.30), thalassa (0.90, 0.60, 0.40), tiefenuhr (0.20, 0.55, 0.85), meridian (0.75, 0.35, 0.70), aschgrau (0.40, 0.90, 0.60), sonnwend (0.60, 0.95, 0.20); a test demands a pairwise distance above 0.15.
27. free_states counts foundings by secession only and is incremented in apply_secession, not in spawn, so that later callers of spawn (for example a union founding a new community) do not use up the cap.
28. The existing test a_battle_outlasts_the_secession_and_the_end_of_a_participant in crates/sim/tests/military.rs built the end of a community by hand (alive = false) and called the old apply_secession. It is rewritten on the API and asserts the new, designed semantics: after the capital secedes the community lives on with a new capital and fights on; when its last planet accedes it ends, the fleet passes to the heir, the war is over and the battle is called off. A second hunk there replaces an assignment of the whole arrears list by an indexed one.
29. Test layout: classify_is_total holds the table; the property loop is a test of its own (classify_never_panics_and_gives_one_coherent_verdict, 20,000 generated censuses); 'wards counted through their head by tag' is tested on census().
30. CSV: 'ending' (label or empty) and 'ending_year' (or empty) as the last two columns. Chronicle and error texts avoid possessive pronouns for communities, because their names have different grammatical genders.

## game_api_changes

Found by grep over /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/game/src; crates/game was NOT built (as instructed), so this is by reading, not by compiling.

No public item of sim that crates/game uses changed its name or signature. What the game uses from the touched areas:
- sim::diplomacy::Diplomacy as a read-only resource with is_at_war (ui/panels.rs:213) and at_war (ui/panels.rs:640): same names and signatures; they now resolve heads (a ward shares the wars of its head). Two new fields (standing, culture) are additive; the game never builds a Diplomacy by literal.
- sim::state::SimConfig.player (ui/panels.rs, ui/finance.rs, ui/fleets.rs, ui/mod.rs, map.rs): unchanged type Option<PolityId>. New behaviour: it can become None during a game when the player's community ends or is subordinated; Endgame.player is then PlayerFate::Out. The UI already handles None (observer), but this transition in the middle of a game is new.
- sim::state::SimState::generate / remove (session.rs and tests): unchanged signatures; SimState has the new field endgame.
- ChronicleKind::End (ui/panels.rs:313): already matched there; it is now actually written, exactly once per game. The enum is unchanged.
- Polity.capital (ui/panels.rs:625, ui/finance.rs:31, map.rs:292): unchanged; it is None for a community that has ended, as before.

Changed public items that the game does not use (no replacement needed there): sim::finance::apply_secession (now (&Secession, &mut Succession, tick)); sim::command::CommandContext (new fields config and endgame, logistics now ResMut); sim::harness::RunSummary (new last field outcome).

Additive for the game: resource sim::endgame::Endgame (outcome, player, path_of, is_seat, absorbed_pop, gap) and the functions census/classify for an end screen or a 'how close is the end' display; Diplomacy::{head, is_free, wards_of, bloc, affinity, culture_of}; sim::polity::{Succession, SuccessionParams, ...}; Balance.endgame; EconomyBalance.reserve_floor; PolityDef.culture.

Behaviour the game has to cope with: after an outcome the systems in sim::SimRoot no longer run and the clock stands still, so the game should show the end from Res<Endgame>.outcome; new chronicle texts appear (capital moved, community ended, subordination, build order lapsed). Scheduling: the monthly cohesion system and the command application now take ResMut<SimConfig> and ResMut<Endgame>; the game's FixedUpdate systems are all in BattleFeedSet, which is configured .after(sim::SimRoot) (battle_view/mod.rs:264-268), so no new ambiguity arises. Saves: the state layout changed, SAVE_FORMAT must be raised by the integrator.

## readme_text

## Besitzwechsel, Nachfolge und Unterstellung

- **Eine Stelle für jeden Wechsel.** Eine Welt wechselt nur über `sim::polity::succession` von einer Gemeinschaft zu einer anderen; dort entstehen nach Spielbeginn neue Gemeinschaften, und dort enden sie. Abspaltung, Übertritt, Eroberung und Vereinigung benutzen dieselben vier Vorgänge: gründen (`spawn`), übertragen (`transfer_planet`), beenden (`absorb`), unterstellen (`subordinate`). Jeder prüft zuerst und ändert dann; Geld, Anteile und Menschen bleiben dabei erhalten.
- **Übertragung einer Welt.** Mit der Welt gehen ihre Bevölkerung, ihre Betriebe und die Halter, die dort sitzen. Ein Teil der Anteile des alten Staates an ihren Betrieben geht an einen Empfänger, den der Anlass nennt; ihre Kurse geben nach; die Region beginnt mit dem Zusammenhalt und der Stufe, die der Anlass vorgibt. Bauaufträge des alten Besitzers bei ihrer Werft verfallen ohne Erstattung.
- **Fällt die Hauptwelt**, wird die bevölkerungsreichste verbleibende Welt Hauptwelt. Die Handelshäuser ziehen mit, ebenso ein Teil der alten Elite samt Besitz und Geld. Was der Elite der alten Hauptwelt im übrigen Gebiet bleibt, ist dort fortan fremder Besitz der Gemeinschaft, die die Hauptwelt jetzt hält.
- **Ende einer Gemeinschaft.** Verliert eine Gemeinschaft ihre letzte Welt oder geht sie in einer anderen auf, endet sie, und eine Erbin tritt an ihre Stelle. Ihre Welten behalten Stufe und Zusammenhalt. Staatskasse, staatliche Anteile, Anleihen, Handelshäuser samt Frachtern und das Wissen gehen über; was die Erbin sich danach selbst schuldete, ist getilgt. Ihre Kriege enden. Auf friedlichem Weg gehen auch Flotten und Bauaufträge über; tritt sie bei, bleibt ihre Hauptwelt als Sitz kenntlich. Unter Zwang werden Flotten und Aufträge aufgelöst, auch mitten im Gefecht.
- **Unterstellung.** Eine Gemeinschaft kann einer anderen unterstehen. Sie behält Welten, Staatskasse und Flotten, teilt aber die Kriege ihres Oberhaupts und kann selbst weder Krieg erklären noch Frieden schließen. Oberhaupt und Unterstellte bilden einen Verbund: In ihm herrscht nie Krieg, und Ketten gibt es nicht (wer einer Unterstellten unterstand, untersteht dem Oberhaupt). Kriege bestehen nur zwischen freien Gemeinschaften.
- **Neue Gemeinschaften.** Ein Freistaat übernimmt Steuersatz, Kultur und Technik des Reichs, das er verlässt, und beginnt ohne Staatskasse, Schulden, Flotte und Handelshaus. In einer Partie entstehen höchstens `endgame.max_free_states` Gemeinschaften durch Abspaltung; danach bleibt eine abtrünnige Region autonom.
- **Wege.** Jede Welt trägt, wie sie zu ihrer heutigen Gemeinschaft kam: eigen, beigetreten oder genommen, dazu ihre Bevölkerung und den kulturellen Abstand beider Gemeinschaften in diesem Augenblick. Eine genommene Welt bleibt genommen, durch jede spätere Vereinigung hindurch; erst wenn sie eine neue Gemeinschaft gründet, ist sie wieder eigen.
- **Kultur.** Jede Gemeinschaft hat drei Züge aus ihrer Vorlage (`polities.ron`): Offenheit, Gemeinsinn, Wehrhaftigkeit. Aus dem Abstand zweier Kulturen ergibt sich ihre Nähe.
- **Gespielte Gemeinschaft.** Endet die Gemeinschaft des Spielers oder wird sie unterstellt, ist er ausgeschieden; die Partie läuft ohne ihn weiter.

## Die beiden Enden

Einmal im Monat wird gezählt. Souverän ist eine freie Gemeinschaft, deren Verbund mindestens `endgame.sovereign_min_share` (2 %) aller Menschen hält; kleinere freie Gemeinschaften zählen nicht.

- **Metamorphose.** Es bleibt genau ein souveräner Verbund, und in ihm sind mehr Menschen beigetreten, als genommen wurden. Gezählt wird die Bevölkerung jeder Welt in dem Augenblick, in dem sie wechselte.
- **Auslöschung.** Es bleibt genau einer, aber die Genommenen überwiegen oder halten die Waage. Oder es bleibt keiner. Oder die Bevölkerung der Galaxie liegt `endgame.collapse_months` (24) Monate in Folge unter `endgame.collapse_share` (30 %) ihres Höchststands: Dann endet die Partie ohne Überlebende.

Jede Partie endet höchstens einmal. Mit dem Ausgang steht die Simulation still: Die Uhr rückt nicht mehr vor, und die Prüfsumme ändert sich nicht mehr. `cargo headless run` nennt Ende, Jahr und Überlebende; `cargo sweep` listet die Enden und schreibt sie in die letzten beiden Spalten der CSV (`ending`, `ending_year`).

Die Stellschrauben stehen in `assets/data/balance.ron` unter `endgame`. `economy.reserve_floor` ist der Rest, unter den kein Vorkommen fällt; bei 0 versiegen Vorkommen ganz.

## Ergänzung zu »Aufbau«

- `crates/sim/src/polity/succession.rs`: Besitzwechsel, Gründung, Ende und Unterstellung von Gemeinschaften, dazu ihre Invarianten.
- `crates/sim/src/fleet/succession.rs`: was dabei mit Flotten, Bauaufträgen und laufenden Gefechten geschieht.
- `crates/sim/src/endgame.rs`: die Wege der Welten, die monatliche Zählung und die Entscheidung über das Ende.
- `crates/sim/src/diplomacy`: Krieg und Frieden, Stellung (frei oder unterstellt) und Kultur.

## Ergänzung zu »Prüfen«

Die harten Invarianten (`sim::harness::check_invariants`) prüfen zuerst die Regeln der Nachfolge: Alle Listen je Gemeinschaft sind gleich lang, jede lebende Gemeinschaft besitzt ihre Hauptwelt, ein Oberhaupt lebt und ist frei, und nichts im Zustand nennt eine Gemeinschaft, die geendet hat. Geschichte ist davon ausgenommen: Chronik, Befehlsprotokoll, Bilanzen beendeter Gefechte und aufgelöste Flotten.

## files_changed

- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/assets/data/balance.ron
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/assets/data/polities.ron
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/headless/src/main.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/command.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/data/defs.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/data/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/diplomacy/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/economy/production.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/endgame.rs (new)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/finance/cohesion.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/finance/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/finance/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/finance/shares.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/fleet/mod.rs (one line)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/fleet/succession.rs (new)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/galaxy/plugin.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/harness.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/lib.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/polity/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/polity/succession.rs (new)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/state.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/src/tech/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/tests/military.rs (two hunks)
- /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge/crates/sim/tests/succession.rs (new)

## checks

All run in /home/user/exp_bevy_stellaris_fable_onePrompt/.worktrees/nachfolge on HEAD 816d69c9e6e5f6c9e5a475596a00bbef1bcbf299 with a clean worktree; output redirected to log files and judged by exit code. Logs: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/nachfolge/final2/ (codes.txt holds the exit codes).

REQUIRED FINAL STATE
1. cargo build -p sim -p headless: exit 0, no warnings.
2. cargo test -p sim: exit 0. 297 passed, 0 failed, 1 ignored (the ignored one is an existing tech test): lib 155, battle 45, economy 6, finance 9, military 34, succession 14, tech 34.
3. cargo clippy -p sim -p headless --all-targets -- -D warnings: exit 0.
4. cargo fmt -p sim -p headless -- --check: exit 0.
5. cargo build --release -p headless: exit 0.
6. target/release/headless sweep --seeds 0..8 --years 60: exit 0, stderr empty, no invariant violation reported. Hashes of seeds 0..7: d05fb35dbb1f8d8b, f83f97ab1f772f9c, 45c3859c5d54a45e, abe74bb3753ad9f7, adfdc8d62e3c46a0, c8e6e967a021880f, 2084a340b485663b, c009644769ecdb44. New last line of the summary: "Enden: Keine Partie endete in der Laufzeit."
7. target/release/headless hash --seed 3 --days 3600: run twice, outputs byte-identical; a third run with --exact is byte-identical too. Day 3600: 34959d70e729f059.

SWEEP COMPARISON with a binary built from 5ac904d (run with the original data directory)
- seeds 0..8, 60 years: the CSV is identical in all 20 common columns (every column except hash) for all 48 rows (cut -d, -f1,3-21, then cmp).
- Differences, both expected: (a) the hash column differs for every seed (baseline for example seed 0: 376768236e106f51) because the hashed state now contains Diplomacy.standing and culture, the Endgame resource and the two fleet lists sized from the start; (b) two new columns at the end, ending and ending_year, empty in all rows.
- Because peaceful runs contain no secession, I added a second comparison that does: sweep --seeds 0..8 --years 40 with nine scripted wars and one peace (command line in the summary). 77 rows, 29 free states founded (12, 10, 7, 10, 8, 9, 12, 9 communities per seed): identical to the baseline in all 20 common columns as well, exit 0, stderr empty. This is the evidence that apply_secession on top of spawn and transfer_planet behaves as before, and that the new cap of 6 free states does not bind earlier than the old limit of 12 communities.

FURTHER CHECKS
- cargo test -p headless: exit 0, 1 passed. cargo doc -p sim --no-deps: exit 0, no warnings.
- Mutation probe in an isolated copy of the sources (earlier commit fad85e5): 22 hand-made faults, each detected by at least one test (lost treasury, wars kept, owner_seen, elite cash, build orders, ward chains, a tie judged as Metamorphosis, no standstill, culture list not grown, least instead of most populous capital, Forced retagged as Joined, heritage counted as revenue, bond holder and issuer not relabelled, seat, trade-house home, tax exemption, state shares, fleets staying on the field, cap of free states, player kept, path not tagged). On the final commit one more probe: with the former seat rule restored, the new test a_last_planet_that_founds_a_community_is_its_own fails as it should.
- Physical collapse on the command line (at fad85e5, with a copied data directory, depletion x 1000 and reserve_floor 0): games end in "Auslöschung ... ohne Überlebende" (seed 2 in year 33), the hash is frozen afterwards and equal with --exact. The same scenario runs in every cargo test as part of the test 'endings'.

NOT RUN: crates/game was not built or tested, as instructed.

## commit

816d69c9e6e5f6c9e5a475596a00bbef1bcbf299 on branch wip/m6-nachfolge, pushed (origin/wip/m6-nachfolge points at the same commit), worktree clean. Nine commits on top of 5ac904d: f284c89, 0b9e4eb, 203929d, 8d21234, 48403e9, fad85e5, d773925, df295ac, 816d69c. All messages are German and end with the two required trailer lines. SAVE_FORMAT and README.md are untouched.

## open_issues

- SAVE_FORMAT must be raised by the integrator: the state layout changed (Diplomacy.standing and culture, the Endgame resource, fleet lists sized from the start). An older save would still deserialize through serde defaults but then fail succession::check on the list lengths.
- In AI-only play no community ends and no ward arises yet: capitals never secede and nothing but secession calls the new operations. The two endings therefore occur only through direct calls (tests) or physical collapse; the 60-year sweep reports 'Keine Partie endete in der Laufzeit'. The packages for the contest for planets and for diplomacy have to call transfer_planet, absorb and subordinate.
- Several fleet rules are not bloc-aware because the existing fleet files were deliberately left untouched: escorts against raids count only the victim's own fleets (raid_pressure), 'defended' in update_blockades, staying at home after an undecided battle, and resupply and repair only in systems of the fleet's own community. A ward's fleet does not protect its head's systems and the other way round.
- Merge guard by design: exhaustive destructuring means that a field added on another branch to Fleet, BuildOrder, Fleets, Battles, Planet, PlanetEconomy, Region, CohesionFactors, Polity, TradeHouse, Bond, Issuer, PolityFinance, PolityResearch, Economy, Finance, Logistics, Diplomacy, Research, Endgame or SimConfig stops the build in polity/succession.rs, fleet/succession.rs, finance/shares.rs, tech/mod.rs or endgame.rs until the field is named there and its fate at a change of owner is decided. Expect this when the fleet branches are merged.
- Tests and code on other branches will need adapting: anything that ends a community by hand (alive = false), assigns a whole per-community list (for example fleets.arrears = vec![..]), calls the old apply_secession signature or builds Diplomacy, Balance, EconomyBalance or PolityDef by literal. succession::check reports hand-made endings as violations. Any new state that stores a community id or is indexed per community has to be added to spawn, to the end of a community and to check.
- Merge spots in shared files: crates/sim/tests/military.rs (two hunks, one of them a rewritten test), crates/sim/src/harness.rs (one first line in check_invariants, early returns in the run loops, RunSummary.outcome as last field, an impl block at the end before the tests), crates/headless/src/main.rs (CSV header and format line extended at the end, lines appended to both summaries), assets/data/balance.ron (first block plus one line in economy), crates/sim/src/state.rs (entry after diplomacy in sim_state!).
- PolityDef.culture is required: any other data set or variant of polities.ron without the field fails to load.
- An accession of a region is passed as Path::Joined, as design D says, also when it was decided under war or blockade; if the later cohesion rules want such accessions to count as Forced, apply_secession is the place.
- PlayerFate::Merged is defined but never set by this package; a later union command sets it after absorb, as designed. The tags, the cultural distance and the seat flag are recorded but not yet read by any cohesion rule (out of scope).
- Free states still start without a trade house and without a fleet (unchanged from before).
- Claiming a free planet and abandoning a dead colony still change the owner outside succession.rs. I read 'the only place where a planet changes its owner' as a change between communities; if it is meant to include these two, they would have to move.
- Inert lazy-growth code is left in files I was asked to touch minimally: the resize in pay_upkeep and at the conclusion of a battle, Economy::sync_polities, book_income, Research::sync. Harmless now that spawn sizes every list; it can be removed later.
- Noted earlier in this session and not fixed because crates/headless is being changed by others: with --war and an unknown community index the core now rejects the command, but headless::war::print_aftermath may still panic.
- crates/game was not built. It should handle SimConfig.player becoming None in the middle of a game, show the end from Endgame.outcome and cope with the simulation standing still afterwards.

