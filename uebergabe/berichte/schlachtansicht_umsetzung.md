# 3D-Schlachtansicht und Gefechtsübung: Bericht der Umsetzung (Hauptzweig, 8206ed1)

## summary

Milestone 4c is finished and committed on branch claude/keen-franklin-aqi4h7 (HEAD 8206ed1, not pushed by me). crates/game now has a 3D battle view (battle_view::BattleViewPlugin) and the practice-battle mode "Gefechtsübung". The milestone diff d883061..HEAD touches only crates/game: 17 files, +8220/-39, of which 9 files (about 7500 lines) are new under crates/game/src/battle_view/. crates/sim, crates/headless, assets/data, Cargo.toml, Cargo.lock and README.md are unchanged; no engine feature was added.

Verified on the final commit: format check, build, 64 tests, clippy with -D warnings, the campaign selftest (SELBSTTEST OK) and three practice selftests (default battle, 496-ship battle, class line-up), each with its OK line. I looked at every screenshot.

Not verified:
1. Interactive use in a real window (mouse look, mouse wheel, clicking ships, clicking HUD buttons) was never exercised. This machine has no input-automation tool, and my attempt to install one (apt-get install -y xdotool) was denied by the permission system; I did not work around that. These paths are covered only by headless system tests (click selection with a window cursor position, hotkeys, camera requests, follow, HUD drawing and its actions).
2. Everything was rendered in software under Xvfb at 3-5 fps. I have no GPU frame rate and have not seen a battle in fluid motion; interpolation, banking, wreck drift and effect timing are checked by tests and still frames only.
3. Entering the view from a campaign was checked once with a temporary probe (removed again), not with real campaign battles; the campaign bridge is M4e.

