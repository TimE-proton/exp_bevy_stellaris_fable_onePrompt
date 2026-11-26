# Nachbesserung

## summary

All nine points are resolved on wip/m4-spiel (HEAD 774b0fe, pushed, worktree clean); every required check passes after `cargo clean -p sim -p game`.

I built on 5ea3050 instead of reverting it. It compiled, its design was sound (new game and load installed only inside the state transition; a pointer state that knows "grabbed"; ownership of a press judged from egui's own widget data), and its nine red tests marked exactly the missing pieces. Reverting would have thrown away working fixes for findings 1, 2 and 6. I finished 2, 5 and 6, corrected three of its tests that expected things egui does not do, and wrote 3, 4, 7, 8 and 9 from scratch.

`cargo test -p game` now runs 151 tests, all green (136 green and 9 red at 5ea3050). Findings 1, 2, 4, 6, 8 and 9 were also checked in a real window under Xvfb.

Evidence is in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2 (acceptance logs and photos in the subfolder abschluss).

Only crates/game/** and the README section 'Bedienung' were touched. From memory I used nothing beyond the Xvfb/xdotool recipe, which the task repeated; no note asked for anything outside the task.

## fixed

- 1 (MAJOR, load crash) - Cause: Load and New Game swapped the simulation mid-frame while map entities and the selection still held ids of the old game. Change (session.rs, ui/mod.rs): the new state waits and is installed in OnEnter(InGame), after the old map is despawned and the selection cleared, the same transition where 'Zum Hauptmenü' removes it; the interface also forgets the chosen issuer and treasury inputs there. Proof: session::tests::loading_another_galaxy_inside_a_running_campaign_works and map::tests::the_map_follows_a_game_loaded_inside_a_running_campaign; real window: seed-1 campaign (51 systems) loading a seed-5 save (46 systems) three rounds by F9 and by Menü > 'Laden (F9)', no panic (f1_04_nach_f9.png, f1_06_nach_menue_laden.png, f1_a.log).
- 2 (mouse look) - Cause: the game treated a grabbed pointer as 'over nothing' while egui, which knows nothing of the grab, still delivered clicks and wheel to the widget under the hidden pointer. Change (pointer.rs, README): while grabbed, egui receives no pointer input, a click picks nothing in the scene, and the wheel goes to the camera only; only the release of a button egui still holds as pressed passes through. Proof: camera::tests::a_grabbed_pointer_clicks_neither_panels_nor_ships, the_wheel_of_a_grabbed_pointer_changes_the_fly_speed_and_scrolls_no_list, pointer::tests::a_grabbed_pointer_is_withheld_from_the_interface; real window p_m_vergleich.png ('Taktik' unchanged by a click under M, toggled without M).
- 3 (module list scroll) - Cause: the scroll state was keyed by ship index only, and indices restart with every battle while egui's memory lives on. Change (hud.rs): the feed epoch is part of the list's id. Proof: hud::tests::a_newly_selected_ship_starts_at_the_top_of_its_module_list, extended by a second battle.
- 4 (thin window) - Cause: the engine sizes the bloom texture as 512 high and aspect x 512 wide, which exceeds the device's texture limit beyond about 32:1. Change (camera.rs): a system after the engine's camera update sets the bloom height each frame from viewport size and the device limit; normal windows keep 512. Proof: camera::tests::the_glow_fits_the_graphics_card_at_every_window_size and a_window_drawn_into_a_thin_strip_keeps_the_glow_within_bounds; real window survived 1600x40, 2000x30, 3000x20, 1600x1, 3000x1, 40x900, 1x900, 20x20, 1x1 with no error line (p_a.log, g_a.log, p_08_wieder_gross.png).
- 5 (selftest and camera) - Cause: the census read the engine's visibility flag, which is set for any active camera, and the map camera also 'sees' the hulls without drawing them. Change (scene.rs, selftest.rs, args.rs, README; map side came with 5ea3050): hulls, wreck parts and map objects are counted from the VisibleEntities of the battle or map camera, and only if that camera is active and not painted over; otherwise the verdict is 'die Schlachtkamera zeichnet nicht' or 'die Kartenkamera zeichnet nicht'. Proof: selftest::tests::the_practice_selftest_fails_when_the_battle_camera_is_off, the_campaign_selftest_fails_when_the_map_camera_is_off, the_campaign_selftest_fails_when_the_battle_camera_paints_over_the_map.
- 6 (fast click on the resize strip) - Cause: the strip outside a panel was known only from the cursor icon of egui's previous pass. Change (pointer.rs): a press belongs to the interface if any interactive widget of egui's last pass lies within its interact radius, which is how egui itself assigns it; widgets inside areas that take no input (the notice) are skipped, as in egui's own hit test. Proof: map::tests::a_press_beside_a_panel_has_one_owner_even_without_a_run_up (move and press in one frame on the strip and just below the '5x' button), pointer::tests::a_press_just_beside_a_button_belongs_to_the_button, windows_cover_and_passive_areas_do_not; real window c_01_ausschnitt.png (fast click at 432,400 keeps the selection).
- 7 (fly speed) - Cause: the speed existed twice, in the engine's flight state (reset with the camera) and in a system-local copy that survived battles and was written back whenever the pointer was over the interface. Change (camera.rs): the copy is gone; the wheel over the interface no longer reaches the engine camera at all, so nothing needs restoring. Proof: camera::tests::a_new_battle_starts_with_the_normal_fly_speed.
- 8 (fleet panels) - Confirmed: only Active, lost and escaped were counted, so withdrawing ships, drifting ships and fighters in hangars stood in no column. Change (hud.rs, camera.rs, README): panels and Bilanz share one tally with columns kämpft / Rückzug / treibt / verloren / entkommen (Bilanz: anfangs / verloren / entkommen / treibt / im Feld); fighters aboard share their carrier's column; panels are 286 px wide instead of 236. Proof: hud::tests::every_ship_stands_in_exactly_one_column_at_every_moment (every preset, every tick, with and without an ordered retreat) and the_side_panel_shows_withdrawing_ships_and_every_row_adds_up (reads the drawn rows); photo p_03_rueckzug_befohlen.png (Großgefecht after 'Rückzug befehlen': Schlachtschiffe 0/6/0/0/0, Träger 0/4/0/0/0).
- 9 (transparent campaign panels) - Confirmed: the style's panel fill had alpha 238 and the window fill 245. Change (ui/panels.rs, hud.rs): both fills are opaque, for bars, side panels, chronicle and also the campaign windows; the battle view's special opaque frames are no longer needed. Proof: panels::tests::bars_panels_and_windows_of_the_campaign_are_opaque (shown failing with the old colours: alpha 238); photo abschluss/partie.png shows no map labels through the right panel or the Börse window.

## not_fixed


## checks

All with CARGO_TARGET_DIR=/home/user/exp_bevy_stellaris_fable_onePrompt/target in the worktree, after `cargo clean -p sim -p game`, judged by exit code. Logs are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/abschluss.

1. `cargo fmt -p game -- --check`: exit 0.
2. `cargo build -p game`: exit 0, no warnings; the binary embeds this worktree's crates/sim path.
3. `cargo test -p game`: exit 0, 'test result: ok. 151 passed; 0 failed; 0 ignored'.
4. `cargo clippy -p game --all-targets -- -D warnings`: exit 0.
5. Campaign selftest (`--selftest 200 --seed 3 --screenshot abschluss/partie.png`): exit 0, 'SELBSTTEST OK: 200 Frames, Datum 04.09.2400, Takt 4875, 16 von 90 Planeten besiedelt, 49 Kartenobjekte im Sichtfeld der Kamera'. Photo looked at: map, opaque panels, Börse window.
6. Practice selftest (`--practice --selftest 400 --seed 3 --screenshot abschluss/uebung.png`): exit 0, 'SELBSTTEST OK: Gefechtsübung, 204 Frames, Gefechtszeit 177,0 s (beendet), 36 Schiffe, 32 Wracks, ... 4 Fotos (1: 31 von 31 Schiffen und 57 Wrackteile; 2: 6 von 31; 3: 3 von 13 und 37 Wrackteile; 4: 2 von 2 und 432 Wrackteile)'. Photos uebung_1 to uebung_4 looked at: overview with every ship between the panels, close-up with ship panel, wreck with debris, Bilanz agreeing with the panels.

Checks 1 to 4 were repeated on the final commit, which differs from the selftest run only by a README paragraph; the binary was unchanged (build was a no-op).

## commit

774b0fe58aa8aab384c94e6b7ab6f06fb20f7464 on wip/m4-spiel, pushed (origin at the same commit). Commits on top of 5ea3050: 7800d34 (finding 1), 5e77a65 (3), 00dd135 (7), 58d6e24 (6), fe9532f (2), 7f268f1 (5), d852ae0 (4), 6fbf420 and 2bf0216 (8), 4e65c52 (9), 774b0fe (README paragraph).

## open_issues

- Decisions taken without asking: (a) while the pointer is grabbed a click has no effect at all, neither on a panel nor on a ship; (b) the fleet table has five worded columns and fighters aboard count in their carrier's column; (c) campaign windows were made opaque too, not only bars and panels; (d) Bloom stays on and shrinks instead of a minimum window size.
- Wider fleet panels (reserve 319 px each instead of 272) leave less room between them. To compensate I lowered the overview's threshold for using the room under the panels from 0.1 to 0.05, so a 900x900 window places the ships below the panels (camera::tests::a_squarish_window_uses_the_room_under_the_panels). This touches the earlier fix for 'Übersicht'; its tests stay green.
- In camera::tests::the_overview_shows_every_ship_beside_the_panels_that_are_open I replaced one precondition assertion ('ships are hidden before pressing U') by a geometric one (ship panel reaches further in than the side panel). With the wider side panels the old precondition no longer occurs; the assertions about the result are unchanged.
- Three tests inherited from 5ea3050 were corrected because they expected behaviour egui does not have: hover ends one pass after the pointer is reported gone, selectable labels do take presses, and the button the map test used sits above the right panel, not above the map.
- Intermediate commits are not individually green: the red tests inherited from 5ea3050 turn green one finding at a time; only HEAD was run through the full acceptance.
- The 'paints over' rule for cameras (active, higher order, non-transparent clear colour) fits this game's three cameras; it is not a general model of the engine's compositing.
- Skipping widgets of input-less areas is done by rectangle containment, because egui exposes rectangles only; an interactive widget of another layer lying wholly inside the notice's rectangle would be skipped too.
- Seen while resizing to absurd sizes: after the window was a 1-pixel strip, the Bilanz window stays where the strip pushed it (top edge, over the top bar) until dragged. Not changed.
- Only verified with software rendering on X11 (llvmpipe, where a locked pointer falls back to confined); a real GPU and true pointer lock on other platforms are untested.
- Worth remembering, all learned in this session: egui 0.36 `interactive_rects_last_pass()` also lists widgets of non-interactable areas although its hit test skips them; a camera that is switched off keeps its stale VisibleEntities list; `NextState::set` to the current state does run OnExit/OnEnter and DespawnOnExit; number keys 1-6 map to ½x, 1x, 2x, 5x, 10x, 25x.
- Photos suited for showing the user: abschluss/partie.png, abschluss/uebung_1.png to uebung_4.png and p_03_rueckzug_befohlen.png, all under /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2.