What exists, by the 13 points of the brief:
1. BattleFeed resource: state after the last tick, previous poses, epoch, per-side name and colour, player side. View systems read nothing else about the battle.
2. BattleOrder message (SetOrders, Retreat) plus BattleViewRequest (Leave, Restart) back to the owner.
3. AppState::Practice owns a Battle built with matchup::build and steps it in FixedUpdate. The main-menu button "Gefechtsübung" opens a setup dialog (four presets, free fleet lists, formation and doctrine per side, seed). CLI: --practice --fleet-a --fleet-b --seed, plus --lineup. Esc or "Verlassen" returns to the menu; the "Bilanz" panel offers "Neu starten", "Zurück" and "Feld ansehen".
4. ViewState::Battle shows the same view: it is active whenever it is shown and the feed holds a battle; enter/exit handling exists for both states.
5. Scene: plane (x, y) maps to world (x, 0, -y); one entity per ship, created lazily; shared mesh and material handles (the side colour is baked into per-side meshes, all hulls share three materials); poses interpolated between ticks; bounded pools for tracers, missiles, beams, impact flashes and shield caps.
6. Procedural meshes for the six classes with distinct silhouettes; capital-ship wrecks break into fore and aft halves.
7. Effects: engine glow, tracers, missiles, beams (misses pass the target, point defence as short streaks), impact flashes, shield shimmer, wrecks that glow and cool, debris.
8. Readability: class markers that fade in as ships get small on screen, optional name labels, reference rings and axes, a toggleable tactical overlay (spawn ring, fleet anchors with facing, weapon range and target line of the selected ship), star field, key and fill light, bloom.
9. Camera: the engine's free camera plus overview, fight centre, focus and follow; all keys are listed in the in-game help ("Hilfe" / F1).
10. Click selection and an inspector with hull, shield, four armour facings, ammunition, fuel, morale, target link and the module table (intakt / beschädigt / ausgefallen).
11. HUD: one panel per side (class table kämpft / verloren / entkommen, Kampfwert, Moral) with formation, doctrine and retreat buttons for the player's side.
12. Selftest: --practice --selftest N --screenshot F takes four photos chosen from the battle state (overview, exchange of fire, wreck, Bilanz) and prints "SELBSTTEST OK: Gefechtsübung, ..." with timings; --lineup photographs the classes.
13. Tests: 49 in battle_view (mesh validity and bounds, pose interpolation including the ±π seam, plane picking, HUD drawn headless, pools stay bounded, feed copy equals the core's state hash every tick, practice stepping and orders) plus new argument tests.

One fix outside the brief: quitting a campaign to the main menu while time was running crashed; see deviations.

Memory: I consulted the stored note "cloud-render-check" (running the game under Xvfb with software rendering, piped output hides cargo's exit code, first build time); it matched the task's instructions and nothing in memory asked for anything beyond the task. Worth saving, all learned in this session:
- Bevy 0.20 evaluates a system's own run condition even when its set's condition is false (from the panic in sim::clock::day_start when FixedUpdate ran without a SimClock).
- Bevy 0.20 names the run-condition combinators or_else / and_then (compiler error on .or()).
- Mesh3d gets Visibility only through the visibility plugin at runtime, so headless test worlds must add Visibility explicitly (from a failing pool test).
- Under Xvfb with software rendering the first ~45 frames of a 3D scene come out black while pipelines compile (from black line-up photos).
- The core's FixedUpdate schedule treats unordered access to one resource as an error (found with the campaign probe).
- Installing system packages is denied by the permission system in this environment, so GUI input cannot be automated here.

## screenshots

All 1600x900, software-rendered under Xvfb. They are in the session scratch directory, which can be cleared; the commands below regenerate them. Battle state and the numbers in the OK line are deterministic; the close-up framing varies by a few pixels between runs because the camera easing runs on real time.

Campaign (--selftest 200 --seed 3):
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/karte.png — galaxy map with top bar, planet panel "Besitz", "Börse" window, list of polities and chronicle. Looks as before this milestone; the new plugin leaves the campaign UI alone.

Default practice battle, 18 vs 18 (--practice --selftest 400 --seed 3):
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/gefecht_1.png — overview at 00:33: blue line left, red line right on the reference plane with rings and axes, fleet anchors with facing arrows in side colours, blue and red beams, tracers, impact flashes; top bar with time controls and view buttons; side panels with class tables, Kampfwert and Moral bars, and the order buttons for blue. Weakness: at this distance hulls are 5-40 px, so only capital ships are recognisable by shape; all beams are drawn at the minimum pixel width and look equally heavy; the outer rings run behind the side panels.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/gefecht_2.png — close-up: a red "Speer" cruiser (grey hull, red stripes, bridge, turrets, side pods) firing a laser at a blue corvette whose shield lights up; selection ring, target line, inspector with all bars, target link "Dolch (Korvette)" and module table. Weakness: the engine glow is a hard-edged cone, not a plume; the module table runs past the lower window edge; the look is flat-shaded low-poly without textures.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/gefecht_3.png — wreck at 00:49: a glowing cruiser wreck broken in two with debris, a cold frigate wreck, a blue corvette with shield shimmer, two tracers, a blue cruiser passing. Weakness: the hot wreck is one flat orange tint; the cold wreck is almost invisible against black; debris pieces are few and small.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/gefecht_4.png — "Bilanz" at 02:57, BEENDET: "Roter Verband behauptet das Feld.", duration, per-side tables (anfangs / verloren / entkommen / im Feld), "Verlust am Bauwert 80 % / 67 %", buttons "Neu starten", "Zurück", "Feld ansehen"; blue panel shows RÜCKZUG with greyed-out orders. Weakness: the window covers the centre of the field and a red anchor marker shows faintly through it.

Large battle, 200 vs 200 plus fighters, 496 ships (--practice --fleet-a "bastion:6,horst:4,speer:30,waechter:60,dolch:100" --fleet-b "bastion:10,speer:40,waechter:60,dolch:90" --selftest 600 --seed 1):
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/gross_1.png — overview at 00:10: both fleets as class markers (size and shape by class), four blue carriers with fighter swarms, fighting at both ends of the lines. The markers keep 400 ships readable. Weakness: the lines are longer than the field, so the fight happens only where the ends meet at the rim (a core formation limit, see api_notes).
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/gross_2.png — a red "Bastion" battleship firing a heavy lance, more heavy beams and the blue line in the background, inspector with its modules. Bloom and beam weights read well here. Weakness: four hard cone glows at the stern; the inspector hides a neighbouring battleship; module table cut at the bottom.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/gross_3.png — a cold corvette wreck with red cruisers passing in line. This is the weakest photo: the wreck differs from a live ship only by a darker tint and the missing engine glow, and there are just a few specks of debris.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/gross_4.png — "Bilanz" at 01:56 over the field with the surviving red line. Weakness: red markers show through the semi-transparent window behind the table.

Class line-up (--practice --lineup --selftest 140):
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/klassen_1.png — all six classes of both sides with name labels (Wespe · Jäger to Horst · Träger). Silhouettes and sizes are clearly different: dart, twin-boom, rounded hull with pods, long hull, broad wedge with turrets, catamaran with flight decks. Weakness: the blue carrier's label sits on its hull; the fighter is barely recognisable at this distance.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/klassen_2.png — fighter and corvette close up. Weakness: the frigate is cut by the right edge; a red label shows through the right-hand panel.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/klassen_3.png — frigate and cruiser close up. Weakness: the battleship and its label are cut by the right edge.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/klassen_4.png — battleship and carrier close up with turrets and deck markings. Weakness: the two carrier labels overlap hulls, and red labels show through the left-hand panel.

Overall: the view is readable and the classes are distinct, but it is a plain, stylised look. Engine glows, wrecks and debris are the least convincing parts.

## measurements

The selftest measures the CPU time of the view's own systems per frame (scene sync, effects, overlay lines, HUD), not the frame time. Machine: shared 4-core VM, dev profile, software rendering, so single-frame maxima are noisy. Render-thread cost could not be measured meaningfully.

| Battle | Ships | Avg per frame | Median | Worst frame | Szene / Effekte / Linien / Tafeln (avg µs) | Core step per tick | Feed copy per tick |
| Default, seed 3 | 36 | 603-657 µs | 524-542 µs | 2.2-4.8 ms | 18 / 8 / 17 / 559-613 | 14 µs | 2-3 µs |
| Large, seed 1 | 496 (443 at once, 333 wrecks) | 716-780 µs, up to 986 µs in repeats | 639-700 µs | 2.4-4.2 ms, up to 19.7 ms in repeats | 81 / 16 / 16-22 / 603-663 | 178-201 µs | 28-30 µs |
| Stress, 300 vs 300 gun ships (earlier build, not re-run) | 600 at once | 929 µs | not recorded | 24.7 ms | 83 / 29 / 23 / 794 | 536 µs | 50 µs |
| Line-up | 12, still | 467-469 µs | 355-359 µs | 5.4-8.0 ms | 21-24 / 3 / 15 / 428 | none | none |

Effects shown at peak: default 45 projectiles and 20 beams in 130 pool slots; large 236 projectiles and 69 beams (100 in one repeat) in 701 slots; stress 1043 projectiles and 160 beams in 1519 slots. Pool caps: 6000 tracers, 1500 missiles, 1500 beams, 700 flashes, 400 shield caps.

Reading: scene, effects and lines together cost about 0.05-0.14 ms even with 400-600 ships. The egui HUD dominates at 0.45-0.8 ms and barely depends on ship count. The worst frames are always the HUD part: the first frame of a newly opened panel costs 2-8 ms, and the larger outliers are scheduling noise on the shared machine. A trial with opt-level 3 for the game crate changed nothing, so the HUD cost is inside egui.

## api_notes

Everything is in crates/game/src/battle_view/. Exported from battle_view: BattleFeed, BattleOrder, BattleViewRequest, CameraRequest, SelectedShip, ViewOptions, EffectStats, BattleFeedSet, BattleViewSet, BattleViewShown, view_active; from battle_view::feed also FeedInfo, SideInfo, Pose.

BattleFeed (Resource, created by the plugin)
- fn begin(&mut self, battle: &Battle, info: FeedInfo): start showing a battle. Bumps `epoch` (scene and pools rebuild) and clones the battle once.
- fn refresh(&mut self, battle: &Battle): take over the current state. When battle.tick changed it first stores the previous poses and derives shield flashes and effect lifetimes. Does nothing before begin. Reuses its buffers.
- fn clear(&mut self): nothing to show.
- fn battle(&self) -> Option<&Battle>, fn is_active(&self) -> bool, pub epoch: u32, pub info: FeedInfo.
- FeedInfo { title: String, sides: Vec<SideInfo { name: String, color: Rgb }>, player_side: Option<usize>, can_restart: bool, still: bool }. `sides` follows the order of Battle::combatants (index = BattleShip::side); a missing entry shows as "Unbekannt" in grey. player_side = None gives a spectator HUD without order buttons. still = true means shown but not stepped.

Messages (registered by the plugin)
- BattleOrder::SetOrders { side: usize, formation: Formation, doctrine: Doctrine } and BattleOrder::Retreat { side: usize }. The HUD writes them only for info.player_side.
- BattleViewRequest::Leave ("Verlassen" in the top bar, "Zurück" in the Bilanz, Esc in practice mode) and BattleViewRequest::Restart ("Neu starten", offered only when info.can_restart).

What a campaign bridge has to do
1. Enter: feed.begin(&battle, FeedInfo { title: system name, sides: one SideInfo per combatant in order, player_side, can_restart: false, still: false }), then NextState<ViewState>::set(ViewState::Battle). Fill the feed before or in the same frame: while ViewState::Battle is shown the campaign panels are hidden even if the feed is empty (only the pause menu remains).
2. Every tick: a FixedUpdate system in BattleFeedSet::Campaign calls feed.refresh(&battle), for example
   app.add_systems(FixedUpdate, mirror.in_set(BattleFeedSet::Campaign).run_if(in_state(ViewState::Battle)));
   The set is mandatory. It is ordered after sim::SimRoot and after BattleFeedSet::Practice, and the core's FixedUpdate schedule treats unordered access to one resource as an error.
3. Tick cadence: the view interpolates with Time<Fixed>::overstep_fraction(), so it expects one new battle tick and one refresh per FixedUpdate run. Several battle ticks per run with one refresh at the end still move smoothly, but shield flashes and effect lifetimes are sampled coarsely. A battle stepped less often than every run will stutter; then the alpha source in battle_view/mod.rs (update_poses) must change.
4. Orders: read MessageReader<BattleOrder> in Update and turn each into the SimCommand the core defines for battle orders, so it lands in the replay log. Call feed.refresh after the core applied it if the panel should react while paused. Practice mode calls Battle::set_orders(&data, side, formation, doctrine) and Battle::order_retreat(side) directly, which is acceptable only because practice has no replay.
5. Leave: on BattleViewRequest::Leave set ViewState::Galaxy and call feed.clear(). Until the bridge does this, "Verlassen" and "Zurück" do nothing inside a campaign.
6. End of battle: the view opens the Bilanz when battle.finished is set. If the core drops a finished battle, stop refreshing and keep the last state in the feed until the player leaves.
7. Watching another battle: call begin again.
OnEnter/OnExit(ViewState::Battle) switch cameras, lights, star field and UI; nothing else needs calling. SimSpeed drives time in both modes, and the time keys work in the view. The strategic simulation keeps running while the player watches unless the bridge pauses it. Map controls are off in the view (gated on ViewState::Galaxy). More than two sides are handled in code (a kit and a panel per side) but were never run.

Core items the view depends on (a rename or type change breaks crates/game)
- sim::battle::Battle: id, system, seed, tick, time, finished, timed_out, combatants, hostile, ships, shots, beams, impacts, wrecks; methods step, set_orders, order_retreat, active_by_class, fighting_value, is_fighting, is_hostile.
- BattleShip: pos, heading, class, status (with in_play()), side, design, radius, hull/hull_max, shield/shield_max, armor/armor_max, modules, ammo/ammo_max, fuel/fuel_max, morale, target, speed/max_speed, main_range, hangar. feed::copy_state needs every field except `modules` to stay Copy.
- Combatant: formation, doctrine, anchor, facing, retreating, initial, lost, escaped, initial_value, enemy_center.
- Shot: pos, vel, side, kind, damage. Beam: from, to, side, kind, hit, ttl. Impact: pos, size, ttl. Wreck: pos, vel, heading, class, side, radius, age.
- Ordering: ship and wreck indices are stable; shots, beams and impacts are compacted in order with new entries appended. The feed relies on this to follow an effect across ticks.
- sim::battle::matchup::{Side::parse, build, outcome_of}; sim::battle::math::wrap_angle; Formation and Doctrine with label(); GameData designs, modules, balance.battle.{dt, exit_radius, spawn_radius}; sim::data::Rgb; sim::hash::hash_value (tests); sim::SimRoot; sim::clock::SimClock; sim::TICKS_PER_SECOND.
- A new public field on Battle must be added to feed::copy_state. A test compares the feed's copy with the core's state hash after every tick and fails if one is missed.

Changes I would want in crates/sim (none made)
1. The run conditions day_start and month_start should tolerate a missing SimClock (Option<Res<SimClock>>). Bevy evaluates a system's own run condition even when its set is switched off, so any FixedUpdate run without a campaign panics. crates/game works around it: practice mode inserts a placeholder SimClock, and time is paused before a campaign or practice battle is removed.
2. Very large fleets: a single line of about 200 ships is longer than the field. Its ends sit at the rim, the fight happens only there, and many ships count as "entkommen" within seconds (27 blue corvettes and 14 frigates after 10 s in the 496-ship run). I observed this in the view and did not analyse the cause in the core; wrapping the line into ranks or scaling the field with fleet size would address it.
3. Beam and Impact carry no shooter or target index. The view infers shield hits from shield deltas between ticks and the hit direction from the ship's target; a target index and a "stopped by shield" flag would make this exact.
4. Impact.pos is the end of the shot's flight segment and can lie behind the hull it hit; the contact point would look right.
5. After `finished`, beams and impacts keep their remaining ttl forever; the view hides them once a battle is finished.
6. Battle has private fields, so the game cannot copy it field by field without allocation; a clone_from in the core that reuses buffers would replace feed::copy_state.
7. For M4a, not a view matter: in the 200-vs-200 run the sides had lost 59 and 51 corvettes after 10 s of battle time.

## files_changed

- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/feed.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/meshes.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/scene.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/effects.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/camera.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/overlay.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/hud.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/practice.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/args.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/main.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/selftest.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/session.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/state.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/time_control.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/mod.rs
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/panels.rs

## checks

All run on the final commit 8206ed1 with output redirected to a log file and the exit code checked; the working tree is clean afterwards.

- cargo fmt -p game --check: exit 0.
- cargo build -p game: exit 0, no warnings.
- cargo test -p game: exit 0, "test result: ok. 64 passed; 0 failed; 0 ignored".
- cargo clippy -p game --all-targets -- -D warnings: exit 0.
- Campaign selftest (--selftest 200 --seed 3 --screenshot ...): exit 0, "SELBSTTEST OK: 200 Frames, Datum 03.09.2400, Takt 4850, 16 von 90 Planeten besiedelt", no panic in the log.
- Practice selftest, default battle (--practice --selftest 400 --seed 3 --screenshot ...): exit 0, "SELBSTTEST OK: Gefechtsübung, 203 Frames, Gefechtszeit 177,0 s (beendet), 36 Schiffe (36 zugleich im Feld), 32 Wracks, höchstens 45 Geschosse und 20 Strahlen gezeigt, 130 Effektplätze, 4 Fotos, Ansicht 603 µs im Mittel (Median 524 µs) und 2169 µs höchstens je Frame ...; Gefechtsrechner 14 µs und Feed 2 µs je Takt".
- Practice selftest, large battle (fleets as in the screenshots field, --selftest 600 --seed 1): exit 0, "SELBSTTEST OK: Gefechtsübung, 179 Frames, Gefechtszeit 117,0 s (beendet), 496 Schiffe (443 zugleich im Feld), 333 Wracks, höchstens 236 Geschosse und 69 Strahlen gezeigt, 701 Effektplätze, 4 Fotos, Ansicht 716 µs im Mittel (Median 639 µs) und 2362 µs höchstens je Frame ...; Gefechtsrechner 178 µs und Feed 28 µs je Takt".
- Practice selftest, line-up (--practice --lineup --selftest 140): exit 0, "SELBSTTEST OK: Gefechtsübung, 97 Frames, Gefechtszeit 0,0 s (Schaustellung), 12 Schiffe ..., 4 Fotos, Ansicht 469 µs im Mittel (Median 355 µs) ..."; its four photos are byte-identical to the ones I inspected.

Run on earlier builds only, not repeated on the final commit (which differs from them by small fixes and the help text):
- Carrier preset and skirmish battles: SELBSTTEST OK.
- 600-ship stress battle: SELBSTTEST OK.
- Failure path, --practice --fleet-a "unsinn:3" --selftest 100: "SELBSTTEST FEHLER: Die Gefechtsübung hat nicht begonnen", exit code 1, as intended.
- Temporary probes, reverted before committing: entering ViewState::Battle inside a campaign with a filled feed and returning to the map; a screenshot of the setup dialog in the main menu.

## deviations

- Selftest frame count: with --practice, --selftest N is an upper limit (minimum 120). The run ends once the battle is over and the photos are taken; the campaign selftest still runs exactly N frames. The --help text says so.
- Photos: four instead of three for a battle (the fourth is the Bilanz) and four for the line-up (all classes, then three close-ups in pairs).
- Overview: frames the ships that are in play, not always the whole field, because the whole field left the default battle tiny.
- Picking: the click is first intersected with the battle plane and matched to a ship's circle; if that misses, the ship nearest to the ray within 11 px is taken.
- Hull size: meshes have length 2, scaled by BattleShip::radius, so a hull is as long as its collision diameter.
- Side colour is baked into per-side copies of the class meshes (vertex colours) instead of one material per side; all hulls share three materials (active, routing, disabled).
- FeedInfo.still was added for the line-up, and BattleViewRequest was added next to BattleOrder for "Verlassen" and "Neu starten".
- Practice starts paused, like a campaign. The player commands the blue side; red follows its doctrine.
- The Bilanz can be hidden ("Feld ansehen") and reopened ("Bilanz" in the top bar). The inspector sits on the side opposite the player's panel.
- Practice mode inserts a placeholder SimClock and removes it on leaving, because of the core's run conditions (see api_notes, wanted change 1).
- Outside the brief, in crates/game/src/session.rs: quitting a campaign to the main menu now resets the speed and pauses time before the simulation is removed. Without it the same run-condition panic hit the campaign whenever the player quit while time was running.
- crates/game/src/time_control.rs: the time keys also work in practice mode.
- README.md was not updated because the commit scope allowed only crates/game. The --help text documents the new options and the in-game help lists the keys.
- Esc leaves a practice battle at once, without a confirmation.

## open_issues

- Real-window interaction is untested: mouse look with the right button, mouse wheel, clicking ships and HUD buttons. There is no input-automation tool on this machine and installing one was denied by the permission system. It needs a manual pass on a real machine: fly, select, give orders, restart, leave.
- No GPU numbers. Under software rendering the game ran at 3-5 fps, so the render cost of 400+ ship entities with bloom is unmeasured and motion was never seen fluidly.
- In a campaign, "Verlassen" and "Zurück" do nothing until the M4e bridge consumes BattleViewRequest::Leave; BattleOrder is likewise unconsumed there.
- The view expects one battle tick per FixedUpdate run. If the campaign steps battles at another cadence, motion will stutter and update_poses in battle_view/mod.rs needs a different alpha source.
- HUD cost: the egui panels take 0.45-0.8 ms per frame regardless of ship count, and the first frame of a newly opened panel takes 2-8 ms.
- Markers and name labels are painted on egui's background layer and show through the semi-transparent panels and the Bilanz window; labels also overlap hulls (visible in klassen_2, klassen_4, gross_4).
- Engine glows are hard-edged cones rather than soft plumes, in every close-up and also on the still ships of the line-up.
- Wrecks are weak: hot wrecks are one flat orange tint, cold wrecks are nearly invisible against black, small ships leave very little debris (gross_3), and wrecks stop drifting when the battle ends.
- All ships fly in one plane, so fighters pass through hulls; there is no cosmetic height offset.
- The inspector's module table runs past the lower window edge at 900 px for ships with many modules (it scrolls, but that is not obvious). Window sizes other than 1600x900 are untested.
- Core: a line of about 200 ships is longer than the field; the fight happens only at the rim and many ships count as escaped within seconds. Details and six further wishes for crates/sim are in api_notes.
- More than two sides are handled in code but were never run.
- The mouse wheel changes the fly speed (engine behaviour) instead of zooming; the top bar shows it as "Flug x,x×". The engine's free camera also logs its English key list once at startup.
- README.md lacks a section on --practice, --lineup and the battle controls.
- Cosmetic: #[allow(dead_code)] on ViewState::Battle in crates/game/src/state.rs is no longer needed.

## commit

Two commits of mine on claude/keen-franklin-aqi4h7, both in German with the two required trailer lines, added with "git add crates/game" only; the branch is 2 ahead of origin and I did not push:
- d52304f "Meilenstein 4: 3D-Schlachtansicht und Gefechtsübung"
- 8206ed1 "Kommandozeilenhilfe: Selbsttest der Partie und der Gefechtsübung unterschieden" (HEAD)

They sit on top of four "Zwischenstand" commits (b9085cd, f595580, c007c74, 0b4777c) that I did not create. These appeared on the branch during my work and hold earlier states of my working tree, presumably snapshots by the main session. My two commits therefore contain only the remaining delta; the full milestone is d883061..8206ed1.

