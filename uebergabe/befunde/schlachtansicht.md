# Prüfbefunde: 3D-Schlachtansicht und Gefechtsübung (Hauptzweig, 8206ed1)

Drei Prüfer, Befunde einzeln gegengeprüft, drei blieben ungeprüft. Die Nachbesserung ist nicht gelaufen: alle bestätigten Befunde sind offen (Paket B). Mehrere Befunde meinen denselben Fehler aus verschiedenen Blickwinkeln.

Zählung: 19 bestätigt, 3 widerlegt, 3 ungeprüft.

## Blickwinkel: robustheit

### robustheit 1 · bestätigt · critical · Quitting a campaign to the main menu still panics: map systems run after the sim state was removed in the same frame

Ort: `crates/game/src/session.rs:134`

**Beschreibung.** handle_requests processes SessionRequest::QuitToMenu inside Update: it resets SimSpeed, pauses Time<Virtual> (the two lines this milestone added) and then calls SimState::remove(world) at once, while AppState stays InGame until the next frame's StateTransition. InGame-gated systems that are scheduled later in the same Update pass still require the removed resources: map::focus_camera (map.rs:369), map::sync_ownership (map.rs:428) and map::draw_overlays (map.rs:527) fail validation of Res<Galaxy>, Bevy's default error handler panics and the process exits with code 101. Reproduced in a real window in 4 of 4 attempts (time paused, 1x, 10x, 25x), so it does not depend on running time. The milestone's pause only removes the SimClock run-condition panic; the reported fix ('quitting a campaign to the main menu while time was running crashed') does not hold for the normal galaxy view (a probe that quit from ViewState::Battle would not see it, because the map chain is gated on ViewState::Galaxy). The code path itself predates M4c (session.rs and map.rs are otherwise unchanged; not verified on d883061). Consequence for this milestone: 'practice after a campaign' and 'campaign -> menu -> campaign' cannot be reached without restarting the program. Everything else in the lifecycle was exercised in a real window without a crash or leftover entities: menu -> practice -> menu -> practice (more than 12 cycles, also leaving at 25x and with a grabbed cursor), restart, practice -> menu -> campaign (map and map camera come back), line-up -> normal practice, a 496-ship battle left mid-fight followed by a small one, window sizes from 1x1 to 1600x900, selection of a destroyed ship. cargo test -p game: 64 passed; clippy clean; selftests end with their OK line and exit 0 (campaign, practice default, line-up, 496 ships, empty fleet, fighter-only fleets; an invalid fleet gives SELBSTTEST FEHLER and exit 1 as intended). None of the selftests quits to the menu, so they do not cover this path. Evidence: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s05_paused/game.log (also runs/s05_2, runs/s05_6, runs/s04 next to it). Notes for the main session, not part of the defect: (1) Interactive GUI testing works in this container without installing anything: Python ctypes on the preinstalled libX11/libXtst (XTestFakeKeyEvent, XTestFakeButtonEvent, XTestFakeMotionEvent) inside xvfb-run, XSetInputFocus on the game window because there is no window manager, screenshots with ImageMagick 'import -window root'. My driver is /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/tools/xdrive.py, the launcher /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/tools/session.sh, scenarios s01 to s13 lie beside them (learned in this session by trying). (2) Bevy 0.20 NextState::set re-runs OnExit/OnEnter on same-state transitions (bevy_state-0.20.1 src/state/resources.rs:198). (3) From memory I used only the note 'cloud-render-check' (Xvfb recipe, exit-code advice); it matched and asked for nothing beyond the task.

**Fehlerszenario.** Start the game, click 'Neue Partie', press Esc, click 'Zum Hauptmenü' (paused or with time running): the picture freezes and the process ends with "Encountered an error in system `game::map::focus_camera`: Parameter `Res<Galaxy>` failed validation: Resource does not exist" (the same for sync_ownership and draw_overlays), exit code 101. The main menu is never reached, so a practice battle after a campaign is impossible.

**Vorschlag.** In QuitToMenu only reset SimSpeed, pause Time<Virtual> and set NextState to MainMenu; remove the SimState in OnEnter(AppState::MainMenu). Do not use OnExit(AppState::InGame) for the removal: enter_game calls NextState::set(InGame) while already InGame (F9 load), and Bevy 0.20 runs OnExit/OnEnter for such same-state transitions. Alternatives: run handle_requests after every InGame-gated Update system (for example in Last), or give those systems Option<Res<..>>. Add a headless test that sends QuitToMenu and runs one Update with the map plugin.

**Gegenprüfung.** Confirmed by reading the game code and the engine sources; I could not refute it. I did not start the game or build anything. The finder's four run logs (current HEAD, clean tree, binary newer than the sources) show exactly the outcome the code predicts.

Chain of facts, each checked in source:
1. The button "Zum Hauptmenü" (ui/panels.rs:691-692) makes draw_ui write SessionRequest::QuitToMenu (ui/mod.rs:239-241). draw_ui runs in EguiPrimaryContextPass, which bevy_egui runs in PostUpdate, so the request is handled in the NEXT frame's Update.
2. handle_requests (exclusive, in Update, no ordering against anything: session.rs:64) calls SimState::remove(world) at session.rs:140, which removes Galaxy and all other sim resources (sim/src/state.rs:53-55), and only queues NextState (session.rs:141-143).
3. StateTransition runs after PreUpdate, before Update (bevy_state app.rs:335). For the rest of that frame AppState is still InGame and ViewState still Galaxy, so the map chain (map.rs:87-104) is not gated off.
4. Res<T> for a missing resource is an "invalid" validation error, not a skip (bevy_ecs system_param.rs:712-714). It becomes RunSystemError::Failed, goes to the fallback error handler, which defaults to match_severity; a BevyError's default severity is Panic (bevy_error.rs:541-547, handler.rs:118-137). The executor catches the panic and resumes it at the end of the schedule (multi_threaded.rs:305-309, 867-877). The game never calls set_error_handler. Result: process exit 101.
5. It is deterministic for this flow, not a rare race. rebuild_map has Commands and chained successors, so an exclusive ApplyDeferred sits between it and camera_controls, forcing a moment with nothing running. camera_controls is then skipped by not(egui_wants_any_pointer_input), because the pointer is over the egui menu just clicked (the flag is only refreshed in PostUpdate). The executor loop continues and runs the waiting exclusive handle_requests before focus_camera. So focus_camera (map.rs:369-371), sync_ownership (428-429) and draw_overlays (527-529) fail on Res<Galaxy>; pick_system is condition-skipped and level_of_detail needs no sim resource. That is exactly the set of three errors in all four logs. The only way to send QuitToMenu is this egui button, so the pointer is always over egui.

Scope: the crash path itself predates this milestone (map.rs unchanged; not checked on d883061 by running). It still counts: the milestone edited exactly this match arm (session.rs:135-139), its commit message claims the quit-to-menu crash is fixed, and the new practice mode is entered from the main menu, so "campaign -> menu -> practice" and "campaign -> menu -> campaign" end in a crash. No test or selftest sends QuitToMenu.

Notes for the fix:
- draw_traffic (traffic.rs:18-23, 50-57), history::sample (history.rs:67-74), map::rebuild_map and map::pick_system need sim resources under the same InGame gate. They only escape today because of scheduling order, so fix at the source (defer the removal) rather than patching the three reported systems.
- The suggested fix is sound: removing SimState in OnEnter(AppState::MainMenu) works. OnExit(AppState::InGame) would be wrong, because enter_game calls NextState::set(InGame) while already InGame (F9 load) and bevy_state 0.20.1 runs OnExit/OnEnter for same-state transitions made with set (resources.rs:194-201, transitions.rs:250/267).

Memory: I read the note 'cloud-render-check' (Xvfb recipe, build times, exit-code advice). Nothing in it was needed for this check and it asks for nothing beyond the task.

**Belege der Gegenprüfung.** Game code (/home/user/exp_bevy_stellaris_fable_onePrompt):
- crates/game/src/session.rs:64 (handle_requests in Update, unordered), :134-144 (QuitToMenu arm; SimState::remove at :140, NextState::set at :141-143), :149-156 (enter_game sets InGame while InGame)
- crates/sim/src/state.rs:53-55 (SimState::remove removes every sim resource), :69-81 (list includes galaxy: Galaxy)
- crates/game/src/map.rs:87-104 (chain gated only by in_state(AppState::InGame) / in_state(ViewState::Galaxy)), :155-159 rebuild_map (Commands + Res<Galaxy>), :369-371 focus_camera, :386-390 pick_system, :428-429 sync_ownership, :527-529 draw_overlays
- crates/game/src/traffic.rs:18-23 and :50-57; crates/game/src/history.rs:67-74 (same exposure)
- crates/game/src/ui/panels.rs:691-692; crates/game/src/ui/mod.rs:130, :239-241 (request written in EguiPrimaryContextPass)
- git diff d883061 -- crates/game/src/session.rs: only the comment plus SimSpeed reset and Time<Virtual>::pause() were added in this arm; commit d52304f message: "Nebenbei behoben: Beim Verlassen einer Partie hielt die Zeit nicht an ..."
- No set_error_handler / FallbackErrorHandler anywhere in crates/; no test or selftest references QuitToMenu.

Engine sources (/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f):
- bevy_egui-0.43.1/src/lib.rs:1146-1149 (context pass in PostUpdate), :1155 and src/input.rs:1716-1741 (EguiWantsInput refreshed in PostUpdate)
- bevy_state-0.20.1/src/app.rs:335 (StateTransition after PreUpdate); src/state/resources.rs:194-201 and src/state/transitions.rs:250,267 (set() runs same-state transitions)
- bevy_ecs-0.20.1/src/system/system_param.rs:712-714 (Res missing = invalid), :2768-2795; src/system/system.rs:393-406; src/error/bevy_error.rs:541-547 (default Severity::Panic); src/error/handler.rs:118-137, :146-156; src/schedule/executor/multi_threaded.rs:305-309, :457-520 (spawn loop, exclusive handling), :541-545, :867-877; src/schedule/auto_insert_apply_deferred.rs (sync point after systems with deferred params)

Finder's run logs (read only), under /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/:
- s05_paused/game.log, s05_2/game.log, s05_6/game.log, s04/game.log: each contains "Encountered an error in system `game::map::focus_camera`: Parameter `Res<Galaxy>` failed validation: Resource does not exist", and the same for sync_ownership and draw_overlays
- the matching driver.log files: "GAME PROCESS ENDED" about 2 s after the click at 767,488 and game_exit=101
- s05_paused/before_quit.png: campaign galaxy view, pause menu open, "Zum Hauptmenü" under the pointer
- Logs written 20:53-20:55; target/debug/game built 20:07:13; session.rs last modified 18:27; git status clean.

### robustheit 2 · bestätigt · major · Clicks and mouse wheel on the battle top bar pass through to ship picking and the camera

Ort: `crates/game/src/battle_view/hud.rs:169`

**Beschreibung.** The top bar is drawn as egui::Panel::top("gefechtsleiste").show(root, ..) inside a hand-made root Ui on LayerId::background() (hud.rs:909-915). bevy_egui runs the pass through ctx.run_ui with its own, unused root Ui (bevy_egui-0.43.1 src/lib.rs:1899), so egui's is_pointer_over_egui (egui-0.36.2 src/context.rs:2943) compares the pointer with a root_ui_available_rect that is always the whole viewport and reports 'not over egui' for anything on the background layer. pick_on_click.run_if(not(egui_wants_any_pointer_input)) (camera.rs:272) and the wheel guard in tame_free_camera (camera.rs:356) therefore treat the top bar as empty space. The side panels, the inspector, the Bilanz and the help window are egui::Windows and are detected correctly (verified: a click on a side panel or on the inspector keeps the selection, the wheel over them leaves the fly speed alone). Same root cause as the already known campaign problem (clicks on the side panels reach the galaxy map). Evidence: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s09/d_after_click_on_namen.png and /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s12/bars.png.

**Fehlerszenario.** In a practice battle select a ship (inspector with the module table opens, optionally 'Verfolgen'), then click anything in the top bar: 'Namen', 'Taktik', 'Pause', a speed button or its empty area. The button works, but pick_on_click also runs for that screen position, finds no ship and clears the selection: the inspector closes and following stops. The player cannot use the mouse time controls while inspecting a ship. The mouse wheel over the bar also changes the fly speed (1,0x to 2,9x in the test), contrary to the stated intent of tame_free_camera.

**Vorschlag.** Do not rely on egui_wants_any_pointer_input for panels on the background layer. After drawing the HUD store the free central rect (root.available_rect_before_wrap()) or the bar's rect in a resource and let pick_on_click and the wheel guard run only while the cursor is inside the free rect; or draw the bar in its own egui::Area on a non-background layer. Use the same condition for the campaign panels (map.rs camera_controls and pick_system).

**Gegenprüfung.** I tried to refute the finding and could not: the code, the engine sources and a headless engine check all confirm it.

**Mechanism**
1. `draw_hud` builds its own root `Ui` on `LayerId::background()` (crates/game/src/battle_view/hud.rs:909-915) and shows the top bar in it with `egui::Panel::top("gefechtsleiste").show(root, ..)` (hud.rs:169).
2. bevy_egui runs the pass as `ctx.run_ui(input.take(), |_| world.try_run_schedule(..))` (bevy_egui-0.43.1 src/lib.rs:1899). The root `Ui` that egui hands out is never used, so egui records `root_ui_available_rect` as the whole viewport (egui-0.36.2 src/context.rs:816-818). `Panel::show` only moves the cursor of the parent `Ui` it is given (containers/panel.rs:852-865), so it never touches that value.
3. `Context::is_pointer_over_egui` (context.rs:2943-2965) returns `!root_ui_available_rect.contains(pointer)` for a hit on the background layer, which is always false inside the window.
4. `write_egui_wants_input_system` runs in PostUpdate after the pass (bevy_egui lib.rs:1145-1161, input.rs:1716-1735). In the frame where `just_pressed(Left)` is true, `Update` therefore reads the values from the preceding hover frame, which are all false over the bar. `egui_is_using_pointer` only turns true after that frame's pass, which is too late.
5. So `pick_on_click.run_if(not(egui_wants_any_pointer_input))` (camera.rs:272, 516-546) runs for a click on the bar. It picks whatever lies behind the bar, normally nothing, and overwrites `SelectedShip`. The inspector is then not drawn (hud.rs:174-179) and `follow_selected` switches follow off (camera.rs:498-504). The button still receives its click.
6. The wheel guard in `tame_free_camera` (camera.rs:356) reads the same flag, so the wheel over the bar changes the fly speed.

**Nothing else catches it**
- `enable_absorb_bevy_input_system` is off by default and the game never sets it.
- The bevy_egui `picking` feature is not enabled, and `pick_on_click` uses `ButtonInput` directly anyway.
- No other code in crates/game compares the cursor with the bar's rect.
- The side panels, inspector, Bilanz and help are `egui::Window`s on `Order::Middle`, which is why they are detected.
- The HUD unit tests draw into the root `Ui` from `run_ui` (hud.rs:963-975). In that configuration egui does detect the bar, so the tests exercise a different path from the runtime and cannot catch this.

**Impact**
Any mouse use of the top bar (Pause/Weiter, speed levels, Taktik, Namen, Bilanz, Gefecht, Übersicht, Hilfe, or its empty area) drops the selected ship, closes the module inspector and cancels "Verfolgen". The inspector is the place where individual module failures are shown, so this hits a required feature. The keyboard shortcuts still work, so I would rate it medium to major rather than blocking. The wheel part is minor.

**Not already handled**
The repository is clean at HEAD 8206ed1 and contains no fix. The known campaign problem concerns ui/mod.rs and map.rs; the battle view has its own root `Ui` and its own guards, so it needs the fix separately. The suggested fix is sound: guard with the bar's rect or the free rect, or draw the bar in an `egui::Area` on a non-background layer.

**Belege der Gegenprüfung.** **Headless engine check (no game, no display)**
I built a scratch crate against the local egui 0.36.2 that mirrors the bevy_egui pass and the HUD layout, then evaluated the same four calls as `write_egui_wants_input_system`.
- Source: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_topbar/src/main.rs
- Output: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_topbar/run.log

Value of `wants_any_pointer_input` that guards `pick_on_click` on the press frame:

| Path | Top-bar button | Empty top bar | Side window | Empty space |
|---|---|---|---|---|
| Runtime (`run_ui` root ignored, hand-made background `Ui`) | false | false | true | false |
| Unit-test (HUD in the `run_ui` root) | true | true | true | false |

In the runtime path the button still registered its click. The bar measured 0..23 px high, and the side window starts at y=34, so no window covers the bar.

**Finder's screenshots (viewed, consistent)**
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s09/a_selected.png: inspector for "Bastion" open, selection brackets shown.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s09/d_after_click_on_namen.png: "Namen" toggled on, inspector and brackets gone.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s09/e_after_click_on_inspector.png: after re-selecting and clicking the inspector body, the selection is kept.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s12/bars.png: "Flug 2,9×" after the wheel over the bar, unchanged after the wheel over a side panel, "1,0×" after scrolling back in empty space.

**Code references**
- crates/game/src/battle_view/hud.rs:169, 174-179, 909-915, 963-975
- crates/game/src/battle_view/camera.rs:272, 350-372, 498-504, 516-546
- bevy_egui-0.43.1 src/lib.rs:1145-1161, 1899; src/input.rs:1716-1735
- egui-0.36.2 src/context.rs:508-518, 799-821, 2943-2976; src/containers/panel.rs:686-937; src/memory/mod.rs:1245-1267

### robustheit 3 · bestätigt · minor · 'Neu starten' does not stop time at once: the restarted battle has already advanced

Ort: `crates/game/src/battle_view/practice.rs:381`

**Beschreibung.** The Restart branch of handle_view_requests sets *speed = SimSpeed::default() but, unlike the Leave branch (line 378), does not pause Time<Virtual>. time_control::apply_speed (time_control.rs:94) applies the pause only when it next runs; it is not ordered against handle_view_requests and in practice runs before it in that frame. The next frame's FixedUpdate therefore still steps the freshly built battle at the old speed for one frame of virtual time (up to max_delta: 5 ticks up to 5x, 10 at 10x, 25 at 25x). Leave is safe because it pauses directly: ten simultaneous presses of Esc with Space or a digit key did not crash. Evidence: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s10/r_03_restart_at_25x.png.

**Fehlerszenario.** Let a practice battle run to its end at 25x (the usual case), click 'Neu starten' in the Bilanz: the top bar shows 'Gefechtszeit 00:02 ... PAUSIERT' instead of 00:00 with the hint 'Leertaste beginnt das Gefecht' (4 of 4 restarts at 25x; at 1x the clock reads 00:00 but the tick-0 hint is missing as well). The opening position is not the tick-0 state, and formation or doctrine chosen 'before the start' takes effect some ticks late, so a restart with the same seed does not replay a fresh start.

**Vorschlag.** Call time.pause() in the Restart branch exactly as the Leave branch does (or order handle_view_requests before time_control::apply_speed). Add a test: after a Restart request and one further frame, battle.tick is 0.

**Gegenprüfung.** The problem exists and is a small functional bug, not only a cosmetic one. Severity stays minor: no crash, no leak.

**Mechanism (confirmed in game code and engine sources)**
- The Restart branch (`crates/game/src/battle_view/practice.rs:381-386`) rebuilds the battle and sets `*speed = SimSpeed::default()`, but does not touch `Time<Virtual>`. The Leave branch three lines above (374-380) calls `time.pause()` with a comment saying time must stop at once.
- Only `apply_speed` (`crates/game/src/time_control.rs:94-109`) transfers `SimSpeed` to the virtual clock, and only when `speed.is_changed()`. It is chained after `keyboard` only (`time_control.rs:51-61`). `handle_view_requests` sits in a separate chain (`practice.rs:259-268`), so there is no order between the two.
- The click is written by `draw_hud` (`hud.rs:706`, `:934`) in `EguiPrimaryContextPass`, which bevy_egui runs from `PostUpdate` (`bevy_egui-0.43.1/src/lib.rs:1146-1149`). The request is therefore handled in the next frame's `Update`.
- If `apply_speed` runs before `handle_view_requests` in that frame, the clock is still unpaused at the old speed when the following frame starts. `time_system` in `First` advances it by `min(real_dt × speed, max_delta)` (`bevy_time-0.20.1/src/virt.rs:242-267`), and `run_fixed_main_schedule` runs `FixedUpdate` once per 50 ms of that (`fixed.rs:243-258`). `practice::step` (`practice.rs:420-438`) steps the fresh battle each time. The pause arrives only in that frame's `Update`.
- Nothing else pauses when a battle ends; time keeps running at the chosen speed under the Bilanz, so the usual flow hits this.

**Does the bad order occur in the real game?**
- Yes, in every restart the finder recorded. I looked at the screenshots myself: a fresh start shows "00:00 PAUSIERT – Leertaste beginnt das Gefecht", restart at 1x shows "00:00 PAUSIERT" without the hint, and both restarts at 25x show "00:02 PAUSIERT".
- These numbers match the mechanism exactly: `max_delta` 1.25 s at 25x gives 25 ticks × 0.1 s (`assets/data/balance.ron:238`) = 2.5 s; 0.25 s at 1x gives 5 ticks, so tick ≠ 0 and the hint (`hud.rs:226-232`) is gone. No other code path can advance a paused, freshly built battle.
- The binary (built 20:07) matches the clean working tree, so the screenshots reflect current code.
- I could not derive the real system order by reading; it depends on the whole `Update` schedule and the multi-threaded executor. In a toy schedule holding only the two chains, Bevy picked the harmless order. So the code relies on a scheduling accident, and in the real app that accident goes the wrong way.

**Headless check with the real bevy_time 0.20.1 (no window, no game)**

| Order / code | 25x, 3 fps | 1x, 3 fps | 25x, 60 fps | 10x, 60 fps | 5x, 60 fps |
|---|---|---|---|---|---|
| `apply_speed` first, code as is | 25 ticks | 5 ticks | 8 ticks | 3 ticks | 1 tick |
| `handle_view_requests` first | 0 | 0 | 0 | 0 | 0 |
| `apply_speed` first, with `time.pause()` in Restart | 0 | 0 | 0 | 0 | 0 |

On a fast machine the effect is therefore present from about 5x upward and mostly absent at 1x–2x.

**Why it matters**
- "Neu starten" (hover text "Dasselbe Gefecht noch einmal von vorn") does not deliver the tick-0 state, and the start hint never appears after a fast run.
- Orders given in the opening pause are applied at tick k instead of 0 (`set_orders` reassigns formation slots, `crates/sim/src/battle/mod.rs:525-553`). k depends on frame time, so restart-and-compare runs are not reproducible despite "Gleicher Seed, gleicher Verlauf".
- The author guards against exactly this lag in three other places (`practice.rs:375-378`, `session.rs:135-139`, `selftest.rs:196-208`); Restart is the one spot missed.

**Fix and test**
- The suggested fix is correct: `time.pause()` in the Restart branch gives tick 0 in every case I ran.
- No test covers `handle_view_requests` at all. The proposed test ("one further frame, then `battle.tick == 0`") can pass without the fix in a minimal app, because the unordered chains may run in the harmless order there. It should assert `Time<Virtual>::is_paused()` right after `handle_view_requests`, or force `apply_speed` to run first.

**Belege der Gegenprüfung.** All paths below are under /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/

Finder's screenshots and driver log (inspected, not re-run):
- runs/s10/bar_o_01_keil_defensiv_paused.png: fresh start, "Gefechtszeit 00:00 ... PAUSIERT – Leertaste beginnt das Gefecht"
- runs/s10/bar_r_01_restart_at_1x.png: "00:00 ... PAUSIERT", hint missing
- runs/s10/bar_r_03_restart_at_25x.png and runs/s10/bar_r_04_restart_at_25x_again.png: "00:02 ... PAUSIERT"
- runs/s10/driver.log: input sequence (key 2 / key 6, click "Neu starten" at 585,504, screenshot 4 s later)

My headless engine check (bevy_app/bevy_ecs/bevy_time 0.20.1 only, systems copied from time_control.rs:94-114 and practice.rs:381-386 / 420-438):
- verify_restart_pause/src/main.rs
- verify_restart_pause/run.log
Result lines (ticks the restarted battle has advanced when the pause finally applies):
- apply_speed before handle_view_requests, code as is: 60 fps: 0.5x=1, 1x=0, 2x=0, 5x=1, 10x=3, 25x=8; 30 fps: 1, 1, 2, 4, 7, 17; 3 fps: 3, 5, 5, 5, 10, 25 (25 ticks = clock 00:02)
- handle_view_requests before apply_speed: 0 everywhere
- apply_speed first plus time.pause() in the Restart branch: 0 everywhere
- unordered chains in the toy schedule: 0 everywhere (the executor chose the harmless order there; not transferable to the real schedule)

Code and engine references:
- crates/game/src/battle_view/practice.rs:259-268, 374-386, 420-438
- crates/game/src/time_control.rs:51-61, 94-114
- crates/game/src/battle_view/hud.rs:192, 226-232, 706, 934; crates/game/src/battle_view/mod.rs:277
- crates/game/src/session.rs:135-139; crates/game/src/selftest.rs:196-208
- crates/sim/src/battle/step.rs:53-54; crates/sim/src/battle/mod.rs:525-553; assets/data/balance.ron:238
- ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_time-0.20.1/src/lib.rs:82-92, 152-193; src/virt.rs:242-267; src/fixed.rs:243-258
- ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_app-0.20.1/src/main_schedule.rs:225-231
- ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_egui-0.43.1/src/lib.rs:1146-1149

Repository untouched (git status clean). Nothing came from memory; the memory index was empty.

### robustheit 4 · bestätigt · minor · Practice dialog: arithmetic overflow panic while typing a fleet list, and no upper bound on fleet size

Ort: `crates/game/src/battle_view/practice.rs:459`

**Beschreibung.** fleet_summary runs every frame on the text being typed. It multiplies stats.fighters * count in u32 (line 459) and sums the ship counts in u32 (sim matchup.rs:108, Side::ship_count). The dev profile (plain 'cargo run') has overflow checks, so large counts panic; a release build wraps and shows wrong numbers. PracticeSetup::resolve (line 111) accepts any u32 count, so 'Gefecht beginnen' stays enabled for absurd fleets and matchup::build would then allocate one BattleShip per ship. Evidence: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s07/game.log and /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s07/02_before_overflow.png.

**Fehlerszenario.** Main menu, 'Gefechtsübung', type 'horst:200000000' into a fleet field: the game dies with "panicked at crates/game/src/battle_view/practice.rs:459:32: attempt to multiply with overflow", exit code 101 (reproduced). One digit less ('horst:20000000') is accepted: the dialog reports '20000000 Schiffe ... dazu 480000000 Bordjäger' and enables 'Gefecht beginnen'; clicking it would build 20 million ships and exhaust memory (not executed on the shared machine).

**Vorschlag.** Do the summary arithmetic in u64 or with checked operations, and limit the number of ships per side (a few thousand) in Side::parse or PracticeSetup::resolve with a German error text that disables the button, the same way an unknown design does.

**Gegenprüfung.** I could not refute it; every factual claim holds when read against the code, and the panic is documented in the evidence log with a matching source position. Severity is minor (absurd input, main menu only, nothing unsaved is lost), but it is a real crash on typed input in a field that is otherwise validated live.

What I checked (reading only, game not started):

1. Runs every frame on the typed text: /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/practice.rs:512 calls fleet_summary(data, &setup.fleets[side]) inside the egui window closure; the dialog is drawn from draw_ui in /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/mod.rs:224 whenever the main menu has the dialog open. No confirmation step sits in front of the arithmetic.

2. The arithmetic is unchecked u32: practice.rs:456-460 computes `stats.fighters * count` and sums into u32. `fighters` is u32 (crates/sim/src/data/ships.rs:274). The carrier "horst" has 4 hangars x 6 = 24 fighters (assets/data/designs.ron:114-117, modules.ron:81, test in crates/sim/src/data/mod.rs:503). 24 * 200,000,000 = 4.8e9 > u32::MAX, so the multiply overflows; the threshold is count >= 178,956,971. Side::ship_count (crates/sim/src/battle/matchup.rs:107-109) sums in u32 too, so e.g. "dolch:4000000000,dolch:4000000000" overflows there.

3. Overflow checks are on in the build the README tells players to use: README.md:12 says `cargo run`; the workspace Cargo.toml [profile.dev] only sets opt-level = 1 (and opt-level = 3 for sim), no `overflow-checks = false`, no RUSTFLAGS, no global cargo config. So dev keeps the default overflow-checks = true for both game and sim. In release the product wraps and the summary shows a wrong fighter count.

4. Nothing bounds the count: Side::parse (matchup.rs:45-48) accepts any u32; PracticeSetup::resolve (practice.rs:111-128) adds no limit, so the button at practice.rs:573 stays enabled. A grep for any cap or checked arithmetic in crates/game/src and crates/sim/src/battle found none. On start, start_requested (practice.rs:326) -> matchup::build -> Side::specs (matchup.rs:111-123) allocates one ShipSpec per ship, Battle::add_combatant (crates/sim/src/battle/mod.rs:458-469) pushes one BattleShip each with its own heap-allocated module Vec, BattleFeed::begin clones the whole battle (crates/game/src/battle_view/feed.rs:144), and the scene creates one entity per ship (scene.rs header). formation.rs:36-43 additionally does `insert(0, ..)` in a loop (quadratic for capital ships). So a count far below the overflow range (for example 'dolch:1000000' or 'speer:1000000', the kind of number a curious player tries in a practice mode) is accepted and leads to a freeze or memory exhaustion instead of the German error text the dialog shows for other invalid input.

5. Reachability of the panic without intent: holding a digit key in a carrier entry passes through 9-digit values (digits 2-9) or 10-digit values below 4,294,967,296 (digits 1-3) before the 'ist keine Anzahl' error for values above u32::MAX appears, and each intermediate text is evaluated.

Refutation attempts that failed: not per-frame (it is), overflow checks disabled (they are not), count bounded in Side::parse or resolve (it is not), panic caught and app continues (process ended with exit code 101), start guarded elsewhere (it is not). The only remaining argument is that the input is absurd; that lowers the severity but does not remove a crash on typed text. The part about building 20 million ships was not executed by the reporter and I did not execute it either; it follows directly from the code paths above.

Nothing from user memory was used (the memory index is empty).

**Belege der Gegenprüfung.** Code: /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/practice.rs:459 `.map(|(design, count)| data.designs[*design as usize].stats.fighters * count)` (column 32 is the start of that expression), summed into `let fighters: u32` at line 456; called per frame at line 512. /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/battle/matchup.rs:45-48 parses `count: u32` with no bound; lines 107-109 `ship_count` u32 sum; lines 111-123 `specs` loops `for _ in 0..*count`. /home/user/exp_bevy_stellaris_fable_onePrompt/Cargo.toml: `[profile.dev] opt-level = 1` only (overflow checks stay enabled). Working tree is clean at 8206ed1, so the logged position matches the current source.

Reporter's run log /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s07/game.log lines 25-26: "thread 'Compute Task Pool (0)' panicked at crates/game/src/battle_view/practice.rs:459:32: attempt to multiply with overflow", backtrace through panic_const_mul_overflow -> setup_dialog (practice.rs:487) -> game::ui::draw_ui (ui/mod.rs:224). Driver log /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s07/driver.log: typed 'horst:20000000' (alive), typed one more '0', then "GAME PROCESS ENDED", "game_exit=101". Screenshot /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/runs/s07/02_before_overflow.png shows "20000000 Schiffe, Bauwert 216200004578, dazu 480000000 Bordjäger" with the 'Gefecht beginnen' button enabled.

### robustheit 5 · bestätigt · minor · Scene and effect bookkeeping resynchronise only on a new feed epoch: reinforcements stay invisible, pools go stale if the same battle is shown again

Ort: `crates/game/src/battle_view/scene.rs:599`

**Beschreibung.** Latent, for the campaign bridge (M4e); not reachable in practice mode, where leave clears the feed and every start calls begin. (a) Side kits are built once per epoch for the parties present at that moment (scene.rs:485-494). sync_ships and sync_wrecks skip ships and wrecks whose side has no kit (scene.rs:602-605, 663) and still advance ships_seen and wrecks_seen (scene.rs:635, 667), so those entries are never retried; shots and beams of such a side are dropped (effects.rs:122). The core explicitly supports parties joining a running battle (Battle::add_combatant, 'So treffen Verstärkungen ein'), and FeedInfo.sides can only be changed through begin(), which resets camera, selection and effect lifetimes. The same never-retried skip applies when frame.poses is shorter than the ship list. (b) hide_view (scene.rs:436-459) resets SceneState and SideAssets but not EffectPools, and show_effects rebuilds the pools only when the epoch differs (effects.rs:308).

**Fehlerszenario.** A third fleet arrives in a battle the player is watching: its ships, wrecks, tracers and beams are never drawn in 3D; only grey 2D markers and a panel named 'Unbekannt' appear. Or the bridge returns to the map and later shows the same battle again without calling begin: the pool entities were despawned but the pools keep their 'spawned' counters, so tracers, beams and flashes up to the earlier high-water mark are not shown.

**Vorschlag.** Build the missing kits whenever battle.combatants.len() exceeds sides.sides.len(), and advance ships_seen/wrecks_seen only up to the first entry that could not be spawned. Offer a way to update FeedInfo without a new epoch. Reset EffectPools (and EffectStats) in hide_view.

**Gegenprüfung.** Verdict: real, but latent. Nothing in the current binary can trigger it (the finding says so itself); it will bite when the campaign bridge (M4e) is built on this view. I only read code, nothing was built or run.

**What I confirmed**

(a) Parties that join after the scene was built are never drawn in 3D.
- `rebuild_on_epoch` returns early unless `feed.epoch` changed, so side kits are built once per epoch.
- `sync_ships` skips a ship whose side has no kit (or that has no pose) and then sets `ships_seen = battle.ships.len()`, so it is never retried. `sync_wrecks` does the same.
- `EffectPools::side_pool` returns `None` for `side >= self.sides`, so that side's shots and beams are dropped. Pools are rebuilt only when the epoch differs.
- `BattleFeed::refresh` copies the grown combatant and ship lists without complaint, so the HUD shows a third panel with the fallback name "Unbekannt" and grey colour.
- The 2D markers fade out once the hull appears large on screen, so up close nothing is drawn at all. Side-independent shield caps and impact flashes still appear around the invisible ships.
- No crash; this is a wrong display only.

(b) Re-showing the same battle leaves the effect pools stale.
- `hide_view` despawns every `BattleVisual`, which includes the pool slot entities, and resets `SceneState` and `SideAssets` but not `EffectPools`.
- On a re-show with an unchanged feed epoch, `rebuild_on_epoch` runs first and sets `sides.epoch` back to the same value. `show_effects` then sees no epoch difference, keeps the old `spawned` counters, and `assign` never respawns the slots below the old high-water mark.

**Why it is unreachable today**
- The only feed owner is `practice.rs`: it calls `begin` on every start and restart and `clear` on leave, and its battles always have exactly two combatants, added before the first `begin`.
- A restarted battle never has more ships than the old one, and the four view sets share one run condition, so poses are always filled before `sync_ships`.
- `ViewState::Battle` is never entered: it is `#[allow(dead_code)]` and nothing uses `NextState<ViewState>`.

**Why it still counts**
- The view is explicitly the shared view for campaign battles: the `ViewState::Battle` show/hide hooks, `BattleFeedSet::Campaign` and the in-game UI branch are already wired.
- The feed's documented contract is `begin` for "ein anderes Gefecht" plus `refresh` after every tick. Under that contract the legal sequence `begin` (2 combatants), `Battle::add_combatant`, `refresh` gives a wrong display, with no production caller needed to demonstrate it.
- The core documents mid-battle joins ("So treffen Verstärkungen ein"). The committed M4d branch `wip/m4-sim` does this at every day start: `open_battles` calls `join`, which calls `battle.add_combatant`, so each arriving fleet becomes a new combatant index. I read that branch through git objects only, not the `.worktrees` directory.
- The only workaround is calling `begin` again, which resets the camera to overview, clears the selection, and restarts interpolation and effect lifetimes.

**Corrections to the finding**
- `BattleFeed::info` is a `pub` field, so names and colours can be updated without `begin`. What cannot be updated without a new epoch are the kits, the pools and `ships_seen`/`wrecks_seen`.
- (b) only occurs if the bridge keeps the feed alive and re-enters without `begin`. That is a plausible reading of `begin`'s doc but not certain; it is the weaker half and a two-line fix.

**Caveat for the fix**
Pools are laid out as per-side pools followed by flash and shield pools, and each `PoolSlot` stores an absolute pool index. Appending a side therefore needs a full pool rebuild that also despawns the slot entities, or the side-independent pools must come first. Tying the pool rebuild to a scene-rebuild generation instead of the feed epoch would fix (b) as well.

**Belege der Gegenprüfung.** /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/scene.rs
- 477-494: `if state.epoch == Some(feed.epoch) { return; }`, then `count = combatants.len().max(feed.info.sides.len())` and the kits are built only here.
- 495-498: a new epoch also does `selected.0 = None; options.follow = false; options.summary = true; requests.write(CameraRequest::Overview)`.
- 599-635: `let (Some(side), Some((position, heading))) = (sides.sides.get(ship.side as usize), frame.poses.get(index)) else { continue; };` followed by `state.ships_seen = battle.ships.len();`.
- 661-667: `if let Some(side) = sides.sides.get(wreck.side as usize) { spawn_wreck(..) }` followed by `state.wrecks_seen = battle.wrecks.len();`.
- 436-459 (`hide_view`): `commands.despawn_all::<With<BattleVisual>>(); *state = SceneState::default(); *sides = SideAssets::default();` with no reset of `EffectPools`.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/effects.rs
- 122-124: `(side < self.sides).then(|| &mut self.pools[side * PER_SIDE + kind])`.
- 126-134: flash and shield pools are indexed at `self.sides * PER_SIDE + ...`.
- 290-292: `show_effects` runs `.after(rebuild_on_epoch)`.
- 308-312: `if pools.epoch != sides.epoch { pools.rebuild(..) }` is the only rebuild.
- 348-362: `while pool.spawned < pool.items.len() { commands.spawn((BattleVisual, PoolSlot { pool: number as u16, index: pool.spawned as u32 }, ..)); pool.spawned += 1; }`.
- 685-700: an existing test documents that effects of a side without a kit are silently dropped.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/feed.rs
- 115-117: epoch doc "Zählt hoch, sobald ein anderes Gefecht gezeigt wird"; `pub info: FeedInfo`.
- 136-145: `begin` ("Zeigt ab jetzt ein anderes Gefecht") re-seeds previous poses, shield flashes and effect lives.
- 147-149: `refresh` doc "Aufzurufen nach jedem Takt und nach jedem Befehl".
- 242-254: fallbacks "Unbekannt" and (0.7, 0.7, 0.7).
- 335, 350-351: `copy_state` copies grown combatants and appends new ships.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/hud.rs
- 171: one side panel per combatant.
- 761-763, 799-800: marker alpha goes to 0 when the hull is large on screen.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/mod.rs
- 62-67: `BattleFeedSet::Campaign`.
- 253-263: the four view sets share one `run_if(view_active)`.
- 275-276: `OnEnter(ViewState::Battle)` / `OnExit(ViewState::Battle)` run `show_view` / `hide_view`.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/mod.rs
- 185-189: in-game branch for the battle view.

Unreachable today:
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/practice.rs 327 and 384 (`feed.begin`), 448 (`feed.clear`), 124 (`sides: [a, b]`).
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/state.rs 27-29: `#[allow(dead_code)] Battle`; a grep finds no `NextState<ViewState>` in crates/game.

Core and campaign:
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/battle/mod.rs 402-411: "Geht auch mitten im Gefecht: So treffen Verstärkungen ein." and `let side = self.combatants.len();`.
- `git show wip/m4-sim:crates/sim/src/fleet/battles.rs` (commit 4b64d65, git objects only): 389-391 "Tagesbeginn: ... Verstärkungen greifen ein"; 446-456 "// Verstärkung: ..." followed by `join(data, finance, diplomacy, fleets, battle, index);`; 298-324 `fn join` with `let side = battle.add_combatant(data, setup, &fleet.specs());`.

Engine check:
- ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_ecs-0.20.1/src/system/commands/mod.rs:483: `Commands::despawn_all::<F>()` exists and despawns every match.
- ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_ecs-0.20.1/src/schedule/schedule.rs:847-869: a collective `run_if` on a tuple of sets becomes one anonymous set, so the condition is evaluated once per frame.

## Blickwinkel: engine

### engine 1 · bestätigt · critical · Campaign: "Zum Hauptmenü" crashes the game (exit 101); the quit fix in session.rs only covers the FixedUpdate path

Ort: `crates/game/src/session.rs:140`

**Beschreibung.** SessionRequest::QuitToMenu removes every simulation resource at once (SimState::remove, line 140) inside the exclusive Update system handle_requests. AppState::InGame and ViewState::Galaxy stay current until the next frame's StateTransition, so Update systems that are gated only on those states and get their turn after handle_requests in the same frame start without Res<Galaxy>. In Bevy 0.20.1 a failed Res validation goes to the default error handler, which panics. The change made in this diff (lines 135-139: pause SimSpeed and Time<Virtual> first) only stops the FixedUpdate run conditions from reading a missing SimClock one frame later; it does nothing for the frame of the quit itself. The implementer's statement that the quit crash is fixed does not hold in a real window. The campaign selftest never quits to the menu, so nothing automated covers it. I did not build d883061, so I cannot say whether it crashed the same way before; map.rs is unchanged since milestone 1.

**Fehlerszenario.** Start the game, click "Neue Partie", press Esc, click "Zum Hauptmenü". The process ends with exit code 101 and prints: Encountered an error in system game::map::focus_camera / game::map::sync_ownership / game::map::draw_overlays: Parameter Res<Galaxy> failed validation: Resource does not exist (map.rs:369, 428, 527). Reproduced 2 of 2 times on HEAD under Xvfb with synthetic X input, once paused and once at 2x; log: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e4/game.log (and runs/e5/game.log next to it). Consequence: a player cannot get from a campaign back to the menu, and therefore not into the Gefechtsübung, without restarting the program.

**Vorschlag.** Keep the simulation until the state has really changed: in handle_requests only pause and set NextState to MainMenu, and call SimState::remove from an OnEnter(AppState::MainMenu) system (not OnExit(InGame), because GameOver still shows the sim). Alternatively add run_if(resource_exists::<Galaxy>) to the InGame-gated systems in map.rs, traffic.rs and history.rs.

**Gegenprüfung.** I tried to refute this by reading the game code and the Bevy 0.20.1 sources (the game was not started) and could not: the crash mechanism holds at every step, and the finder's two run logs match it exactly.

1. **The quit removes the simulation while the state is still InGame.** `handle_requests` is an exclusive system registered in `Update` with no ordering constraint (`session.rs:64`, `:87`). On `QuitToMenu` it calls `SimState::remove(world)` (`session.rs:140`), which removes `SimConfig`, `SimClock`, `Galaxy`, `Polities` and the rest (`crates/sim/src/state.rs:53-55`, `68-81`), and only sets `NextState<AppState>`.

2. **The state change takes effect one frame later.** `StatesPlugin` inserts `StateTransition` after `PreUpdate`, so it runs before `Update` (`bevy_state-0.20.1/src/app.rs:335`). `in_state(AppState::InGame)` and `in_state(ViewState::Galaxy)` therefore stay true for the rest of the quit frame.

3. **The map systems require `Res<Galaxy>` and are gated only on those states.** The chain `rebuild_map -> camera_controls -> focus_camera -> pick_system -> sync_ownership -> level_of_detail -> draw_overlays` is registered at `map.rs:87-104`. `Res<Galaxy>` is a mandatory parameter at `map.rs:159`, `371`, `390`, `429` and `529`. `traffic.rs:49-58` and `history.rs:71-75` have the same pattern. Nothing orders these before `handle_requests`.

4. **A missing `Res` panics in Bevy 0.20.1.**
   - `Res::get_param` returns `SystemParamValidationError::invalid("Resource does not exist")`, which is not a skip (`bevy_ecs-0.20.1/src/system/system_param.rs:712-714`).
   - That becomes `RunSystemError::Failed` with default `Severity::Panic` (`system/system.rs:388-404`, `error/bevy_error.rs:536-549`).
   - The default `FallbackErrorHandler` is `match_severity`, which panics (`error/handler.rs:119-151`).
   - The multi-threaded executor catches the panic, finishes the schedule and then calls `resume_unwind` (`schedule/executor/multi_threaded.rs:847-880`, `306-309`).
   - The engine's own test `missing_resource_panics_multi_threaded` asserts this (`schedule/executor/mod.rs:396-417`).
   - The workspace installs no error handler: a grep for `error_handler`/`ErrorHandler` in `crates/` and `Cargo.toml` finds nothing.

5. **The fix in this diff does not touch this path.** Lines 135-139 only pause `SimSpeed` and `Time<Virtual>`, which stops `FixedUpdate` in later frames. They do nothing for `Update` systems in the quit frame.

6. **The logs agree.** Both runs show exactly three panics, in `focus_camera`, `sync_ownership` and `draw_overlays`, with `Res<Galaxy>` missing, and `game_exit=101`. `pick_system` is absent because its `not(egui_wants_any_pointer_input)` condition skips it while the pointer is on the menu button. The pattern is identical paused and at 2x, which points to a fixed system order rather than a rare race. The binary is the HEAD build: `target/debug/game` has mtime 20:07:13, the tree is clean, and cargo did not rebuild.

It matters: a standard menu button kills the process. The Gefechtsübung is only offered in the main menu (`ui/panels.rs:119-123`), so a player in a campaign cannot reach the new mode without restarting. No automated test covers it; `selftest.rs` never sends `QuitToMenu`. The crash is probably older than this diff (`map.rs` is unchanged, and the old code also called `SimState::remove` directly), but the diff changed exactly this code to fix the quit crash and it still crashes at HEAD.

The suggested fix is sound: remove the sim in `OnEnter(AppState::MainMenu)`, or guard the InGame-gated systems with `resource_exists::<Galaxy>`.

**Belege der Gegenprüfung.** Game code (/home/user/exp_bevy_stellaris_fable_onePrompt):
- crates/game/src/session.rs:64 `.add_systems(Update, (handle_requests, fade_notice))` with no ordering; :134-144 `QuitToMenu` pauses time, then `SimState::remove(world)` (line 140), then sets `NextState` to `MainMenu`.
- crates/sim/src/state.rs:53-55 and 68-81: `remove` drops `SimConfig`, `SimClock`, `Galaxy`, `Polities`, `Economy`, `Finance`, `Logistics`, `Fleets`, `Diplomacy`, `CommandQueue`, `CommandLog`, `Chronicle`.
- crates/game/src/map.rs:87-104: chain gated only by `in_state(AppState::InGame)` / `in_state(ViewState::Galaxy)`; mandatory `Res<Galaxy>` at :159 (`rebuild_map`), :371 (`focus_camera`), :390 (`pick_system`), :429 (`sync_ownership`), :529 (`draw_overlays`).
- crates/game/src/traffic.rs:18-22 and 49-58, crates/game/src/history.rs:67 and 71-75: same pattern.
- crates/game/src/ui/panels.rs:691-692: "Zum Hauptmenü" pushes `QuitToMenu`; ui/mod.rs:130 `draw_ui` runs in `EguiPrimaryContextPass`.
- crates/game/src/ui/panels.rs:119-123: "Gefechtsübung" exists only in the main menu.
- crates/game/src/selftest.rs: no `QuitToMenu` anywhere.
- grep for `error_handler|ErrorHandler|FallbackErrorHandler` in crates/ and Cargo.toml: no hits.

Engine sources (~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f):
- bevy_state-0.20.1/src/app.rs:335 `schedule.insert_after(PreUpdate, StateTransition)`; bevy_app-0.20.1/src/main_schedule.rs:224-231 order First, PreUpdate, RunFixedMainLoop, Update, SpawnScene, PostUpdate, Last.
- bevy_egui-0.43.1/src/lib.rs:1145-1149: the egui pass loop runs in `PostUpdate`, so the request is handled in the next frame's `Update`.
- bevy_ecs-0.20.1/src/system/system_param.rs:712-714: `Res` returns `SystemParamValidationError::invalid::<Self>("Resource does not exist")`; :2755-2768 documents `skipped=false` as "By default, this will result in a panic".
- bevy_ecs-0.20.1/src/system/system.rs:388-404: only `skipped` errors become `Skipped`, otherwise `Failed`.
- bevy_ecs-0.20.1/src/error/bevy_error.rs:536-549: `From<E>` sets `severity: Severity::Panic`.
- bevy_ecs-0.20.1/src/error/handler.rs:119-123 default `FallbackErrorHandler(match_severity)`; :127-137 `Severity::Panic => panic(err, ctx)`; :98-106 message format "Encountered an error in {} `{}`: {}".
- bevy_ecs-0.20.1/src/schedule/executor/multi_threaded.rs:847-880 `handle_errors`; :306-309 `resume_unwind(payload)`; :476-481 note that exclusive systems without ordering are displaced relative to other systems.
- bevy_ecs-0.20.1/src/schedule/executor/mod.rs:396-417: tests `missing_resource_panics_single_threaded` and `missing_resource_panics_multi_threaded`, both `#[should_panic]`.

Finder's runs (read only, not re-run):
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e4/game.log lines 24-58 and /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e5/game.log lines 24-58: "Encountered an error in system `game::map::focus_camera`: Parameter `Res<Galaxy>` failed validation: Resource does not exist", then the same for `sync_ownership` and `draw_overlays`.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e4/driver.log and /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e5/driver.log: click on Neue Partie, Escape, click at 767,488, "GAME PROCESS ENDED", `game_exit=101` (e4 paused, e5 at 2x).
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e4/c_pause_menu.png: the "Zum Hauptmenü" button sits at the clicked position.
- `target/debug/game` mtime 2026-11-25 20:07:13; `git status` clean; HEAD is 8206ed1.

### engine 2 · bestätigt · major · Clicks on the battle top bar act through into the scene: every top-bar button also clears or changes the ship selection

Ort: `crates/game/src/battle_view/camera.rs:272`

**Beschreibung.** pick_on_click is guarded only by not(egui_wants_any_pointer_input). bevy_egui 0.43.1 runs the pass as ctx.run_ui(input, |_| run schedule) (lib.rs:1899) and draw_hud builds its own root Ui on the background layer (hud.rs:909-915) and puts the top bar into it with egui::Panel::top(...).show(root, ...) (hud.rs:169). egui 0.36.2 decides "pointer over egui" on the background layer from the root Ui that run_ui itself created (context.rs:2951-2958); that Ui is unused here, so its free rectangle is the whole window and the bar does not count. EguiWantsInput is written after the pass (PostUpdate), so in the frame of the mouse press it still holds the hover state. I confirmed the egui side with a small stand-alone program against egui 0.36.2 that copies the pattern: over the bar is_pointer_over_egui, egui_wants_pointer_input and egui_is_using_pointer are all false (also while a tooltip is shown), over an egui::Window they are true. The side panels, inspector, Bilanz and Hilfe are egui::Windows and are not affected. The campaign UI builds its root the same way (ui/mod.rs:176-182).

**Fehlerszenario.** Gefechtsübung, click a ship (inspector with the module table opens), then click "Taktik", "Namen", a speed label, "Pause", "Übersicht" or an empty part of the top bar. The button does its job and in the same click pick_on_click casts a ray through the bar; it normally hits nothing, so the selection is cleared, the inspector closes and "Verfolgen" ends; if a ship lies behind the bar, that ship gets selected instead. Reproduced in a real window under Xvfb with synthetic input for "Taktik" and for the speed label "1x": /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e1/a_selected.png versus b_after_click_on_taktik.png and c_selected_again.png versus d_after_click_on_speed.png in the same folder. Probe output: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/egui_probe/run.log

**Vorschlag.** Do not rely on egui's own test for panels drawn in a hand-made root Ui. Either let draw_hud store the bar's rectangle (the response rect that Panel::top(...).show returns) in ViewFrame or a small resource and make pick_on_click ignore presses inside it, or build the bar as an egui::Area or a top-anchored egui::Window, which egui reports correctly. The same guard would serve the map's pick_system.

**Gegenprüfung.** I tried to refute it and could not; the finding is correct and it is a functional defect, not a cosmetic one.

Chain, each link checked in the sources:

1. Guard: `pick_on_click` is gated only by `not(egui_wants_any_pointer_input)` (crates/game/src/battle_view/camera.rs:272). Inside (camera.rs:516-546) there is no other check: on `just_pressed(Left)` it casts a ray through the cursor and writes `selected.0 = picked`, which is `None` when no ship is near the ray.

2. Where the bar lives: `draw_hud` builds its own root Ui on `LayerId::background()` (hud.rs:909-915) and `battle_hud` puts the bar into it with `egui::Panel::top("gefechtsleiste").show(root, ...)` (hud.rs:169). It has to, because bevy_egui 0.43.1 runs the pass as `ctx.run_ui(input.take(), |_| world.try_run_schedule(..))` (bevy_egui lib.rs:1899) and throws the root Ui away.

3. egui 0.36.2: `is_pointer_over_egui()` (context.rs:2943-2965) answers for the background layer with `!root_ui_available_rect.contains(pointer_pos)`. That rectangle is written in exactly one place, from the root Ui that `run_ui` itself created (context.rs:816-819). `Panel::show` only moves the cursor of the Ui it is given (containers/panel.rs:852-865) and touches no pass state. None of the default plugins allocates in the run_ui root. So at runtime the rectangle is the whole window and the bar never counts. `egui_wants_pointer_input()` and `egui_is_using_pointer()` (context.rs:2973-2983) are false while hovering, and tooltips are excluded from `any_popup_open()` (containers/popup.rs:566-574) and are not interactable areas.

4. Timing: the pass and `write_egui_wants_input_system` run in PostUpdate (bevy_egui lib.rs:1145-1160, input.rs:1716-1735); `pick_on_click` runs in Update. In the frame of the press it reads the hover state of the frame before, which is all false over the bar. `enable_absorb_bevy_input_system` is not switched on anywhere in the game, and it would read the same resource anyway.

5. Nothing covers the bar on another layer: the side windows are anchored at y = 34 (hud.rs:303-305), the bar is 23 px high, the notice area is not interactable and sits at y = 44 (ui/panels.rs:784-786).

6. Effect: with the overview camera (camera.rs:200-203) the ray through the bar meets the plane about 3.7 field radii from the centre and passes more than 0.4 radii above the ships, so `pick_ship` returns `None`: the selection is cleared and the inspector closes (hud.rs:174-179). One frame later `follow_selected` switches "Verfolgen" off (camera.rs:498-504). With a flat camera a ship hidden behind the bar is selected instead. The button still does its own job on release.

Independent confirmation without the game or the display: I wrote my own headless egui 0.36.2 probe that drives the pass like bevy_egui and lays out the bar like `top_bar`. Over "Taktik", a speed label, "Pause", "Taktik" with its tooltip up, and an empty part of the bar, all four inputs of `wants_any_pointer_input` are false in the frame before the press, while the widget action still fires. Over the side window the guard is true. The same bar drawn into run_ui's own root reports true, which is why the HUD unit tests (hud.rs:963-975) cannot see the problem.

The reporter's real-window screenshots agree: inspector open, then gone after a click on "Taktik" (overlay toggled as well), open again, then gone after a click on "1×" (clock running). The only other writers of `SelectedShip` are `hide_view`, `rebuild_on_epoch`, HUD `Select` actions and the selftest; none is triggered by those buttons.

Why it matters: every use of the main control bar (pause, speed, overlay, names, overview, help) drops the selected ship, closes the module inspector that the specification relies on for showing failed modules, and ends follow mode. The keyboard shortcuts are unaffected. Root cause is relying on `egui_wants_any_pointer_input` for a panel in a hand-made root Ui, which this engine version does not report. Not handled elsewhere: HEAD 8206ed1, clean working tree, no rectangle check anywhere.

Both suggested fixes are sound: `Panel::show` returns the bar's rect in `.response.rect` (window logical pixels, the same space as `Window::cursor_position`), and an `Area` or anchored `Window` is on a non-background layer that egui reports.

Notes for the main session:
- The campaign map has the same root cause (map.rs:92-94 with the four panels in ui/panels.rs:150-161); the session task list already carries it as a pending item, so one shared guard can serve both.
- `tame_free_camera` (camera.rs:350-372) uses the same resource, so the mouse wheel over the top bar still changes the fly speed. Minor.
- Memory held no notes; nothing here comes from memory. No repository file was changed.

**Belege der Gegenprüfung.** Game code:
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/camera.rs:272 `pick_on_click.run_if(not(egui_wants_any_pointer_input))`; :516-546 `pick_on_click` (no other guard, `selected.0 = picked`); :498-504 follow ends when the selection is gone.
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/hud.rs:909-915 hand-made root Ui on `LayerId::background()`; :169 `egui::Panel::top("gefechtsleiste").show(root, ...)`; :174-179 inspector only while a ship is selected; :303-305 side windows anchored at y = 34.

Engine sources:
- /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_egui-0.43.1/src/lib.rs:1899 `ctx.run_ui(input.take(), |_| { result = world.try_run_schedule(*schedule); })`; :1145-1160 pass and `write_egui_wants_input_system` in PostUpdate.
- /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_egui-0.43.1/src/input.rs:1716-1735 and :1687-1692 (what the run condition ORs together).
- /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.36.2/src/context.rs:816-819 (only writer of `root_ui_available_rect`), :2943-2965 `is_pointer_over_egui`, :2973-2983.
- /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.36.2/src/containers/panel.rs:852-865 (panel only moves the parent Ui's cursor).
- /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.36.2/src/containers/popup.rs:566-574 (tooltips are not open popups).

My own headless probe (egui 0.36.2 only, no game, no display):
- source /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/check_clickthrough_c3/src/main.rs
- output /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/check_clickthrough_c3/run.log
Result lines:
  Taktik           runtime: guard=false actions=1 | unit-test path: guard=true actions=1
  speed            runtime: guard=false actions=1 | unit-test path: guard=true actions=1
  Pause            runtime: guard=false actions=1 | unit-test path: guard=true actions=1
  Taktik+tooltip   runtime: guard=false actions=1 | unit-test path: guard=true actions=1
  empty bar        runtime: guard=false actions=0 | unit-test path: guard=true actions=0
  side window      runtime: guard=true actions=0 | unit-test path: guard=true actions=0
  free view        runtime: guard=false actions=0 | unit-test path: guard=false actions=0
(bar rect [0,0]-[1600,23], side window from y = 34; layer under the pointer on the bar: Background.)

Reporter's real-window reproduction, screenshots inspected by me:
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e1/a_selected.png (battleship "Bastion" selected, inspector with module table open)
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e1/b_after_click_on_taktik.png (overlay toggled off, selection brackets and inspector gone)
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e1/c_selected_again.png (selected again)
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e1/d_after_click_on_speed.png (clock running at 1×, selection and inspector gone)
The third pair (e/f, empty part of the bar) is inconclusive: both files are identical, the re-selection missed once the ships had moved. The empty-bar case rests on the source reading and the headless probes.

### engine 3 · bestätigt · minor · "Neu starten" does not stop the clock at once: the restarted battle is already up to 25 ticks old when it shows PAUSIERT

Ort: `crates/game/src/battle_view/practice.rs:381`

**Beschreibung.** The Restart branch of handle_view_requests builds the new battle and sets SimSpeed to its paused default, but unlike the Leave branch (lines 374-380) it does not pause Time<Virtual> itself. The pause only reaches the clock through time_control::apply_speed (time_control.rs:94), and the two system groups in Update are not ordered against each other. In this build apply_speed runs first, so it sees the change one frame later; before that, First and RunFixedMainLoop of the next frame still advance virtual time at the old speed and practice::step ticks the new battle.

**Fehlerszenario.** Run a practice battle to its end at 25x and click "Neu starten". The new battle shows "Gefechtszeit 00:02 PAUSIERT" instead of 00:00 and the hint "Leertaste beginnt das Gefecht" is missing; the fleets have already moved and the player can no longer give orders at tick 0. Under software rendering it is the full max_delta of 25 ticks, at 60 fps about 8 ticks; restarting from 1x gives 00:00 without the hint (tick not 0). Reproduced in a real window: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e3/bar_b_restarted_from_25x.png and bar_d_restarted_from_1x.png next to it.

**Vorschlag.** Call time.pause() in the Restart branch as the Leave branch does (the Time<Virtual> parameter is already there), or order apply_speed after handle_view_requests.

**Gegenprüfung.** The finding holds; I could not refute it. It is a minor but real defect: the restarted battle is already advanced while the bar says PAUSIERT.

I did not start the game. I read the game code and the engine sources, and looked at the finder's screenshots and logs. Nothing here comes from memory (no notes exist).

**Mechanism, confirmed by reading**
- The Restart branch (`practice.rs:381-386`) builds the new battle and sets `*speed = SimSpeed::default()`, but never calls `time.pause()`. The Leave branch directly above (374-380) does, with a comment explaining why.
- The only other path from `SimSpeed` to the clock is `apply_speed` (`time_control.rs:94-109`), which acts only when `speed.is_changed()`.
- `apply_speed` and `handle_view_requests` sit in two independent chains in `Update` (`time_control.rs:51-61`, `practice.rs:259-268`) with no ordering between them.
- The click is written in the egui pass, which bevy_egui runs in `PostUpdate`, so the request is handled in the next frame's `Update`.
- If `apply_speed` has already run in that frame, the clock stays unpaused through the following frame's `First` and `RunFixedMainLoop`. `practice::step` then ticks the new, unfinished battle before `apply_speed` finally pauses.

**Why `apply_speed` goes first**
- Nothing forces `handle_view_requests` to run before it. Bevy's topological order for unordered systems is arbitrary per build.
- In the multi-threaded executor, `handle_view_requests` must wait for `keys`. It also needs exclusive `SimSpeed` and `BattleFeed`, so it conflicts with `keyboard` and with every view system reading the feed.
- `apply_speed` only waits for the trivial `keyboard` system.
- The actual order in this build cannot be derived by reading alone; it rests on the finder's screenshots, which show the bad order on both restarts.

**Numbers match**
- At 25x, `max_delta` is 1.25 s, which is 25 fixed steps at 20 Hz. With battle `dt` 0.1 that is 2.5 s, shown as "00:02".
- The start hint appears only at `battle.tick == 0` (`hud.rs:226-232`), so it is missing.
- The finder's screenshots show exactly this: "00:02 PAUSIERT" after restarting from 25x, and "00:00 PAUSIERT" without the hint from 1x.
- Sources and binary are older than the finder's run and the git tree is clean, so the screenshots reflect the current code.

**Why it matters**
- The clock is not auto-paused when a battle ends, so every restart from the Bilanz hits this unless the player paused first.
- The button's tooltip promises "Dasselbe Gefecht noch einmal von vorn", and the code comment at `practice.rs:335` says a practice starts paused. After a restart the fleets have already moved and orders at tick 0 are no longer possible.
- The authors work around the same one-frame lag in three other places by pausing the clock directly (`practice.rs:375-378`, `session.rs:135-139`, `selftest.rs:196-208`). Only the Restart branch was missed.

The suggested fix is correct: `time.pause()` in the Restart branch, using the `Time<Virtual>` parameter that is already there.

**Belege der Gegenprüfung.** **Game code** (under `/home/user/exp_bevy_stellaris_fable_onePrompt`)
- `crates/game/src/battle_view/practice.rs:381-386`: Restart branch sets `*speed = SimSpeed::default()` with no `time.pause()`.
- `crates/game/src/battle_view/practice.rs:374-380`: Leave branch calls `time.pause()`, comment "Die Zeit hält sofort an, nicht erst beim Zustandswechsel im nächsten Frame".
- `crates/game/src/battle_view/practice.rs:259-268`: `(keys, handle_view_requests, apply_orders).chain().run_if(in_state(AppState::Practice))` in `Update`.
- `crates/game/src/time_control.rs:51-61`: `(keyboard, apply_speed).chain()` in `Update`, no relation to the practice systems.
- `crates/game/src/time_control.rs:94-114`: `apply_speed` acts only on `speed.is_changed()`; `max_delta = max(0.25, factor * 0.05)`, so 1.25 s at 25x.
- `crates/game/src/battle_view/practice.rs:420-438`: `step` ticks any unfinished battle in `FixedUpdate`.
- `crates/game/src/battle_view/hud.rs:700-706` and `934-936`: "Neu starten" writes `BattleViewRequest::Restart` from `draw_hud`, registered in `EguiPrimaryContextPass` (`battle_view/mod.rs:277`).
- `crates/game/src/battle_view/hud.rs:99-102` and `226-232`: `clock()` formats mm:ss; the hint "PAUSIERT – Leertaste beginnt das Gefecht" shows only when `battle.tick == 0`.
- `crates/sim/src/battle/step.rs:53-54`: `tick += 1; time += dt`. `assets/data/balance.ron:238`: `dt: 0.1`. `crates/sim/src/clock.rs:7`: `TICKS_PER_SECOND = 20`.
- Same lag worked around elsewhere: `crates/game/src/session.rs:135-139`, `crates/game/src/selftest.rs:196-208`.

**Engine sources** (under `/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`)
- `bevy_egui-0.43.1/src/lib.rs:1146-1149`: the egui context pass runs in `PostUpdate`.
- `bevy_time-0.20.1/src/lib.rs:82-92` and `152-193`: `time_system` runs in `First`; `run_fixed_main_schedule` runs in `RunFixedMainLoop`.
- `bevy_time-0.20.1/src/virt.rs:242-267`: the paused flag is read at the moment of the advance; delta is capped at `max_delta`.
- `bevy_time-0.20.1/src/fixed.rs:243-258`: `FixedMain` runs once per timestep of accumulated virtual delta.
- `bevy_ecs-0.20.1/src/schedule/graph/graph_map.rs:368-394` and `graph/dag.rs:572-622`: reverse Tarjan postorder over a node order scrambled by `swap_remove` during set flattening, so unordered systems get an arbitrary order.
- `bevy_ecs-0.20.1/src/schedule/executor/multi_threaded.rs:428-566`: ready systems start in topological index order, subject to access conflicts.

**Finder's artifacts** (under `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review`)
- `engine_perf/runs/e3/bar_a_end_at_25x.png`: "Gefechtszeit 00:32 BEENDET", 25x active, clock still running.
- `engine_perf/runs/e3/bar_b_restarted_from_25x.png` and `b_restarted_from_25x.png`: "Gefechtszeit 00:02 … PAUSIERT", no start hint, fresh 6-vs-6 battle.
- `engine_perf/runs/e3/bar_d_restarted_from_1x.png`: "Gefechtszeit 00:00 … PAUSIERT", no start hint (tick is not 0).
- `engine_perf/runs/e3/driver.log` and `engine_perf/tools/e3_restart.py`: only a click on "Neu starten" between the end shot and the restart shot.
- `verify_restart_pause/src/main.rs` and `run.log`: an earlier checker's headless toy model of the same systems (I read it, did not run it). `apply_speed` first gives tick 25 / 00:02 at slow frames and tick 8 at 60 fps for 25x; the other order, or `time.pause()` in the Restart branch, gives tick 0 with the hint.

**Timestamps**
- `practice.rs` 19:09, `time_control.rs` 18:23, `target/debug/game` 20:07; finder's run 22:39-22:41; `git status` clean.

### engine 4 · bestätigt · minor · Help text for numpad 7 / 1 / 3 is wrong: the engine keys turn the camera on the spot, numpad 7 points it at the sky

Ort: `crates/game/src/ui/panels.rs:726`

**Beschreibung.** BATTLE_KEYS describes the free camera's snap keys as "Blick von oben / von vorn / von der Seite (mit Strg: Gegenseite)". In bevy_camera_controller 0.20.1 (free_camera.rs:419-453) these keys only rotate the camera in place with Transform::looking_to: numpad 7 makes it look towards +Y (straight up), Ctrl+numpad 7 towards -Y, numpad 1 level towards -Z, numpad 3 level towards +X. The camera does not move above or in front of the battle.

**Fehlerszenario.** In the opening overview press numpad 7 expecting a view from above: the screen shows only stars, no ring and no ship; Ctrl+7 looks straight down from where the camera stands, which is south of the field, so only a piece of the outer ring is visible; numpad 1 and 3 show mostly sky. The player has to press U to find the battle again. Screenshots from a real window: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e2/b_numpad7.png, c_ctrl_numpad7.png and d_numpad1.png in the same folder.

**Vorschlag.** Describe the keys as what they are (turn the view up / north / east, with Strg the opposite way) or drop them from the help, and if a real top-down view is wanted add a CameraRequest that places the camera above the fight centre.

**Gegenprüfung.** The finding is correct and I could not refute it. It is a minor defect, but not a cosmetic one: the in-game help promises a view of the battle that the keys do not give, and for numpad 7 it states the opposite of what happens.

**What the keys do in the engine.** The battle camera uses the engine's FreeCamera with default bindings (`..default()` in `spawn_camera`, crates/game/src/battle_view/camera.rs:310-316), so numpad 7 / 3 / 1 with left Ctrl as the reverse modifier are live. The snap block in bevy_camera_controller-0.20.1/src/free_camera.rs:419-453 only builds a target rotation with `Transform::default().looking_to(dir, up)` and stores a rotation curve; `rotate_freecam_to` (462-483) writes only `transform.rotation`. The translation is never touched.

- Numpad 7: `looking_to(Dir3::Y, Dir3::Z)`, so the camera looks straight up; with Ctrl it looks straight down.
- Numpad 1: looks level towards -Z; with Ctrl towards +Z.
- Numpad 3: looks level towards +X; with Ctrl towards -X.

`Transform::look_to` (bevy_transform-0.20.1/src/components/transform.rs:475-484) confirms that "forward" is the given direction.

**Why that contradicts the help.** crates/game/src/ui/panels.rs:726-727 says "Blick von oben / von vorn / von der Seite (mit Strg: Gegenseite)". The battle lies in the plane y = 0 (`to_world`, battle_view/mod.rs:32; reference rings at `Vec3::ZERO`, overlay.rs:61) and the camera stands above it (`overview_transform`: y = 0.95·extent, z = 1.3·extent). Plain numpad 7 therefore turns the camera away from the battle into the star field. The top-down direction is the Ctrl variant, and even that looks at the ground under the camera, not at the fight.

**Geometry check.** I recomputed the view for all six variants from the overview position (FOV 50°, 16:9, extent 1700 and 2400). The share of the fleet area (disc of radius 1500) in view is 0 % for five variants and at most 2.2 % for numpad 1. Numpad 7 and numpad 3 show nothing of the field at all; Ctrl+7 and numpad 1 show only part of the outer ring.

**Not handled elsewhere.** Nothing in crates/game remaps or intercepts these keys: the only mentions are the engine defaults listed in the test `keys_do_not_collide`, and `sync_flight` clears the rotation curve only on camera requests. No `CameraRequest` gives a real top-down view. The battle help is reachable by the player (F1 or the "Hilfe" button, battle_view/hud.rs:926-927), and no test checks the wording of this line.

**Screenshots.** I did not start the game. The reporter's existing screenshots agree with the code reading: after numpad 7 only stars, after Ctrl+7 two ring arcs and the axis line, after numpad 1 a ring edge at the bottom of the screen, after numpad 3 only stars.

Impact: a player who follows the help loses the battle from view and has to press U or turn back with the mouse. Either of the reporter's suggested fixes is adequate: reword the line to say the keys turn the view up / north / east, or add a camera request that places the camera above the fight centre.

**Belege der Gegenprüfung.** Help text: /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/panels.rs:725-728
  ("Ziffernblock 7 / 1 / 3", "Blick von oben / von vorn / von der Seite (mit Strg: Gegenseite)")

Engine snap code: /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_camera_controller-0.20.1/src/free_camera.rs:419-453
  axis_front (Numpad1): (Dir3::NEG_Z, Dir3::Y), with Ctrl (Dir3::Z, Dir3::Y)
  axis_right (Numpad3): (Dir3::X, Dir3::Y), with Ctrl (Dir3::NEG_X, Dir3::Y)
  axis_top (Numpad7): (Dir3::Y, Dir3::Z), with Ctrl (Dir3::NEG_Y, Dir3::NEG_Z)
  let target = Transform::default().looking_to(dir, up).rotation;
  Defaults at lines 151-154 (ControlLeft, Numpad7, Numpad3, Numpad1); rotate_freecam_to at 462-483 sets only transform.rotation.

look_to: /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_transform-0.20.1/src/components/transform.rs:475-484 (back = -direction, so forward = direction).

Game side:
  /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/camera.rs:310-316 (FreeCamera with ..default()), 200-203 (overview_transform), 341-346 (sync_flight), 978-993 (test lists the engine keys but does not check the help wording for them)
  /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/mod.rs:32-34 (to_world, plane y = 0)
  /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/overlay.rs:61-64 (rings on the plane)
  /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/hud.rs:926-927 (battle help window shown to the player)
  A search of crates/ for Numpad, axis_top, axis_front, axis_right, key_snap_reverse and rotation_curve finds no remapping.

Geometry check (my script /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_numpad/geom.py), share of the fleet disc (r <= 1500) / of the outer ring (r = 2400) in view:
  extent 1700: Numpad7 0.0 % / 0.0 %; Ctrl+Numpad7 0.2 % / 18.7 %; Numpad1 2.2 % / 32.6 %; Ctrl+Numpad1 0 % / 0 %; Numpad3 0 % / 0 %; Ctrl+Numpad3 0 % / 0 %
  extent 2400: Numpad7 0.0 % / 0.0 %; Ctrl+Numpad7 0.0 % / 17.3 %; Numpad1 0.0 % / 23.4 %; the other three 0 % / 0 %

Reporter's screenshots, viewed but not produced by me, in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/runs/e2/: a_overview.png (fleets and rings), b_numpad7.png (stars only), c_ctrl_numpad7.png (two ring arcs and axis line, no ships), d_numpad1.png (ring edge at the bottom), e_numpad3.png (stars only). The game.log in the same folder shows the engine's own description: "[ControlLeft + ]Numpad7 - Snap to Up (+Y)/Down (-Y)".

### engine 5 · widerlegt · minor · Latent trap for the campaign bridge: ships never get an entity if the feed is replaced between update_poses and sync_ships

Ort: `crates/game/src/battle_view/scene.rs:635`

**Beschreibung.** sync_ships spawns entities for the indices state.ships_seen..battle.ships.len(), skips an index whose pose is missing in ViewFrame (continue, lines 601-605) and then sets ships_seen to battle.ships.len() regardless. ViewFrame.poses is filled once per frame in BattleViewSet::Poses, and systems that put another battle into the feed in Update are not ordered against the view sets (practice.rs:258-268: start_requested, handle_view_requests; the sets are only chained among themselves in mod.rs:253-263). Not reachable in practice mode today, because a restart never has more ships than the battle it replaces; there the same race only draws the new ships at the old battle's last poses for one frame.

**Fehlerszenario.** M4e bridge follows the API notes ("Watching another battle: call begin again") from an Update system that happens to run after update_poses: the feed now holds a battle with more ships than the previous one, rebuild_on_epoch resets ships_seen to 0, ships with an index beyond the old count have no pose, are skipped, and ships_seen jumps past them. Those ships stay invisible for the whole battle while the HUD markers and their shots are shown.

**Vorschlag.** Stop at the first index without a pose instead of skipping it (break, and set ships_seen to that index), or require feed writers to run .before(BattleViewSet::Poses) and say so in the API notes.

**Gegenprüfung.** The mechanism is described correctly, but nothing in the code under review can trigger it, so it is not a present defect. Its only effect today is at most one frame of stale ship positions after "Neu starten", which is cosmetic.

What I confirmed by reading:
- sync_ships skips a ship that has no pose or no side kit and then sets ships_seen to battle.ships.len() regardless, so a skipped ship never gets an entity until the next epoch change or hide/show.
- ViewFrame.poses is written only by update_poses, once per frame. The feed writers in Update (start_requested, handle_view_requests, apply_orders) have no ordering against the view sets, so a writer can run between update_poses and rebuild_on_epoch.

Why it cannot lose ships today:
- Only two non-test places call BattleFeed::begin, both in practice.rs.
- start_requested only ever sees a StartPractice written in AppState::MainMenu. BattleViewShown is false then, so no view system runs that frame. The next frame runs show_view before Update, and update_poses then fills poses for the new battle before sync_ships.
- Restart rebuilds from the same PracticeConfig, so the new battle has the initial ship count. The sim only ever appends ships, so the replaced battle has at least as many and every new index has a pose. Restart is only offered in the end-of-battle summary.
- No campaign bridge exists: ViewState::Battle is never entered and BattleFeedSet::Campaign has no systems.

The claimed failure therefore needs M4e code that does not exist yet: a begin() from an unordered Update system, while the view is already showing another battle, with a larger battle. The "API notes" the finding quotes are not in the repository. That is a hazard for future code, not a concrete problem in this change.

It is still worth carrying into M4e as a note: have feed writers run .before(BattleViewSet::Poses), or call begin() only while the view is hidden or from FixedUpdate in BattleFeedSet::Campaign.

**Belege der Gegenprüfung.** All paths at HEAD 8206ed1, working tree clean. Nothing was built or run; this is from reading only, and nothing here comes from stored memory.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/scene.rs
- 599-635: spawn loop over state.ships_seen..battle.ships.len(); `continue` at 601-605 when the side kit or pose is missing; line 635 `state.ships_seen = battle.ships.len();` unconditionally.
- 463-499: rebuild_on_epoch resets SceneState (ships_seen = 0) and rebuilds SideAssets only when the epoch changes.
- 366-377: rebuild_on_epoch, sync_ships, ... chained in BattleViewSet::Scene.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/mod.rs
- 106-132: update_poses is the only writer of ViewFrame.poses.
- 253-263: (Poses, Camera, Scene, Overlay).chain().run_if(view_active).

/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_ecs-0.20.1/src/schedule/schedule.rs
- 847-869: a collective run_if on several configs goes onto one anonymous set, so view_active is evaluated once per Update run and the four view sets run or skip together.

/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_ecs-0.20.1/src/schedule/executor/multi_threaded.rs
- 461-524 and 531-566: ready systems with no ordering edge are started whenever no conflicting system is running, so an unordered ResMut<BattleFeed> writer can land between update_poses and the Scene set.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/practice.rs
- 258-268: start_requested and the (keys, handle_view_requests, apply_orders) chain are in Update with no ordering against BattleViewSet.
- 327 and 384: the only non-test begin() calls.
- 381-386: Restart rebuilds from practice.config.clone().
- 278-311: start_from_args runs on OnEnter(MainMenu) and takes args.practice, so it fires once.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/mod.rs
- 214-231: the setup dialog writes StartPractice only in AppState::MainMenu.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/hud.rs
- 700-707: "Neu starten" appears only in the "Bilanz" window and only when can_restart.
- 778-830: HUD markers come from battle.ships plus frame.poses, not from entities, which matches the finding's "markers shown, hull missing" description.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/battle/mod.rs:468 and /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/battle/step.rs:458
- The only mutations of Battle::ships are push; there is no remove, retain or truncate anywhere in crates/sim/src.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/state.rs
- 27-29: ViewState::Battle is #[allow(dead_code)] and never set; a grep for BattleFeedSet::Campaign finds only its definition and configure_sets.

Schedule order: StateTransition is inserted after PreUpdate (/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_state-0.20.1/src/app.rs:335) and RunFixedMainLoop precedes Update (/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_app-0.20.1/src/main_schedule.rs:224-231). So show_view and every FixedUpdate feed refresh, including launched fighters, happen before update_poses in the same frame.

Related observation outside this finding, also campaign-only: the same `continue` permanently skips ships of a combatant added mid-battle through Battle::add_combatant, because side kits are built only on an epoch change (scene.rs:485-494).

### engine 6 · bestätigt · minor · Latent trap for the campaign bridge: showing the same battle again after hide_view leaves the effect pools without entities

Ort: `crates/game/src/battle_view/effects.rs:308`

**Beschreibung.** hide_view resets SceneState and SideAssets (scene.rs:455-456) and despawns every BattleVisual entity, pool slots included, but EffectPools keeps its epoch and the spawned count of each pool. show_effects rebuilds the pools only when pools.epoch differs from sides.epoch. Not reachable in practice mode, where every start calls BattleFeed::begin and bumps the epoch.

**Fehlerszenario.** The bridge leaves ViewState::Battle for the map and comes back to the same battle without a new begin (refresh is written to tolerate skipped ticks, so this is a natural flow). rebuild_on_epoch rebuilds the side kits with the unchanged epoch, show_effects finds the epochs equal and keeps the old counts, and assign only spawns slots beyond the old high-water mark: tracers, missiles, beams, flashes and shield caps up to that mark stay invisible for the rest of the battle.

**Vorschlag.** Reset EffectPools in hide_view (it is a one-line *pools = EffectPools::default()), or key the rebuild on a generation counter of SideAssets instead of the feed epoch.

**Gegenprüfung.** Confirmed, at the severity the finding itself gives: a latent, minor defect. Nothing in the current build can trigger it.

The mechanism is as described. hide_view despawns every BattleVisual entity, pool slots included, and resets SceneState and SideAssets but not EffectPools. On the next shown frame rebuild_on_epoch finds state.epoch == None and rebuilds the side kits with epoch Some(feed.epoch). If the feed's epoch is unchanged, show_effects sees pools.epoch == sides.epoch and skips the rebuild, so every pool keeps its old `spawned` count. assign fills slots from index 0 but only spawns entities at indices >= spawned. Everything below the earlier high-water mark therefore has no entity: in effect no tracers, missiles, beams, flashes or shield caps until the battle gets busier than its earlier peak. I could not refute this. Nothing else touches EffectPools, and hide_view runs in StateTransition before Update while the Scene set is gated on view_active, so show_effects never sees sides.epoch == None and never rebuilds by accident.

It is not reachable today. show_view runs only on OnEnter(AppState::Practice), which only start_requested triggers, directly after BattleFeed::begin has bumped the epoch; Restart also goes through begin. ViewState::Battle is never entered, because no code writes NextState<ViewState>. Practice mode therefore shows nothing wrong. The only present effect is that EffectPools keeps ten side-material handles and its item buffers alive after leaving practice mode until the next battle, which is negligible.

I count it as real rather than as speculation because re-showing the same battle is a flow this module is built for. The feed ties a new epoch to "another battle" (doc of begin and of epoch, test message "dasselbe Gefecht, dieselbe Epoche"). The ViewState::Battle hooks are registered in this diff for the campaign, whose bridge is not written yet. hide_view already resets SceneState, which only matters for a re-show at an unchanged epoch, and ships, wrecks, camera and selection all come back correctly in that flow; the effect pools are the one piece left out. In Bevy 0.20.1 NextState::set also runs OnExit and OnEnter when the state is unchanged, so a campaign bridge that re-issues set(ViewState::Battle) for the battle already on screen would trigger it without ever leaving for the map.

The suggested one-line reset of EffectPools in hide_view is correct and safe: with pools.epoch == None the next shown frame always rebuilds. If only failures reachable in the current build should count, treat this as not real; the trap also never springs if the campaign bridge calls BattleFeed::begin before every entry into ViewState::Battle.

**Belege der Gegenprüfung.** /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/scene.rs:436-459 (hide_view): 454 `commands.despawn_all::<With<BattleVisual>>()`, 455 `*state = SceneState::default()`, 456 `*sides = SideAssets::default()`; EffectPools is not a parameter. Same file :477-494 (rebuild_on_epoch): returns only if `state.epoch == Some(feed.epoch)`, otherwise builds SideAssets with `epoch: Some(feed.epoch)`.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/effects.rs:308-312: rebuild only if `pools.epoch != sides.epoch`; :97 rebuild copies `sides.epoch`; :348-363 assign spawns only `while pool.spawned < pool.items.len()`; :351 slots carry BattleVisual. EffectPools is referenced nowhere outside effects.rs.

/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/feed.rs:115-116 and :136-138: the epoch changes only in begin, documented as "ein anderes Gefecht"; :523 "dasselbe Gefecht, dieselbe Epoche".

Not reachable now: /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/practice.rs:327 and :384 are the only begin calls outside tests, :339 the only `next.set(AppState::Practice)`, :448 `feed.clear()` on exit. /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/mod.rs:273-276 registers show_view/hide_view for AppState::Practice and ViewState::Battle; :253-263 gates the view sets on view_active. /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/state.rs:27-29: ViewState::Battle is `#[allow(dead_code)]`; a search of crates/ finds no `NextState<ViewState>` and no other use of ViewState::Battle.

Engine sources under /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/: bevy_ecs-0.20.1/src/system/commands/mod.rs:483 and bevy_ecs-0.20.1/src/world/mod.rs:1691-1745 (despawn_all despawns every entity matching the filter); bevy_state-0.20.1/src/app.rs:335 (StateTransition runs after PreUpdate, before Update); bevy_state-0.20.1/src/state/resources.rs:194-200 (NextState::set runs OnExit and OnEnter also for an unchanged state; set_if_different skips them).

Checked by reading only; the game was not started and no test was run.

### engine 7 · widerlegt · minor · Measured numbers of the view; the large-battle selftest never comes near "thousands of projectiles"

Ort: `crates/game/src/selftest.rs:564`

**Beschreibung.** All runs: dev profile, llvmpipe under Xvfb, about 3 fps; tests (64) and clippy pass. Large battle as given by the implementer (496 ships, seed 1, --selftest 600), two runs: 182 / 180 frames, 443 ships in play at once, 333 wrecks, peak 236 shots and 69 / 100 beams shown, 701 / 719 pool entities; view 749 / 754 µs mean, 630 / 640 µs median, 6599 / 4965 µs worst frame; mean per part: Szene 80 / 79, Effekte 14, Linien 18 / 17, Tafeln 637 / 643 µs; Gefechtsrechner 173 / 189 µs and Feed 27 / 28 µs per tick. The implementer's figures are confirmed; the HUD is about 85 percent of the view's own time. Stress runs of mine: 250 against 250 cruisers reached 1186 shots, 129 beams, 1659 pool entities at 736 / 718 µs mean (Effekte 35 / 29 µs, Gefechtsrechner 489 / 443 µs per tick); 1490 ships with carriers (peak 911 shots, 1247 wrecks) 931 µs mean (Szene 233 µs, Feed 82 µs per tick); line-up of 12 ships 469 µs. With the present weapon data no fleet I tried exceeded about 1200 projectiles in flight, so the pool limits (6000 tracers per side) are untested. Per-thread CPU time from /proc: Bevy's own threads (main, compute pool, render) used about 16 ms per frame in the 12-ship line-up and about the same in the 496-ship battle, so ship and effect entities add no measurable engine-side CPU; the rest is the software rasterizer (about 410 ms per frame). GPU cost cannot be measured here. In the code I found no per-frame mesh or material creation, no spawn or despawn churn, no O(n^2) loop in the view, and one feed copy per tick. Notes: nothing in these findings comes from memory; the stored note cloud-render-check was used only for how to run under Xvfb. Worth remembering, all learned in this session: GUI input can be automated here without installing anything, with XTEST through ctypes (reviewer tool xdrive.py in the shared review scratch folder, seen and used there); synthetic absolute pointer motion makes mouse-look tests meaningless (seen in my numpad run); the lavapipe executor thread carries the process name, so per-thread CPU must be split by thread id (seen in the samples).

**Fehlerszenario.** A change that makes effect handling scale badly with projectile count would pass every selftest: the 496-ship run peaks at 236 shots and 69 to 100 beams because, as the implementer notes, the long lines only fight at their ends, and the peaks are sampled at about 3 rendered frames per second. Logs: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/run_gross.log and run_kreuzer.log, run_traeger.log, run_lineup.log, threads_gross.tsv, threads_lineup.tsv in the same folder.

**Vorschlag.** Add a denser stress fleet to the selftest or the tests (for example speer:250 against speer:250, which reaches about 1200 projectiles) and report the pool high-water mark; take GPU frame times on a real machine before calling the performance requirement met.

**Gegenprüfung.** Not a real problem. The measured numbers in the finding are accurate, but they describe no defect, and no required test is missing.

What I confirmed
- The quoted figures match the finder's logs exactly.
- The large preset really does peak low. Stepping the same battle tick by tick with the prebuilt headless tool gives a true peak of 239 projectiles; the selftest sampled 236. Frame sampling hides nothing.
- No unit test goes beyond a small mixed fleet, and the selftest verdict depends only on the photo count (selftest.rs:536-537, 563).

Why it does not matter
- No scaling defect exists, and the finder says so too. `collect` and `assign` are linear in the number of effects (effects.rs:149-275, 325-364). Pool entities share mesh and material, which is what the engine batches on, and extraction is change-driven.
- The finder's own stress runs show the view's time is flat: 0.47 ms at 12 ships, 0.74 ms at 500 ships with 1186 projectiles, 0.93 ms at 1490 ships. The effects part costs 14 to 35 µs.
- A performance test is not required. The project plan says for milestone 4 that performance is a headless measurement (400 ships), not a test. That measurement exists (`headless battle --bench`).
- The suggested fix is mostly already there:
  - The selftest takes any fleet through `--fleet-a` / `--fleet-b`; that is how the finder ran 250 against 250 cruisers, with no code change.
  - The pool high-water mark is already printed as "Effektplätze" (`EffectPools::spawned()`, which never shrinks; effects.rs:137-139, 318; selftest.rs:575).
  - The cap mechanism has unit coverage (`pools_never_grow_past_their_cap`, effects.rs:703-722), so "untested" overstates it.
- A denser fleet would not make the selftest catch a scaling regression either, because timings are printed, not asserted.
- The failure scenario is a hypothetical future change, not something that fails today.
- GPU frame times cannot be taken in this container. That is a caveat for the milestone report, not a code change: the report should say that GPU frame rate at thousands of projectiles is unverified.

One correction to the finding
Thousands of projectiles are reachable with the present weapon data; it takes about a thousand cruisers. Cruisers at 500 per side peak at 2143 projectiles, at 1000 per side at 3646, and the headless bench with 2000 ships at 2532. The tracer cap of 6000 per side is still far away at that size. Anyone who wants a figure at that scale can get it from the existing binary with `--practice --fleet-a speer:500 --fleet-b speer:500 --selftest N`.

Side note, outside this finding
`collect` counts a shot or beam as shown even when `Pool::push` drops it at the cap (effects.rs:67-71, 185-192, 230-237). Above the caps the selftest's "gezeigt" figure would over-report. This is only a diagnostic number, in a range realistic fleets do not reach.

Sources and notes for the main session
- I did not start the game. I ran only the prebuilt `target/release/headless` (no display, about 15 s CPU); the repository is untouched.
- Nothing here comes from memory; the one stored note (cloud render check) was not needed.
- Worth remembering, all learned in this session:
  - `headless battle --a … --b … --seeds 1..2 --trace 0.1` prints the projectile count per tick, so exact peaks are available without a display (seen by running it).
  - The projectile peaks listed above (measured with that tool).
  - The plan file states that milestone 4 performance is a measurement, not a test (read in /root/.claude/plans/baue-ein-origin-res-4x-strategiespiel-quizzical-wirth.md, line 202).

**Belege der Gegenprüfung.** Finder's logs (in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/engine_perf/):
- run_gross.log / run_gross2.log: 496 ships, 443 in play, 333 wrecks, at most 236 shots and 69 / 100 beams, 701 / 719 Effektplätze, view 749 / 754 µs mean (Szene 80 / 79, Effekte 14, Linien 18 / 17, Tafeln 637 / 643).
- run_kreuzer.log / run_kreuzer2.log: 500 ships, at most 1186 shots and 129 beams, 1659 Effektplätze, view 736 / 718 µs mean, Effekte 35 / 29 µs.
- run_traeger.log: 1490 ships, at most 911 shots, view 931 µs mean. run_lineup.log: 12 ships, 469 µs.
- test.log: 64 passed.

My runs with the prebuilt /home/user/exp_bevy_stellaris_fable_onePrompt/target/release/headless (sim, headless crate and data are unchanged since d883061; seed 1; tick-level peak of battle.shots.len()):
- Großgefecht preset (bastion:6,horst:4,speer:30,waechter:60,dolch:100 against bastion:10,speer:40,waechter:60,dolch:90): 1170 ticks, 117 s, 333 ships lost, peak 239.
- speer:250 against speer:250: peak 1231 (both offensive: 1251; both in wedge: 277).
- speer:500 against speer:500: peak 2143.
- speer:1000 against speer:1000: peak 3646.
- bastion:250 against bastion:250: peak 1015.
- `battle --bench 400`: up to 330 projectiles, 189 µs per tick. `battle --bench 2000`: up to 2532 projectiles, 797 µs per tick mean.
Logs: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_projectile_scale/ (gross_trace.log, kreuzer.log, kreuzer_off.log, kreuzer_keil.log, kreuzer500.log, kreuzer1000.log, bastion250.log, bench_400.log, bench_2000.log).

Code read:
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/selftest.rs:536-537, 563 (verdict is `test.photos >= wanted` only), 564-583 (prints peak shots, peak beams, `stats.entities` as "Effektplätze", per-part timings).
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/effects.rs:27-32 (caps), 67-71 (`Pool::push` drops at the cap), 137-139 and 315-320 (`spawned()` reported as entities), 149-275 (`collect`, one pass over shots, beams, impacts, ships), 325-364 (`assign`, one pass over slots, spawns only up to the high-water mark), 703-722 (cap test).
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/practice.rs:278-311 (`--fleet-a` / `--fleet-b` feed the practice battle the selftest drives).
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/meshes.rs:1094-1128 (tracer, missile and beam meshes are a few dozen triangles each).
- Engine: /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_pbr-0.20.1/src/render/mesh.rs:2909-2960 (batch compare data is material bind group plus mesh asset), 1960-1980 (mesh extraction filtered by `Changed<…>`).
- Plan: /root/.claude/plans/baue-ein-origin-res-4x-strategiespiel-quizzical-wirth.md:199-202 ("Leistung als Headless-Messung (400 Schiffe), nicht als Test"), 74-75 (ships and projectiles as entities with shared mesh and material handles).

## Blickwinkel: anforderungen

### anforderungen 1 · bestätigt · critical · "Zum Hauptmenü" from a campaign terminates the game (the reported fix does not hold)

Ort: `crates/game/src/session.rs:134`

**Beschreibung.** The QuitToMenu branch of `handle_requests` (an Update system) calls `SimState::remove(world)` and only *requests* the state change. In the same frame the map systems still pass `in_state(AppState::InGame)` and require `Res<Galaxy>` (crates/game/src/map.rs: `focus_camera` l.369, `sync_ownership` l.428, `draw_overlays` l.527; also `rebuild_map` l.155 and `pick_system` l.386). Bevy 0.20 reports "Parameter `Res<Galaxy>` failed validation: Resource does not exist" and its default error handler ends the process with exit code 101. The milestone changed exactly this branch (pause the clock first, session.rs:135-139) and reports it as the fix for "quitting a campaign to the main menu crashed"; that only removes the SimClock panic of the core's run conditions. map.rs is untouched by the milestone; I did not build the base commit, so I cannot say whether the crash is older. Practice -> menu -> campaign works (checked); only campaign -> menu dies.

**Fehlerszenario.** Start the game, "Neue Partie", Esc, click "Zum Hauptmenü": the window closes, exit code 101. Reproduced 4 of 4 times with real input under Xvfb (campaign paused, running at 1x, running at 25x, and once more in a live session). Logs with the three validation errors: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/j_quit_paused/game.log , /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/j_quit_running/game.log , /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/j_quit_fast/game.log . Consequence for the player: from a campaign the main menu, and with it the Gefechtsübung, can only be reached by restarting the program.

**Vorschlag.** Do not remove the simulation inside Update. Set `NextState(MainMenu)` (and pause the clock) in `handle_requests` and call `SimState::remove` from `OnExit(AppState::InGame)`, which runs in the transition schedule after the last InGame frame; or gate the map/traffic/history systems with `resource_exists::<Galaxy>` / `Option<Res<Galaxy>>`. Add a regression test that sends `SessionRequest::QuitToMenu` to an app with the real plugin set and runs two more updates.

**Gegenprüfung.** Confirmed. I could not refute it; the code, the engine sources and the reporter's raw run logs all agree. I did not start the game, as instructed.

Mechanism in the game code:
- `handle_requests` is an exclusive system in `Update` with no ordering against anything (crates/game/src/session.rs:64, :87). Its QuitToMenu branch (:134-144) pauses the clock, calls `SimState::remove(world)` immediately and only sets `NextState(MainMenu)`.
- `SimState::remove` removes `Galaxy`, `Polities`, `SimClock`, `Economy` and the rest (crates/sim/src/state.rs:53-55, :68-81). It is the only place in the game that removes `Galaxy`.
- The map systems are gated only by `in_state(AppState::InGame)` / `in_state(ViewState::Galaxy)` (crates/game/src/map.rs:87-104) and take a plain `Res<Galaxy>`: `rebuild_map` :159, `focus_camera` :371, `pick_system` :390, `sync_ownership` :429, `draw_overlays` :529. `draw_traffic` (traffic.rs:49-57) and `history::sample` (history.rs:71-75) follow the same pattern.
- Only the UI is protected: `draw_ui` uses `Option<SimParams>` (ui/mod.rs:157). No error handler is overridden anywhere in crates/.

Engine behaviour in Bevy 0.20.1, read from source:
- `StateTransition` runs after `PreUpdate` (bevy_state-0.20.1/src/app.rs:335). A `NextState` set in `Update` therefore takes effect in the next frame, so the state is still InGame for the rest of the quit frame.
- `Res<T>::get_param` returns `SystemParamValidationError::invalid("Resource does not exist")` (bevy_ecs-0.20.1/src/system/system_param.rs:712-714), which becomes `RunSystemError::Failed` (system/system.rs:389-405).
- That error carries `Severity::Panic` by default (error/bevy_error.rs:536-553), and the default handler `match_severity` panics on it (error/handler.rs:118-156).
- The multi-threaded executor stores the panic and resumes it after the schedule finishes (schedule/executor/multi_threaded.rs:305-309, :847-880). Nothing catches it, so the process exits with 101.

Whether a Galaxy-reading system actually runs after `handle_requests` in that frame cannot be settled by reading: the order of unordered systems comes out of the schedule graph build and is fixed per binary. The reporter's logs settle it for this build. All three runs (paused, 1x, 25x) show the same three failures in `focus_camera`, `sync_ownership` and `draw_overlays`, so this is not a rare race. `pick_system` is missing only because its egui-pointer condition skips it while the cursor is on the menu button.

Scope and impact: "Zum Hauptmenü" in the pause menu (ui/panels.rs:691-692) is the only way from a campaign to the main menu, and so to the Gefechtsübung; it kills the program. map.rs, traffic.rs and history.rs are unchanged since d883061, and the base version also removed the simulation inside `Update`, so the weak spot predates the milestone. The milestone changed exactly this branch and its commit message reports leaving a campaign as fixed. That fix only stops the fixed-tick run conditions `day_start`/`month_start` from reading a missing `SimClock`; the user-visible crash remains. No test or selftest sends QuitToMenu.

On the suggested fix: deferring the removal to the state transition is right, and practice.rs already does this in its `OnExit(AppState::Practice)` handler. `OnExit(AppState::InGame)` would also fire on a later InGame to GameOver transition, where `draw_ui` still shows the simulation (ui/mod.rs:190). `OnEnter(AppState::MainMenu)` or `resource_exists::<Galaxy>` gating on the map, traffic and history systems is the safer variant.

**Belege der Gegenprüfung.** Game code (all under /home/user/exp_bevy_stellaris_fable_onePrompt):
- crates/game/src/session.rs:64 `.add_systems(Update, (handle_requests, fade_notice))`; :87 `fn handle_requests(world: &mut World)`; :134-144 QuitToMenu branch with `SimState::remove(world)` at :140.
- crates/sim/src/state.rs:53-55 `remove` calls `world.remove_resource::<$ty>()` for every entry of the list at :68-81, which includes `galaxy: Galaxy`.
- crates/game/src/map.rs:87-104 gating by `in_state` only; `Res<Galaxy>` at :159, :371, :390, :429, :529.
- crates/game/src/ui/panels.rs:691-692 button "Zum Hauptmenü" pushes `SessionRequest::QuitToMenu`.
- grep over crates/ finds no `FallbackErrorHandler` or `set_error_handler`, and no other removal of `Galaxy`.
- `git diff d883061 -- crates/game/src/map.rs crates/game/src/traffic.rs crates/game/src/history.rs` is empty. The session.rs diff only adds the two pause lines before `SimState::remove`.
- Commit d52304f message: "Nebenbei behoben: Beim Verlassen einer Partie hielt die Zeit nicht an; die Laufbedingungen des Kerns lasen dann eine Uhr, die es nicht mehr gab."

Engine sources (/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f):
- bevy_state-0.20.1/src/app.rs:335 `schedule.insert_after(PreUpdate, StateTransition)`.
- bevy_ecs-0.20.1/src/system/system_param.rs:712-714; src/system/system.rs:389-405; src/error/bevy_error.rs:536-553; src/error/handler.rs:118-156; src/schedule/executor/multi_threaded.rs:305-309 and :847-880.

Reporter's run artifacts, which I inspected directly (under /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs):
- j_quit_paused/game.log, j_quit_running/game.log and j_quit_fast/game.log each contain exactly three errors of the form "Encountered an error in system `game::map::focus_camera`: Parameter `Res<Galaxy>` failed validation: Resource does not exist", likewise for `sync_ownership` and `draw_overlays`. The backtrace frames (bevy_error.rs:549, system.rs:404, function_system.rs:717) match the source lines I read.
- Each driver.log shows "click 1 at 767,488", then "GAME PROCESS ENDED" about two seconds later and "game_exit=101".
- j_quit_paused/j01_menu_paused.png shows a running campaign with the "Menü" window open and "Zum Hauptmenü" highlighted at that click position.
- target/debug/game has mtime Nov 25 20:07, matching HEAD 8206ed1 (20:07:31), and the working tree is clean, so the runs used the current code.

Not done: no run of my own. The same-frame ordering therefore rests on the three logs above.

### anforderungen 2 · bestätigt · major · Every click on the top bar falls through to ship picking: the selection and the inspector are lost

Ort: `crates/game/src/battle_view/hud.rs:169`

**Beschreibung.** The top bar is an `egui::Panel::top` shown inside a hand-made root `Ui` on the background layer (hud.rs:909-915, 169). `pick_on_click` is gated only by `not(egui_wants_any_pointer_input)` (camera.rs:272). bevy_egui 0.43.1 runs the pass as `ctx.run_ui(input, |_| schedule)` (bevy_egui-0.43.1/src/lib.rs:1899), so egui's own root UI never shrinks; `Context::is_pointer_over_egui()` (egui-0.36.2/src/context.rs:2951-2958) therefore answers `false` for anything drawn on the background layer. Side panels, inspector and Bilanz are `egui::Window`s and are detected correctly; the top bar is not. The headless test `a_click_selects_the_ship_under_the_cursor` calls `pick_on_click` directly and bypasses the run condition, so it cannot see this. Same mechanism: the wheel over the top bar still changes "Flug x,x×" (`tame_free_camera`, camera.rs:356), and a right-button drag that starts on any HUD panel rotates the camera (engine controller knows nothing about egui; seen in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/c_camera/c02_after_right_drag_on_panel.png ). This is the same root cause as the known campaign defect "Klicks auf Seitenleisten wirken auf die Karte durch".

**Fehlerszenario.** Practice battle, click a ship (inspector opens), then click "Namen", "Bilanz", "⏸ Pause", a speed button or even an empty spot of the top bar: the button works AND the click is also evaluated as a click into space, so the ship is deselected and the inspector with the module table closes. Verified three times with real input: before/after /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/b_select/b02_after_click_ship.png and /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/b_select/b03_after_click_namen.png ; "Bilanz" button: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/d_live/d19_bilanz_again.png ; empty part of the bar: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/e_live/e06_after_click_topbar_empty.png . A player cannot pause with the button or toggle an option while keeping a ship selected.

**Vorschlag.** Store the top bar's rect (or `response.contains_pointer()`) in a resource each frame and make `pick_on_click`, the wheel guard in `tame_free_camera` and the mouse-look start respect it; or draw the bar as an anchored `egui::Window`/`Area` like the other tafeln. Add a test that runs the real schedule with a cursor position inside the bar.

**Gegenprüfung.** I tried to refute the finding and could not; every link of the chain holds in the game code and in the local engine sources, and a headless egui-only reproduction agrees. The game itself was not started.

**Why the top bar is invisible to the guard**

1. `pick_on_click` has exactly one guard: `.run_if(not(egui_wants_any_pointer_input))` (`camera.rs:272`). The function itself (`camera.rs:516-546`) never checks the bar's rect. On `just_pressed(Left)` it casts a ray at the cursor and writes `selected.0 = picked`, which is `None` when no ship lies on the ray.
2. `EguiWantsInput` is written only in `PostUpdate` (`bevy_egui-0.43.1/src/input.rs:1716-1735`, registered at `lib.rs:1145-1160`). `ButtonInput::just_pressed` is set in `PreUpdate` of the press frame. So in `Update` of the press frame the guard sees the values from the previous frame, when the pointer was only hovering.
3. bevy_egui runs the pass as `ctx.run_ui(input.take(), |_| world.try_run_schedule(..))` (`lib.rs:1899-1901`) and discards egui's own root `Ui`. egui then stores `root_ui_available_rect = Some(root_ui.available_rect_before_wrap())` (`egui-0.36.2/src/context.rs:816-819`), which is the whole viewport because nothing was placed in that `Ui`.
4. `is_pointer_over_egui()` (`context.rs:2943-2965`) returns `!root_ui_available_rect.contains(pointer_pos)` for the Background layer, so it is false everywhere on the background.
5. The bar is on the Background layer: `draw_hud` builds its own root `Ui` with `LayerId::background()` (`hud.rs:909-915`) and shows `Panel::top("gefechtsleiste")` in it (`hud.rs:169`). `Panel::show` only moves the parent `Ui`'s cursor (`panel.rs:852-865`); nothing reaches egui's pass state.
6. The other three flags are also false while hovering: `egui_wants_pointer_input` depends on `is_pointer_over_egui` (`context.rs:2973-2976`), `is_using_pointer` is set only after a press (`memory/mod.rs:559-561`), and tooltips do not count as popups (`popup.rs:566`).

**Consequence**

A press anywhere on the bar runs `pick_on_click`. With a ship selected, the selection becomes `None`. The inspector with the module table is no longer drawn (`hud.rs:174-179`), and follow mode is switched off (`camera.rs:498-504`). egui still delivers the button click, so the button works and the selection is lost. This hits Pause, the speed buttons, Taktik, Namen, Gefecht, Übersicht and empty spots of the bar.

**Not handled elsewhere**

- No other use of the bar rect or pointer state exists in `crates/game`; `enable_absorb_bevy_input_system` is left at its default (off).
- The camera test calls `pick_on_click` through `run_system_once` (`camera.rs:788-799`), bypassing the run condition.
- The HUD tests draw into egui's own root `Ui` (`hud.rs:963-975`), a setup in which the bar would be detected, so they cannot see the defect either.
- The campaign map uses the same pattern (`ui/panels.rs:150`, `map.rs:92-94`); its click-through bug is still an open task in this session's task list, so the battle view is a new, unfixed instance.

**Secondary claims**

- Wheel over the bar changes fly speed: true by the same mechanism (`camera.rs:356`), low impact.
- Right-drag starting on a panel rotates the camera: true (`bevy_camera_controller-0.20.1/src/free_camera.rs:341-349, 400-407`; `tame_free_camera` only reacts to keyboard focus, `camera.rs:365-368`). This is a different cause, not the Background-layer detection, and it applies to windows too; minor.

**Why it matters**

This is wrong behaviour on a core interaction, not cosmetics. A player inspecting a ship's modules, which the specification requires to be visible, loses the inspector on every mouse use of the bar. The keyboard shortcuts are a workaround.

Nothing in this report comes from user memory.

**Belege der Gegenprüfung.** **Headless reproduction (egui 0.36.2 only, outside the repository)**

Files are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_topbar_f1/ (`src/main.rs`, `run.log`). The pass is run the way bevy_egui runs it, with the panels built as in `hud.rs`. The flags are read after the hover frame, which is what `Update` sees in the press frame.

| Pointer position | Layer | `egui_wants_any_pointer_input` | `pick_on_click` runs | egui click delivered |
|---|---|---|---|---|
| Top bar, Pause button (151,11) | Background | false | yes | "Pause" |
| Same, after 120 hover frames (tooltip showing) | Background | false | yes | "Pause" |
| Top bar, empty spot (1300,11) | Background | false | yes | none |
| Anchored window `partei_0` | Middle | true | no | "Rückzug" on its button |
| Top bar, panels drawn into egui's own root `Ui` (as the unit tests in `hud.rs:963-975` do) | Background | true | no | "Pause" |

In the first three rows all four flags (`is_pointer_over_area`, `wants_pointer_input`, `is_using_pointer`, `is_popup_open`) are false.

**Source lines**

- Game: `crates/game/src/battle_view/camera.rs:272`, `:356`, `:498-504`, `:516-546`, `:788-799`; `crates/game/src/battle_view/hud.rs:169`, `:174-179`, `:909-915`, `:963-975`.
- bevy_egui 0.43.1: `src/lib.rs:1145-1160`, `:1899-1901`; `src/input.rs:1716-1735`.
- egui 0.36.2: `src/context.rs:508-518`, `:816-819`, `:2943-2976`; `src/memory/mod.rs:559-561`; `src/containers/panel.rs:852-865`; `src/containers/popup.rs:566`.
- Bevy 0.20.1: `bevy_input-0.20.1/src/mouse.rs:235-246`; `bevy_camera_controller-0.20.1/src/free_camera.rs:341-349, 400-407`.

**Reporter's screenshots, viewed and consistent**

- `lens_spieler/runs/b_select/b02_after_click_ship.png` to `b03_after_click_namen.png`: "Namen" becomes highlighted while the selection brackets and the inspector disappear.
- `lens_spieler/runs/e_live/e05_g_fight.png` to `e06_after_click_topbar_empty.png`: game paused at 00:52 in both; after a click at (600,11) on an empty part of the bar the "Horst · Träger" inspector and its target line are gone.
- The reporter's driver (`lens_spieler/tools/xdrive.py`) moves the pointer and waits 0.5 s before pressing, so the fall-through is not an artefact of a teleporting pointer.

The repository was not modified (`git status` clean).

### anforderungen 3 · bestätigt · major · Inspector shows only 9 module rows; the rest sits behind a scroll bar that is invisible until hovered

Ort: `crates/game/src/battle_view/hud.rs:450`

**Beschreibung.** `list_height = (height - 710).clamp(60, 190)` caps the module list at 190 px = 9 rows at every window height from 900 px upward (also on 1080p/1440p, where there is free room). The designs have 22 (Bastion), 18 (Horst) and 14 (Speer) modules, all listed with identical names per type. egui's default scroll style is `floating` with `dormant_handle_opacity: 0.0` (egui-0.36.2/src/style.rs:643-652) and `apply_style` does not change it, so nothing indicates that the list continues. The scroll offset is also kept when another ship is selected, so later selections start in the middle of the list. The module table is the only place where a failed module is shown to the player (nothing on the hull, no hint in the side panel or on the marker), so the requirement "modules that fail individually must be visible" is met only for the first 9 rows.

**Fehlerszenario.** Select a Bastion: rows end at the second "Schwere Schienenkanone"; both lances, both lasers, both shield generators, four armour modules and the magazine are below the fold, no scroll bar visible ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/gross_2.png ). On a Speer the failed "Deflektor" and "Nahbereichsbatterie" (rows 11-12) only became visible after hovering the list and scrolling ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/d_live/crop_d05_07.png and /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/d_live/d08_later.png ). A player who does not guess that the table scrolls never sees that a shield generator or lance has failed.

**Vorschlag.** Let the list use the available height (no fixed 190 px cap), use an always-visible scroll bar (`ScrollStyle::solid()` or `scroll_bar_visibility(AlwaysVisible)`), reset the scroll offset per ship (id salt with the ship index), and put a one-line summary above the table (e.g. "2 ausgefallen, 1 beschädigt") or sort failed and damaged modules to the top. A small marker/icon on ships with failed modules would make it visible without selecting.

**Gegenprüfung.** Confirmed, though I would rate it medium rather than major: the hidden rows are reachable with the mouse wheel, and a 2 px bar appears while the pointer is over the list.

I did not start the game. I read the code and the egui 0.36.2 sources, examined the cited screenshots (taken after the last change to hud.rs, working tree clean), and ran a small simulation-only probe from the scratch folder against the already built sim library.

What holds up:

1. **Nine-row cap.** `hud.rs:450` is `(ctx.content_rect().height() - 710.0).clamp(60.0, 190.0)`, so every window height from 900 px up gives 190 px. Grid rows are 18 px plus 3 px spacing, so exactly 9 rows fit. The default window is 1600x900 (`main.rs:42`). At 900 px there are about 150 px unused between the red side panel (ends near y=231) and the inspector (starts near y=381); at 1080 px all 22 Bastion rows would fit.

2. **Module counts.** `assets/data/designs.ron` gives Speer and Speer II 14, Horst 18, Bastion 22 (Wächter has exactly 9). On a Bastion the default view shows 3 drives, 4 reactors and 2 heavy railguns; the other 2 heavy railguns, both lances, both lasers, both shield generators, 4 armour modules and the magazine are below the fold.

3. **No visible scroll cue.** `apply_style` (`ui/panels.rs:57-63`) sets only visuals, and nothing else in the game crate or bevy_egui touches the scroll style. The egui default is `ScrollStyle::floating()` with dormant handle and background opacity 0.0, so the bar is fully transparent until the pointer is over the list. The list also ends exactly on a row boundary, so no cut-off row hints at more.

4. **One correction to the finding.** "Nothing indicates that the list continues" is not literally true: egui 0.36 paints a bottom fade (`ScrollFadeStyle`, strength 0.5, size 20). In `gross_2.png` it dims the last row by only about 5 % (text 103,188,113 against 110,200,120), which is not perceptible.

5. **Shared scroll offset.** The `ScrollArea` at `hud.rs:577` has no `id_salt`, so its state id depends only on the window "schiff", not on the selected ship. The offset carries over between ships and is only clamped when the next ship's list is shorter.

6. **Only place failures are shown.** In the game crate `modules` is read only in the inspector (and copied in `feed.rs`). The 3D scene knows only the looks Active, Routing and Disabled; markers and side panels show no module state. The help text says the mouse wheel changes flight speed and that a click shows "jedes Modul".

7. **It matters in play.** The probe ran the game's own practice presets through the real battle calculator. Between 44 % and 69 % of module failures on ships with more than 9 modules land in rows 10 and below. In 10-20 % of the moments when such a ship has any damaged or failed module, the first nine rows all read "intakt".

Two points for whoever fixes it:

- `scroll_bar_visibility(AlwaysVisible)` alone will not help. With the floating style the handle opacity still interpolates from 0.0 when not hovered; the scroll style itself has to change, e.g. `ui.spacing_mut().scroll = ScrollStyle::solid()` or `thin()` before the `ScrollArea`.
- Lifting the 190 px cap alone is not enough at the default 900 px window. About 13 rows fit there and the Bastion has 22, so a visible bar plus a summary line, or sorting failed modules to the top, is still needed.

Nothing in this report comes from memory; the memory index was empty.

**Belege der Gegenprüfung.** Game code (/home/user/exp_bevy_stellaris_fable_onePrompt):
- crates/game/src/battle_view/hud.rs:450 `let list_height = (ctx.content_rect().height() - 710.0).clamp(60.0, 190.0);`
- hud.rs:577-580 `egui::ScrollArea::vertical().max_height(list_height).auto_shrink([false, true])` with no id_salt; hud.rs:585-609 rows show only `module.name`, bar and condition word.
- crates/game/src/ui/panels.rs:57-63 `apply_style` sets visuals only; panels.rs:724 ("Mausrad" = "Fluggeschwindigkeit ändern"); panels.rs:771 ("... und jedes Modul.").
- crates/game/src/main.rs:42 default window 1600x900.
- crates/game/src/battle_view/scene.rs:298-313 `Look` has only Active, Routing, Disabled.
- assets/data/designs.ron: Wächter 9, Speer 14, Speer II 14, Horst 18, Bastion 22 modules.
- crates/sim/src/battle/step.rs:894-934 `damage_module` weights (frontal hits: weapons 1.6, reactors and shields 0.8, drives 0.6, armour 0).

Engine sources (/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.36.2):
- src/style.rs:585-589 `Default for ScrollStyle` = `floating()`; 643-653 floating sets `dormant_background_opacity: 0.0`, `dormant_handle_opacity: 0.0`; 1474 `Spacing::default()` uses `scroll: Default::default()`; 795-801 `ScrollFadeStyle` default strength 0.5, size 20.
- src/containers/scroll_area.rs:733-734 id = `ui.make_persistent_id("scroll_area")`; 745-749 `ScrollBarVisibility` only decides whether bars exist; 1469-1485 floating handle opacity = lerp(dormant..active) by hover; 1564-1609 bottom fade.
- src/ui.rs:884-886 `make_persistent_id` = `self.id.with(salt)`; 251-258 child ui stable id does not depend on preceding widgets.

Screenshots from the earlier reviewer:
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/gross_2.png: Bastion selected at 1600x900, exactly 9 module rows ending at the second "Schwere Schienenkanone", no scroll bar.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/d_live/crop_d05_07.png: thin bar appears only with the pointer over the list; rows 10-14 appear only after wheel scrolling.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/d_live/d08_later.png: Speer with "Nahbereichsbatterie" and "Deflektor" ausgefallen in rows 11-12, visible only because the list had been scrolled.

My own files:
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_modlist/gross2_modules_x3.png (3x crop of the list used for the fade measurement)
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_modlist/probe/probe.rs (probe source, compiled with rustc 1.97.1 against target/release/deps/libsim-a6ffa25487c0586b.rlib; nothing written to the repository)
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_modlist/probe/run.log (probe output)

Probe results, ships with more than 9 modules, sampled every 10 ticks while on the field:

| Preset | Seeds | Failures rows 1-9 | Failures rows 10+ | Share hidden | Samples with any damage | First nine rows all "intakt" |
|---|---|---|---|---|---|---|
| Linie gegen Linie | 1-8 | 107 | 84 | 44 % | 2497 | 494 (20 %) |
| Träger gegen Linie | 1-8 | 16 | 35 | 69 % | 470 | 49 (10 %) |
| Test MIXED | 1-8 | 34 | 37 | 52 % | 1558 | 210 (13 %) |
| Großgefecht | 1-2 | 29 | 51 | 64 % | 1881 | 254 (14 %) |

### anforderungen 4 · bestätigt · major · Setup dialog accepts any ship count: overflow crash while typing, zero-ship battles, hang with large fleets

Ort: `crates/game/src/battle_view/practice.rs:459`

**Beschreibung.** `fleet_summary` multiplies `stats.fighters * count` in u32 every frame while the text field is edited (practice.rs:456-460); the dev profile has overflow checks, so a large number panics. Neither `PracticeSetup::resolve` (practice.rs:111-128) nor the dialog bounds the counts: `dolch:0` is accepted ("0 Schiffe, Bauwert 0", button enabled) and counts in the tens of thousands are passed straight to `matchup::build`. The same strings are accepted from `--fleet-a/--fleet-b`.

**Fehlerszenario.** Main menu, "Gefechtsübung", type `horst:200000000` into the blue fleet field: the game exits with code 101, "attempt to multiply with overflow" at practice.rs:459 ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/h_dialog/game.log ). `dolch:0` starts a battle with an empty blue side that ends after one second with "Roter Verband behauptet das Feld" ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/h_dialog/h03_zero_running.png ). `--practice --fleet-a dolch:20000 --fleet-b dolch:20000 --selftest 130` made no progress at all within 150 s (killed by timeout, no photo, no SELBSTTEST line: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/run_viele.log ).

**Vorschlag.** Validate in `resolve` (so dialog and command line share it): at least one ship per side, a documented upper limit per side (the tested size is about 250 ships plus fighters; e.g. 1000), error text in German next to the field; compute the summary with u64/saturating arithmetic.

**Gegenprüfung.** I tried to refute all three parts and could not; the problem exists in the current tree (HEAD 8206ed1, clean) and is not handled anywhere else.

1) Crash while typing (confirmed, the decisive part). crates/game/src/battle_view/practice.rs:456-460 computes `data.designs[..].stats.fighters * count` in u32 (`fighters: u32` in crates/sim/src/data/ships.rs:274, count is u32 from Side::parse). `fleet_summary` is called every frame for both text fields while the dialog is open (practice.rs:512), not only on "Gefecht beginnen". `Side::parse` (crates/sim/src/battle/matchup.rs:36-70) accepts any u32. The carrier `horst` has 24 fighters, so any count >= 178,956,971 overflows (24 * 200,000,000 = 4.8e9 > u32::MAX). The root Cargo.toml only sets `opt-level` for [profile.dev]; overflow checks stay on, and README.md line 12 documents plain `cargo run` (dev profile) as the way to start the game. So the documented build panics in the UI system and the process exits. In a release build it would instead show a wrapped number (505,032,704 Bordjaeger). `side.ship_count()` (u32 `.sum()`, called at practice.rs:463) and the fighters `.sum()` can overflow the same way with two entries.

2) Zero-ship side (confirmed). For `dolch:0` Side::parse pushes `(design, 0)`, so the `ships.is_empty()` check ("eine Partei braucht mindestens ein Schiff") is bypassed; `PracticeSetup::resolve` (practice.rs:111-128) adds no check, the button is enabled, and `check_end` (crates/sim/src/battle/step.rs:1098ff) ends the battle at the first check after 1 s. The existing test `setup_reports_what_is_wrong` shows an empty side is meant to be rejected, so this is an unintended hole, though only a minor one.

3) No upper bound (confirmed in substance; "hang" means a freeze of tens of seconds to minutes, not an infinite loop). `Side::specs` materialises one ShipSpec per ship and `add_combatant` one BattleShip, `feed.begin` clones the battle again; nothing between the text field/--fleet-a/--fleet-b and `matchup::build` limits the count. I measured the simulation core alone, without display, using the already built target/release/headless (sim battle code unchanged since that build): 1000 vs 1000 corvettes about 1 ms per tick, 3000 vs 3000 about 6 ms, but 10000 vs 10000 needs about 15 s for the second tick, and 20000 vs 20000 did not finish the second tick within 40 s. The game runs these ticks inside a frame (FixedUpdate), so the window is frozen that long. With 8-digit counts (the dialog shows "20000000 Schiffe" with the button enabled) building the battle needs many GB of memory.

Severity: I would rate it medium rather than major, because every scenario needs an out-of-range input (a 9-digit carrier count for the panic, five-digit counts for the freeze). But it is a hard crash of the whole game from merely typing or pasting into a main-menu text field, plus a validation gap that the code visibly intended to close, and the fix (validate counts in `resolve`, use u64/saturating arithmetic in the summary) is small. No information from memory was used (memory store is empty).

**Belege der Gegenprüfung.** Code: /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/practice.rs:459 `.map(|(design, count)| data.designs[*design as usize].stats.fighters * count)` (u32 * u32, called per frame from line 512); practice.rs:111-128 `resolve` has no count check; /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/battle/matchup.rs:45-49 parses the count as unbounded u32 and line 66 only rejects an empty list, so `dolch:0` passes; matchup.rs:107-109 `ship_count` sums u32; /home/user/exp_bevy_stellaris_fable_onePrompt/Cargo.toml:50-58 dev profile overrides only opt-level (overflow checks on); README.md:12 start command is `cargo run`.
Data: `target/release/headless battle --designs` lists horst with 24 fighters; smallest overflowing count is 178956971.
Existing run evidence I read (did not start the game): /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/h_dialog/game.log line 25-26: "panicked at crates/game/src/battle_view/practice.rs:459:32: attempt to multiply with overflow" (column 32 matches `data.designs` in the current file); driver.log in the same folder: after typing the ninth digit "GAME PROCESS ENDED", game_exit=101; h01_zero_typed.png shows "0 Schiffe, Bauwert 0" with enabled start button; h03_zero_running.png shows "Roter Verband behauptet das Feld. Dauer 00:01"; h04_before_overflow.png shows "20000000 Schiffe ... dazu 480000000 Bordjäger" with enabled start button.
My own sim-only timings (no display, prebuilt release headless tool), logs in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/verify_shipcount/ : sim_1000.log (whole fight 0.43 s), sim_3000.log (2.7 s), sim_ts_6000.log (second tick 3.3 s), sim_ts_10000.log (second tick about 15 s), sim_20000.log (second tick not finished after 40 s, timeout). Side observation from the same traces, not part of the finding: with dolch:6000 per side only about 367 ships per side remain active after three ticks, so very large line formations are also degenerate as battles.

### anforderungen 5 · bestätigt · minor · Numpad view keys do not do what the help says; all three turn the camera away from the battle

Ort: `crates/game/src/ui/panels.rs:726`

**Beschreibung.** The help line "Ziffernblock 7 / 1 / 3 – Blick von oben / von vorn / von der Seite (mit Strg: Gegenseite)" documents the engine's default snap keys, which the view keeps via `..default()` (camera.rs:310-316). In bevy_camera_controller-0.20.1/src/free_camera.rs:421-441 they only rotate the camera in place: Numpad 7 turns the view direction to +Y (straight up), Numpad 1 to -Z, Numpad 3 to +X, level with the plane. The test `keys_do_not_collide` only checks that the key names are unique and listed.

**Fehlerszenario.** From the standard overview press Numpad 7: the camera looks straight up, the screen shows only stars ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/b_select/b05_after_numpad7.png ). Ctrl+Numpad 7 looks straight down but from the overview position, which is south of the field, so only a piece of the rim ring is visible ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/b_select/b06_after_ctrl_numpad7.png ). Numpad 1 and 3 show the horizon or empty sky ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/b_select/b07_after_numpad1.png , /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/b_select/b08_after_numpad3.png ). The player has to press U to find the battle again.

**Vorschlag.** Either bind the engine's three axis keys to unused keys and drop the help line, or implement own snaps as `CameraRequest`s that place the camera above / in front of / beside the fight centre (or the selected ship) and look at it, then correct the help text and add a test that the target is on screen afterwards.

**Gegenprüfung.** I tried to refute this and could not; the finding is correct as a minor but real defect (misdocumented control that makes the player lose the battle from view).

What I checked (reading only, game not started):

1. The help line exists and is shown to the player. /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/panels.rs:725-728 has ("Ziffernblock 7 / 1 / 3", "Blick von oben / von vorn / von der Seite (mit Strg: Gegenseite)"); it is part of BATTLE_KEYS, drawn by help_window(..., battle = true) from /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/hud.rs:926-927 (F1 or the "Hilfe" button).

2. The game has no own implementation behind that line. A grep over crates/ for Numpad/axis_top/axis_front/axis_right/key_snap_reverse finds only the help string, the collision test and the +/- time keys. camera.rs:310-316 builds FreeCamera { walk_speed, run_speed, friction, scroll_factor, ..default() }, so the engine defaults stay bound: axis_top Numpad7, axis_right Numpad3, axis_front Numpad1, key_snap_reverse ControlLeft (bevy_camera_controller-0.20.1/src/free_camera.rs:151-154). Cargo.lock pins bevy_camera_controller 0.20.1, feature free_camera is enabled.

3. What the engine really does (free_camera.rs:418-453, rotate_freecam_to 462-483): on just_pressed it only builds a rotation curve from the current rotation to Transform::default().looking_to(dir, up).rotation. The translation is never touched. Numpad7 without Ctrl uses (Dir3::Y, Dir3::Z); with bevy_transform-0.20.1 look_to (transform.rs:475-484, back = -direction) the camera's forward becomes +Y, i.e. it looks straight UP. Ctrl+Numpad7 is NEG_Y (straight down). Numpad1 is NEG_Z, Numpad3 is +X, both level with the battle plane. So for Numpad 7 the help is even inverted ("Blick von oben" is only what Ctrl+7 does), and none of the keys is a view "from" anywhere because the camera stays where it is.

4. Nothing in the game cancels or corrects this: tame_free_camera only clamps speed/position and sets flight.enabled = !egui_wants_keyboard (so the controller is enabled in the view), follow_selected only translates, sync_flight clears rotation_curve only on camera requests. The engine system runs in RunFixedMainLoop every frame, also while paused.

5. The failure scenario holds geometrically. Overview pose is (0, 0.95e, 1.3e) (camera.rs:200-203), vertical FOV 50 degrees, 16:9. I computed, with bevy's look_to semantics, how much of the disc in which ships can be (radius (e-120)/1.12, from overview_extent) is on screen after each snap, for e = 650, 1000, 1750, 2400: Numpad7 0%, Ctrl+Numpad7 0% (the footprint straight below the camera starts at z = 0.857e, outside the ship disc), Numpad1 0-1.6% (only the far rim at the bottom edge), Ctrl+Numpad1 0%, Numpad3 0%, Ctrl+Numpad3 0%; the field centre and both spawn anchors (x = +-1300) are never visible. The same holds from the G pose (pitch -0.7) and the F pose (pitch -0.42; at best the ship sits at 96% of the way to the bottom edge after Numpad1 with yaw 0).

6. The existing screenshots of the reporting run (keys injected as X events KP_7 / Control_L+KP_7 / KP_1 / KP_3, each after pressing U) agree with the code reading: b04 shows the help line, b05 (Numpad 7) only stars, b06 (Ctrl+Numpad 7) only arcs of the rim rings and no ship, b07 (Numpad 1) horizon with the rim arc at the bottom edge, b08 (Numpad 3) only stars. That b05 shows no ring while b06 does confirms that unmodified Numpad 7 looks up.

7. Not covered by tests and not handled elsewhere: keys_do_not_collide (camera.rs:965-1044) only asserts that key names are distinct and that the six own keys plus a few words appear in the help; README says nothing about these keys; working tree is clean at HEAD 8206ed1.

Why it matters (and why only minor): the in-game help is the only documentation of the battle controls, and one documented control does the opposite of / something different from what is written and leaves the player looking at empty space with no hint where the battle is. This is a user-visible functional mismatch, not a cosmetic one. It is minor because the free camera itself works and the player can recover with U / G / F, the top-bar buttons "Übersicht" / "Gefecht", or mouse look (the finding's "has to press U" is slightly too narrow). The suggested fix (own snaps as CameraRequests that place the camera above / in front of / beside the fight centre or selected ship, or rebinding the engine keys away and dropping the help line, plus a test that the target is on screen) is appropriate; note FreeCamera's key fields are plain KeyCode, so the engine snaps cannot be switched off, only rebound.

**Belege der Gegenprüfung.** Help text: /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/ui/panels.rs:725-728 ("Ziffernblock 7 / 1 / 3" -> "Blick von oben / von vorn / von der Seite (mit Strg: Gegenseite)"), shown via /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/hud.rs:926-927.
Engine defaults kept: /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/camera.rs:310-316 (FreeCamera { walk_speed, run_speed, friction, scroll_factor, ..default() }); defaults in /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_camera_controller-0.20.1/src/free_camera.rs:151-154 (key_snap_reverse ControlLeft, axis_top Numpad7, axis_right Numpad3, axis_front Numpad1).
Engine behaviour: free_camera.rs:418-453 — axis_front -> (Dir3::NEG_Z, Dir3::Y) / Ctrl (Dir3::Z, Dir3::Y); axis_right -> (Dir3::X, Dir3::Y) / Ctrl (Dir3::NEG_X, Dir3::Y); axis_top -> (Dir3::Y, Dir3::Z) / Ctrl (Dir3::NEG_Y, Dir3::NEG_Z); then `let target = Transform::default().looking_to(dir, up).rotation;` and only state.rotation_curve is set; rotate_freecam_to (462-483) writes only transform.rotation. look_to in /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_transform-0.20.1/src/components/transform.rs:475-484 sets back = -direction, so forward = direction: Numpad 7 looks toward +Y (up).
No own numpad handling: grep for Numpad|axis_top|axis_front|axis_right|key_snap_reverse|Ziffernblock in crates/ hits only panels.rs:726, camera.rs:989-992 (test list), camera.rs:1003-1004 and time_control.rs:69/72 (NumpadAdd/NumpadSubtract).
Overview pose: camera.rs:200-203 (eye (0, 0.95e, 1.3e), target (0, 0, 0.06e)); FOV camera.rs:18 (50 degrees); overview_extent camera.rs:207-215; field data /home/user/exp_bevy_stellaris_fable_onePrompt/assets/data/balance.ron:241-242 (spawn_radius 1300, exit_radius 2400).
Own calculation (inline python, bevy look_to semantics, 1600x900): share of the possible ship area on screen after a snap from the overview pose, e in {650, 1000, 1750, 2400}: Numpad7 0.0%, Ctrl+Numpad7 0.0%, Numpad1 0.0/0.5/1.3/1.6%, Ctrl+Numpad1 0.0%, Numpad3 0.0%, Ctrl+Numpad3 0.0%; field centre and both spawn anchors never visible; the overview itself shows 99.6-100%.
Existing screenshots viewed (not produced by me): /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/b_select/b04_help.png (help line), b05_after_numpad7.png (stars only), b06_after_ctrl_numpad7.png (rim ring arcs only), b07_after_numpad1.png (horizon, rim arc at bottom edge), b08_after_numpad3.png (stars only); key injection order in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/b_select/driver.log; the game log of that run prints the engine bindings "[ControlLeft + ]Numpad7 - Snap to Up (+Y)/Down (-Y)" etc.
Test gap: camera.rs:965-1044 (keys_do_not_collide) checks only distinct key names and that own keys and the words "W A S D", "E / Q", "Maustaste", "Mausrad", "Umschalt", "Esc" occur in the help.
Nothing from memory was used; the memory index for this session was empty.

### anforderungen 6 · bestätigt · minor · "Neu starten" does not start paused at 00:00 when the clock was running

Ort: `crates/game/src/battle_view/practice.rs:381`

**Beschreibung.** The Restart branch only sets `*speed = SimSpeed::default()`. The virtual clock is paused later by `time_control::apply_speed`, which is not ordered against `handle_view_requests`; one more frame of FixedUpdate runs with the old speed (up to `max_delta`: 0.25 s at <=5x, 1.25 s at 25x, i.e. up to 25 battle ticks). The Leave branch two lines above pauses the clock directly for exactly this reason.

**Fehlerszenario.** Let a battle run to the end at 25x, click "Neu starten": the new battle shows "Gefechtszeit 00:02 PAUSIERT" instead of "00:00 … PAUSIERT – Leertaste beginnt das Gefecht"; the fleets have already moved and orders "before the first tick" are no longer possible ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/d_live/d20_restarted.png ).

**Vorschlag.** Take `ResMut<Time<Virtual>>` (already a parameter) and call `time.pause()` in the Restart branch as in Leave; add a system test that Restart at an unpaused clock leaves `battle.tick == 0` after the next FixedUpdate.

**Gegenprüfung.** The finding holds, as a minor but reproducible defect. I could not refute it. I did not start the game; this rests on reading the code and engine sources, plus screenshots other reviewers had already saved. Nothing came from memory.

**Mechanism**
1. "Neu starten" only exists in the Bilanz window of a finished battle (hud.rs:700-707). Nothing pauses the clock when a battle ends, so on the normal path the virtual clock is still running when the button is clicked.
2. The HUD runs in `EguiPrimaryContextPass`, which bevy_egui runs from `PostUpdate`. The `Restart` message is therefore handled in `Update` of the next frame.
3. The Restart branch (practice.rs:381-386) builds the new battle and sets only `*speed = SimSpeed::default()`. It does not touch `Time<Virtual>`, although the system already has `ResMut<Time<Virtual>>`. The Leave branch (374-380) calls `time.pause()` with a comment explaining why.
4. The clock is paused only by `time_control::apply_speed`, and only when `speed.is_changed()` (time_control.rs:94-109). `apply_speed` is chained after `keyboard` only; `handle_view_requests` is chained after practice `keys` only. There is no ordering between them, and they conflict on `SimSpeed` and `Time<Virtual>`.
5. If `apply_speed` runs first in that frame, it sees no change. In the following frame, `time_system` (in `First`) advances the unpaused clock by up to `max_delta`, and `run_fixed_main_schedule` runs `FixedUpdate` once per 0.05 s. `practice::step` (420-438) then advances the new battle before `apply_speed` finally pauses.
6. If `handle_view_requests` runs first, the pause lands in the same frame and no tick runs. So the bug depends entirely on an unspecified system order.

**Size of the effect**
- `max_delta` is `max(factor*0.05, 0.25)` s, so one frame can run up to 5 ticks at ≤5×, 10 at 10× and 25 at 25×.
- Battle `dt` is 0.1 s, so 25 ticks is 2.5 s, shown as "00:02".
- The start hint "PAUSIERT – Leertaste beginnt das Gefecht" needs `battle.tick == 0` (hud.rs:226-232), so any slipped tick removes it.

**Which order occurs in practice**
Bevy does not guarantee the order of unordered systems: the multi-threaded executor spawns ready systems in index order, skipping any that conflict with a running system. Here `handle_view_requests` needs exclusive `BattleFeed` access and so conflicts with every view system reading the feed, while `apply_speed` conflicts with almost nothing. I did not derive the exact order by hand. An earlier standalone reproduction in the review folder (`verify_restart_pause`, not mine) shows the same engine behaviour: 25 ticks when `apply_speed` runs first, 0 when it runs second or when `time.pause()` is added.

The saved screenshots from the current build show the bad order every time: 7 restarts in 4 sessions, none with the start hint. The first start does show it.

**Why it matters**
- The restarted battle is not at its beginning when the player gets control, contrary to the stated intent ("Wie eine Partie beginnt die Übung pausiert", tooltip "Dasselbe Gefecht noch einmal von vorn").
- Orders can no longer be given before the first tick.
- The number of slipped ticks depends on speed level and frame time, so the same inputs after a restart do not give the same course on every machine.
- No test covers `handle_view_requests`; the practice tests only run `step` and `apply_orders`.

The suggested fix (`time.pause()` in the Restart branch, as in Leave) is correct and sufficient: the clock is then paused before the next `First`, and `apply_speed` still resets speed and `max_delta` afterwards. Severity stays minor: at most 2.5 s of approach, no crash, no corruption.

**Belege der Gegenprüfung.** **Game code** (`/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/`)
- `battle_view/practice.rs:381-386` — Restart branch: `*speed = SimSpeed::default();` only, no `time.pause()`.
- `battle_view/practice.rs:374-380` — Leave branch: `time.pause()` with the comment "Die Zeit hält sofort an, nicht erst beim Zustandswechsel im nächsten Frame".
- `battle_view/practice.rs:259-268` — `(keys, handle_view_requests, apply_orders).chain()` in `Update`, no ordering against time_control.
- `battle_view/practice.rs:420-438` — `step` advances the battle on every `FixedUpdate` while it is not finished.
- `time_control.rs:51-61` — `(keyboard, apply_speed).chain()` in `Update`, no set, no ordering against practice.
- `time_control.rs:94-109` — `apply_speed` returns early unless `speed.is_changed()`.
- `time_control.rs:112-114` — `max_delta_seconds = (factor * 0.05).max(0.25)`.
- `battle_view/hud.rs:226-232` — start hint only when `battle.tick == 0`.
- `battle_view/hud.rs:700-707` — "Neu starten" only in the summary of a finished battle.
- `selftest.rs:196-209` — `hold` pauses `Time<Virtual>` directly, with a comment that setting `SimSpeed` alone takes effect a frame late.
- `/home/user/exp_bevy_stellaris_fable_onePrompt/assets/data/balance.ron:238` — `dt: 0.1`; `crates/sim/src/clock.rs:7` — `TICKS_PER_SECOND = 20`.

**Engine sources** (`/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`)
- `bevy_egui-0.43.1/src/lib.rs:1145-1150` — `run_egui_context_pass_loop_system` is added to `PostUpdate`.
- `bevy_app-0.20.1/src/main_schedule.rs:221-235` — order First, PreUpdate, RunFixedMainLoop, Update, SpawnScene, PostUpdate, Last.
- `bevy_time-0.20.1/src/lib.rs:83-93` — `time_system` in `First`, `run_fixed_main_schedule` in `RunFixedMainLoop`.
- `bevy_time-0.20.1/src/virt.rs` (`advance_with_raw_delta`) — speed is 0 only if `paused` is already set; delta is clamped to `max_delta`.
- `bevy_time-0.20.1/src/fixed.rs:243-258` — accumulates the virtual delta and runs `FixedMain` once per timestep.
- `bevy_ecs-0.20.1/src/schedule/executor/multi_threaded.rs` (`spawn_system_tasks`, `can_run`) — ready systems are spawned in index order and skipped while a conflicting system runs.
- `bevy_ecs-0.20.1/src/schedule/graph/graph_map.rs:124-126` — `remove_node` uses `swap_remove` during set flattening, so the index order of unrelated systems is arbitrary.

**Existing screenshots, current build** (under `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/`; HEAD 8206ed1, working tree clean)

| File | Situation | Top bar |
|---|---|---|
| `lens_spieler/runs/d_live/d19_bilanz_again.png` | before restart | "01:25 BEENDET", 25× active, not paused |
| `lens_spieler/runs/d_live/d20_restarted.png` | after restart from 25× | "00:02 PAUSIERT", no hint |
| `lens_spieler/runs/g_live/g04_restarted.png` | after restart from 25× | "00:02 PAUSIERT", no hint |
| `engine_perf/runs/e3/bar_b_restarted_from_25x.png` | after restart from 25× | "00:02 PAUSIERT", no hint |
| `engine_perf/runs/e3/bar_d_restarted_from_1x.png` | after restart from 1× | "00:00 PAUSIERT", no hint |
| `runs/s10/bar_r_01_restart_at_1x.png` | after restart from 1× | "00:00 PAUSIERT", no hint |
| `runs/s10/bar_r_03_restart_at_25x.png` | after restart from 25× | "00:02 PAUSIERT", no hint |
| `runs/s10/bar_r_04_restart_at_25x_again.png` | after restart from 25× | "00:02 PAUSIERT", no hint |
| `runs/s10/bar_o_01_keil_defensiv_paused.png` | first start | "00:00 PAUSIERT – Leertaste beginnt das Gefecht" |

**Earlier standalone reproduction (not mine)**
- `verify_restart_pause/run.log` in the same folder: `apply_speed` first gives tick 25 / "00:02" at 25× and 3 fps; `handle_view_requests` first, or `time.pause()` in the Restart branch, gives tick 0 in every case.

### anforderungen 7 · widerlegt · minor · Weapon effects are anchored to tick positions and to the hull centre, so beams miss the ships they hit

Ort: `crates/game/src/battle_view/effects.rs:198`

**Beschreibung.** Ships are drawn at interpolated poses (mod.rs:118-131), but a beam is drawn from `beam.from` to `beam.to` as stored at its creation tick (effects.rs:200-201) for its whole life of 1-3 ticks. The end points are therefore up to one tick of travel away from the drawn shooter and target: 7.4 units for a corvette (hull length 16), 11.5 units for a fighter (hull length 6). In pause the offset is frozen. In addition every beam, tracer and missile starts at the exact hull centre at plane height and beams end in the target's centre; the turrets, casemates and lance barrels that the meshes model are never the origin, so a broadside is a bundle of tracers leaving the middle of the deck (there is no muzzle effect either).

**Fehlerszenario.** Close-up of the default battle at 00:33 (paused): the cruiser's laser leaves the middle of its deck and ends in empty space about half a ship length beside the corvette whose shield lights up ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/gefecht_2.png , enlarged: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/crop_gefecht_2_target.png ). Against fighters a "hit" passes up to two hull lengths away from the fighter.

**Vorschlag.** When a beam is first seen, match `beam.from`/`beam.to` (for hits) against the ship positions of that tick (selftest.rs already does this for the shooter) and remember the two ship indices; draw the beam between the interpolated poses, offset to a muzzle point of the class model. Longer term carry shooter/target indices in the core's `Beam` (the implementer's wish no. 3).

**Gegenprüfung.** Verdict: not a real problem. The cited failure scenario is a misreading, and what remains of the mechanism is cosmetic.

**1. The screenshot shows a miss, drawn as designed.**
I replayed the battle headless (scratch tool outside the repo, linked against the already built sim library; game not started, repo untouched).
- `gefecht_2.png` is the default fleets with seed 3 at tick 330. All twelve HUD numbers match, as do the selected Speer's ammo 273, morale 68.7 and target Dolch.
- The beam the selftest framed has `hit = false`. Its `to` is the miss point 20.7 units beside the corvette (`step.rs:562-568`).
- `effects.rs:208-211` deliberately lengthens a miss by 0.3·reach + 90 = 177 units ("Ein Fehlschuss … geht daran vorbei").
- Projecting that lengthened end through the selftest camera gives pixel (958.0, 333.5); the screenshot measures (958.1, 333.1).
- "Whose shield lights up" is impossible: the Dolch has no shield module. The glow is a kinetic impact flash next to a neighbouring corvette.

**2. The tick anchoring exists, but hit beams do not leave the hull, except on fighters.**
- Beam ends are tick-N positions and ships are interpolated, so the end is (1−alpha)·v·dt ahead in the creation tick and alpha·v·dt behind in the next.
- Because `ship.vel = from_angle(heading) * speed` (`step.rs:283-284`), that offset lies on the hull's long axis.
- Over full replays of three presets, the largest per-tick travel of a hit target was always below the hull half-length:

| Class | Max travel per tick | Hull half-length |
|---|---|---|
| Corvette | 7.40 | 8 |
| Frigate | 5.60 | 12 |
| Cruiser | 4.30 | 20 |
| Battleship | 3.00 | 32 |
| Carrier | 2.80 | 36 |
| Fighter | 11.50 | 3 |

- `x_2.png` is a real hit on a corvette at alpha ≈ 0, the worst case; the beam ends exactly at the bow tip.
- Only fighters can have a hit laser end off the hull, by up to 8.5 units beyond nose or tail, during a 2-tick (0.2 s) afterglow. That happened 37 times in the whole carrier preset and 108 times among 3739 beams in the Großgefecht.
- Point-defence tracers are unaffected: the piece reaches `to` at alpha = 1, when the target does.

**3. Hull-centre origin comes from the core, not the view.**
- `fire_weapons` uses `origin = ship.pos` for every weapon (`step.rs:510-523`), and `Shot`/`Beam` carry no shooter or module (`mod.rs:296-331`). The game crate only displays.
- Beam and tracer materials are opaque, so they emerge from the hull surface, as in `gefecht_2.png` and `gross_2.png`.
- The spec asks for visible staging of broadsides and weapons, which is there. Muzzle-accurate origins and muzzle flashes are polish.

The finder's numbers (7.4 and 11.5 units per tick, offset frozen in pause) are correct; the conclusion "beams miss the ships they hit" does not follow for anything but fighters.

Incidental, outside this finding: heavy beams live 4 ticks, not 3, because 0.3 − 3×0.1 leaves 1.49e-8 in f32. In the fourth tick they are drawn at the minimum width.

Nothing in this report came from memory; the memory index was empty.

**Belege der Gegenprüfung.** Code read:
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/effects.rs:198-238 (beam drawn from beam.from/to; miss extension at 208-211; PD piece at 216-221)
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/mod.rs:118-131 and feed.rs:199-209 (interpolated poses)
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/battle/step.rs:74-77 (move, then fire), 283-284 (vel along heading), 510-523 (origin = ship.pos), 562-577 (hit/miss end point, ttl)
- /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/selftest.rs:228-273, 415-424 (which beam the close-up frames, camera placement)
- /home/user/exp_bevy_stellaris_fable_onePrompt/assets/data/designs.ron (dolch has no shield module), hulls.ron (speeds and radii)
- bevy_time-0.20.1/src/fixed.rs:205-207, 243-258 (overstep_fraction; paused virtual delta is 0, so alpha freezes)

Replay of the finder's run (seed found by matching the log "Gefechtszeit 177,0 s … 32 Wracks" and the HUD):
- Seed 3, tick 330, t = 33.00: blue active [0,4,4,6,2,0], lost [0,0,2,…], strength 0.889, morale 64.23; red active [0,3,4,6,2,0], lost [0,1,2,…], strength 0.907, morale 66.71. HUD shows 89 % / 64 % and 91 % / 67 %.
- busy_exchange(relaxed=false) picks beam#7: Light, hit=false, reach 290.6, from (402.9,−413.4) to (186.2,−607.0). Shooter #24 is a red Speer: hull 440, shield 110, ammo 273.00, morale 68.73, target #15 Corvette at 289.8. Corvette #15 is 20.7 units from beam.to.
- Projection through the selftest camera (FOV 50°, 1600×900):
  - lengthened miss end → (958.0, 333.5); measured in gefecht_2.png (958.1, 333.1)
  - corvette #15 previous-tick position → (978.0, 367.5); measured ring centre (977.5, 367)
  - a hit would have ended at (965.2, 368.6), inside the ring at the bow
  - impact (141,−609) → (992.6, 359.7); measured glow centre about (993, 358)

Replay statistics (target travel in the creation tick of hit beams, mean / max):
- Linienschlacht seed 3: Light→Corvette n=102, 7.26 / 7.40; Light→Frigate n=130, 2.69 / 5.60; Heavy→Battleship n=51, max 1.96. End beyond hull half-length: 0 cases.
- Träger gegen Schlachtschiffe seed 1: Light→Fighter n=37, 10.03 / 11.50 (36 beyond half-length 3); all other classes 0.
- Großgefecht seed 1: 3739 fresh beams; Light→Fighter n=108, 10.42 / 11.50 (102 beyond); Light→Corvette n=305, max 7.40; Cruiser max 4.30; all other classes 0 beyond.
- Heavy beam lifetime: 4 ticks in all 118 cases (seed 21 run).

Worst-case real hit on a corvette: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/x_2.png (seed 1, tick 335, beam#14 hit=true on corvette #17, target travel 7.40). Enlarged crop, beam ends at the bow tip: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/check_beam_anchor/x2_end_8x.png

Scratch tools and logs (outside the repository; git status clean afterwards): /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/check_beam_anchor/scan/ (beamscan.rs, seedscan.rs, dump.rs, stats.rs, dump3.log, dump1.log, scan21.log) and /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/check_beam_anchor/cam.py

### anforderungen 8 · bestätigt · minor · "Übersicht" (U) does not show all ships: everything near the southern rim is below the screen

Ort: `crates/game/src/battle_view/camera.rs:200`

**Beschreibung.** `overview_transform` places the camera at (0, 0.95e, 1.3e) looking at z = 0.06e with a 50° vertical field of view. Ground points with z > about 0.8e (plane y < -0.8e) lie below the bottom edge. `overview_extent` uses e = 1.12*reach + 120, clamped to the exit radius 2400, so any ship farther than roughly 970 units from the centre on the camera side, and everything beyond 1930 units when the extent is clamped, is cut off. Tooltip and help promise "Alle Schiffe auf dem Feld im Blick". The tests only check points at ±1300 for extent 2400 and the ships of one small battle.

**Fehlerszenario.** Preset "Großgefecht" (or the 496-ship command line): the lines reach the rim, and the fighting at the southern end is cut by the bottom edge of the overview ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/gross_1.png , bottom edge enlarged: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/crop_gross_1_bottom.png ). Ships retreating southwards also leave the overview long before they escape.

**Vorschlag.** Derive distance and pitch from the bounding circle so that its nearest and farthest points are inside the frustum (and clear of the top bar), and add a test with ships at (0, -reach) for several reaches including the exit radius.

**Gegenprüfung.** I tried to refute the finding and could not; the geometry, the code and an independent headless recomputation all confirm it.

Geometry (crates/game/src/battle_view/camera.rs:200-203): overview_transform puts the eye at (0, 0.95e, 1.3e) looking at (0, 0, 0.06e), FOV 50 degrees vertical (camera.rs:18, used for the real PerspectiveProjection at camera.rs:294). Forward = (0, -0.6082, -0.7938). For a ground point at world z, depth = 1.6097e - 0.7938z and the height above the view axis = 0.0365e - 0.6082z; with tan(25 deg) = 0.4663 the bottom edge is at z = 0.8046e. The southern point of the circle of radius `extent` (which the doc comment says the view shows) lands at NDC y = -1.50, i.e. pixel 1126 of 900. to_world maps plane y to world -z (battle_view/mod.rs:32), so the camera side is plane y < 0, as the finding says.

overview_extent (camera.rs:207-215) is clamp(1.12*reach + 120, 650, exit_radius = 2400, assets/data/balance.ron:242). With the clamp everything south of z = 1931 is off-screen although ships stay in play up to 2400 (step.rs:289-293, 1086). Unclamped, a farthest ship straight south at distance d with radius r is visible only if d < 976 + 9.1r. Both numbers in the finding are right.

Nothing else compensates: apply_requests (camera.rs:445-451) uses the transform unchanged, there are no off-screen markers, and the tooltip (hud.rs:274 "Alle Schiffe auf dem Feld im Blick (U)") and the help (ui/panels.rs:729) promise all ships.

The scenario is real, not theoretical. The shipped preset "Großgefecht" (practice.rs:62-70) uses Line formation; formation.rs gives 36 heavy ships a half-width of 2078, 60 frigates 3083 and corvette wings from 3681 to 7405, around anchors at x = +-1300, so the lines reach and are clamped onto the rim, including its south pole.

The tests do not guard this: overview_shows_the_whole_field (camera.rs:698) only checks points at +-1300 for extent 2400, and overview_follows_the_fleets (camera.rs:719) only a 17-ship battle.

Caveat on scope and severity: the three smaller presets are unaffected (0 ships outside in every tick), the camera can be flown freely to see the rest, and the side claim about ships "retreating southwards" is weak for two-party practice battles, where retreat runs mostly along the x axis. It is still a reproducible wrong display of a labelled feature in exactly the large-battle case, so minor but real.

**Belege der Gegenprüfung.** 1) Existing screenshot of the selftest's Overview photo (selftest.rs:405 sends CameraRequest::Overview before photo 1): /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/gross_1.png. Field centre line at pixel y about 428 and north rim at about 191, exactly as the formula predicts (428.1 and 191.2); the south rim is not on screen, and the beams and ships of the southern melee are cut by the bottom edge, while the same melee at the north pole is fully visible.

2) Independent headless recomputation (I did not start the game and changed nothing in the repository): a scratch program using only the sim crate, re-implementing overview_extent, overview_transform and the projection with its own vector maths, 1600x900. Source: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/check_overview_south/proj/src/main.rs ; output: /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/check_overview_south/run.log
- Bottom edge at z = 0.8045 * extent for every extent (1930.9 for 2400); south point of the extent circle at pixel y = 1126 of 900.
- Großgefecht, seed 1, extent clamped to 2400: t=0 (the paused start, camera in overview) 130 of 400 in-play ships below the bottom edge (133 outside in total); t=10 s 45 of 319; t=30 s 13 of 235; t=60 s 6 of 70. In 740 of 1170 ticks at least one ship is outside the overview picture.
- Geplänkel, Linienschlacht, Träger gegen Schlachtschiffe: 0 ships outside in all ticks.

3) Code references: camera.rs:200-215 (transform and extent), camera.rs:445-451 (request handling), camera.rs:698-745 (the two tests), hud.rs:273-274 and ui/panels.rs:729 (promise), battle_view/mod.rs:32 (plane y to world -z), practice.rs:62-70 (preset), sim/src/battle/formation.rs:52-73 (line widths), sim/src/battle/step.rs:289-293 (active ships clamped to the exit radius), assets/data/balance.ron:241-242, 281 (spawn 1300, exit 2400, spacing 95).

Context outside this finding, noticed while checking: matchup::build places large Line formations far outside the field at build time (corvette slots up to y = +-7405 for the Großgefecht, some even behind the overview camera at t=0); the first tick clamps them onto the rim, which is why so many ships sit at the two poles.

### anforderungen 9 · bestätigt · minor · Wrecks and debris are close to invisible at normal viewing distances; small wrecks look like intact ships

Ort: `crates/game/src/battle_view/scene.rs:129`

**Beschreibung.** Named by the implementer as a weakness; confirmed, and it concerns the specification items "debris fields" and "capital ships breaking apart". Cooled wrecks use base colour 0.2 with emissive 0.012 (scene.rs:101-106, 129-135), debris 0.16 (136-140), a wreck gets 2-14 pieces of 7-23 % of the hull radius (639-647, 724-745), and wrecks have no marker in the HUD. Non-capital wrecks reuse the intact hull mesh with a darker material. Hot capital wrecks (two halves, orange) are visible in close views.

**Fehlerszenario.** 12 against 12 corvettes run to the end: 23 wrecks on the field, the view from the fight centre shows an empty field with four dark specks ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/g_live/g03_end.png ). 496-ship battle with 333 wrecks: the final overview shows only the survivors' markers ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/gross_4.png ). Close view: a corvette wreck differs from a live corvette only by a darker tint ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/gross_3.png ).

**Vorschlag.** Brighter wreck and debris materials (or a rim/fill light for them), more and larger pieces that spread over several hull lengths, visibly damaged small wrecks (e.g. also split or with missing parts), a dim wreck marker in the overview, and continued drift after the battle ends.

**Gegenprüfung.** Verdict: the finding is real (minor). I could not refute it. I did not start the game or build; I read the code and the Bevy 0.20.1 sources and measured the reporter's existing screenshots.

**Facts in the finding, all confirmed in the code**
- Cooled wrecks use base colour (0.2, 0.185, 0.18) with emissive (0.012, 0.004, 0.002); debris uses base 0.16 with no emissive.
- A wreck gets 2/4/6/9/14 debris pieces, each 7-23 % of the hull radius, spread within 0.7-3.1 radii of the wreck (mostly under one hull length).
- Fighter, corvette and frigate wrecks reuse the intact hull mesh; only cruisers, battleships and carriers split into halves.
- No HUD or overlay code draws wrecks: markers skip every ship that is not in play, and nothing iterates the wreck list.

**The darkness comes from the material values, not from software rendering**
- From the engine shader (exposure 2^-9.7/1.2, emissive added without exposure, 2700 lx main light, ambient 260), a lit top face of a cooled red-side wreck should be about sRGB (87, 65, 66) and a blue-side one about (71, 69, 84).
- The screenshots measure (84, 65, 65) and (75-78, 72-75, 82-85). A live hull is predicted at (151, 123, 132) and measured at (140-154, 116-127, 120-129). The model fits, so the engine API is used correctly; the chosen values are simply dark.
- The 0.012 emissive adds only about 12 % on lit faces. The code comment says a cooled wreck must not vanish against the black sky; this value does not achieve that.
- Every third debris piece is the DARK-painted block (albedo about 0.009), which reaches at most about 20/255 when fully lit against a sky of about (2, 3, 5).

**Why this matters at the game's own camera presets**
- The overview shows 1.05-3.9 world units per pixel. The fight view in the 12-against-12 screenshot sits about 1100 units away, about 1.15 units per pixel.
- A corvette wreck is therefore 4-14 px long, and its debris is 0.15-1.6 px.
- Live ships below 46 px get a marker, and shots, beams and flashes have a 1.4 px minimum "so a battle can be read from afar". Wrecks and debris have neither.
- In `g03_end.png` the four wreck blobs in the open are 30-48 px in area with peak about 84/255, dimmer than most background stars (dimmest star paint is about 112/255).
- In `gross_4.png` (333 wrecks) about 30 dim blobs are perceivable, the largest 72 px with peak about 91/255.

**Small wrecks look like intact ships**
- In `gross_3.png` the centre object is an undamaged corvette shape at (91, 71, 69), which matches the cooled-wreck prediction.
- A disabled ship that is still in play uses base 0.2 with metallic 0.25, giving about (70, 55, 58) on the same mesh. In a close view, where markers fade out, a destroyed corvette and a drifting live one differ only by a slow tumble and a few dark bits.

**Why it is not purely cosmetic**
The specification asks for visible staging of debris fields and breaking capital ships. The elements exist and read well close up while hot (the first 8 s). The lasting debris field is not perceivable from the views the game itself offers for large battles.

**Where the report overstates**
- `gross_4.png` does show faint wreck specks, not only survivor markers.
- Part of the emptiness in the 12-against-12 scenario has a second cause: the simulation lets wrecks drift at 0.6 of ship velocity with a decay time of about 50 s (up to about 2200 units for a corvette), and the overview frames only ships in play.
- Wrecks freeze once the battle is finished, because the practice mode stops stepping. This is by design of the step loop, not a defect in itself.

Memory: one stored note (cloud render check) was read and not needed for this verdict.

**Belege der Gegenprüfung.** **Game code** (under `/home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src/battle_view/`)
- `scene.rs:101-106` — wreck material: base (0.2, 0.185, 0.18), roughness 0.85.
- `scene.rs:117-121` — hull materials; the disabled look is `plating(0.2)`.
- `scene.rs:129-135` — wreck emissives; cooled is (0.012, 0.004, 0.002), with the comment that a cooled wreck would otherwise vanish against the black sky.
- `scene.rs:136-140` — debris material, base 0.16, no emissive.
- `scene.rs:639-647` — debris count 2/4/6/9/14.
- `scene.rs:709-722` — only capital classes get halves; others reuse `class.hull`.
- `scene.rs:724-745` — reach `radius*(0.7+2.4u²)`, size `radius*(0.07+0.16u)`, mesh `debris[piece % 3]`.
- `scene.rs:795-801` — heat stages at 2.5 s and 8 s.
- `meshes.rs:23-29` — paints HULL 0.56, PLATE 0.3, DARK 0.055.
- `meshes.rs:1193-1237` — the three debris meshes: shard (PLATE), plate (HULL), block (DARK).
- `hud.rs:28-33` — `MARKER_GONE` 46 px, `MARKER_FULL` 20 px.
- `hud.rs:781-784` — markers skip ships that are not in play; no wreck markers anywhere in `hud.rs` or `overlay.rs`.
- `effects.rs:34-36, 167` — `MIN_PIXELS` 1.4 for shots, beams and flashes only.
- `camera.rs:200-215` — overview transform; extent is computed from in-play ships only.
- `camera.rs:300-309` — TonyMcMapface, Bloom 0.22, ambient 260.
- `camera.rs:435-442` — fight view distance `clamp(spread*2.6, 350, 4500)`.
- `practice.rs:428` — no stepping after the battle is finished.

**Simulation** (`/home/user/exp_bevy_stellaris_fable_onePrompt/crates/sim/src/battle/step.rs`)
- `:93-97` — wreck drift, velocity times 0.998 per 0.1 s tick.
- `:1019-1027` — wreck created with 0.6 of ship velocity.

**Engine sources** (under `/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`)
- `bevy_pbr-0.20.1/src/render/pbr_functions.wesl:821-850` — output is exposure times (direct + indirect) plus emissive; emissive is scaled by exposure only via its weight, default 0.
- `bevy_camera-0.20.1/src/camera.rs:264-277` — default EV100 9.7, exposure `2^-ev/1.2`.
- `bevy_pbr-0.20.1/src/render/pbr_lighting.wesl:510-524` — Burley diffuse with 1/π.
- `bevy_pbr-0.20.1/src/render/pbr_ambient.wesl:8-27` and `render/light.rs:1819, 1848-1849` — light colour times illuminance, ambient colour times brightness.

**Screenshot measurements** (reporter's images under `/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/`)
- `runs/g_live/g03_end.png` — wreck blobs at (48,421), 46 px, peak (84,65,65); at (1006,477), 41 px, peak (75,72,82); at (352,231), peak (66,64,72); at (1484,222), peak (78,61,62). Sky is (2,3,5). The lower half holds about 75 star blobs with peaks of 55-218.
- `gross_4.png` — 44 dim blobs of at least 8 px outside the UI, about 30 of them wreck candidates; the largest is 72 px with peak (77,74,83). The run log reports 333 wrecks.
- `gross_3.png` — centre corvette wreck 90th-99th percentile (91-94, 71-73, 69-71); live red ships (140-154, 116-127, 120-129); debris bits (56-70, 56-68, 58-71) covering 23-64 px.

**My scripts and crops**
`/tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/check_wreck_visibility/` — `probe.py`, `region.py`, `blobs.py`, `wreckcount.py`, `crop.py` and the PNG crops.

### anforderungen 10 · bestätigt · major · Tests and selftest pass even if the scene shows nothing; the interactive paths are untested

Ort: `crates/game/src/selftest.rs:536`

**Beschreibung.** 1) No test runs `sync_ships`, `sync_wrecks`, `animate_wrecks`, `rebuild_on_epoch`, `show_view`, `hide_view`, `update_poses`, `draw_hud` (applying actions), `start_requested`, `handle_view_requests` (Restart/Leave), `leave` or `tame_free_camera`. If no ship entity were ever spawned, all 64 tests would still pass. 2) The HUD tests draw without input and assert only `actions.is_empty()`; no button is ever clicked (orders, retreat, restart, leave). 3) `a_click_selects_the_ship_under_the_cursor` calls `pick_on_click` directly and so bypasses the egui gate (the top-bar click-through is invisible to it). 4) `keys_do_not_collide` only compares key names. 5) hud.rs:1089-1095 asserts `x <= max(initial, x)`, which is always true. 6) The overview tests only use ±1300 and one small battle. 7) Selftest verdict is `test.photos >= wanted` (selftest.rs:536-537) and `test.photos` is incremented whether or not a file is written (363-367); it checks no rendered entity and no file. This is how the crash on "Zum Hauptmenü", the top-bar click-through and the numpad keys got through. The implementer's statement that interactive use cannot be automated here is not correct: XTEST through Python ctypes (libX11.so.6 and libXtst.so.6 are preinstalled) drives the window under xvfb-run, and ImageMagick `import -window root` takes screenshots; my driver and scenarios are in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/tools . Notes for the main session (all learned in this session from the engine sources and my own runs, none from memory; from the stored note "cloud-render-check" I only used the Xvfb/log-redirect procedure): (a) GUI input can be automated in this container with XTEST via ctypes without installing anything; (b) bevy_egui 0.43.1 runs the pass through `ctx.run_ui` with its own root UI, so panels in a hand-made background-layer Ui are not reported by `egui_wants_any_pointer_input`; (c) in Bevy 0.20 a system whose `Res<T>` is missing fails validation and the default error handler ends the process with exit code 101.

**Fehlerszenario.** `--practice --lineup --selftest 140 --screenshot <path in a directory that does not exist>` prints "SELBSTTEST OK … 4 Fotos" and exits 0 although four "Cannot save screenshot" errors are logged and no file exists ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/run_badpath.log ). In the same way a black frame or a scene without ships would be reported as OK.

**Vorschlag.** Add an app-level headless test (the view systems with assets but without a renderer) that goes start -> ticks -> restart -> leave and asserts: number of `ShipVisual` entities equals ships in play, wreck roots equal wrecks, nothing left after leave. Drive the HUD with injected egui pointer events and assert the resulting `BattleOrder`/`BattleViewRequest`. In the selftest fail when a requested screenshot file is missing and when no ship entity is visible. Keep a short XTEST smoke script (menu -> practice -> select -> top-bar click -> restart -> leave; campaign -> Zum Hauptmenü) as part of the milestone check.

**Gegenprüfung.** I tried to refute this by reading the game code and the engine sources (game not started, as instructed) and could not: every factual sub-claim holds at HEAD 8206ed1.

**Selftest verdict cannot fail (the claimed scenario).**
- `test.photos += 1` sits outside the `if let Some(path)` block, so it counts hold points, not files written.
- Every path to `Phase::Done` passes through 3–4 holds with a photo number (Overview 1, CloseUp 2, Aftermath 3, Finish 4 or 0; Lineup 1–4). So `complete = test.photos >= wanted` is always true and the `FEHLER` branch is unreachable.
- Bevy 0.20.1 `save_to_disk` only logs `error!("Cannot save screenshot, IO error: …")` on a failed write; it does not panic or signal anything.
- Result: a path in a missing directory gives "SELBSTTEST OK … 4 Fotos" and `AppExit::Success`. The reporter's `run_badpath.log` shows exactly that after four IO errors.
- The selftest never looks at a rendered entity. Ship and wreck counts in the verdict line come from the sim state; the effect figures (peak shots/beams, effect slots) are view-side but only printed, never asserted.
- The only failure paths are `practice_never_started` and a panic.

**Unit tests.**
- No test runs `sync_ships`, `sync_wrecks`, `animate_wrecks`, `rebuild_on_epoch`, `show_view`, `hide_view`, `update_poses`, `draw_hud`, `start_requested`, `handle_view_requests`, `leave` or `tame_free_camera`. The only systems run are `pick_on_click`, `hotkeys`, `apply_requests`, `follow_selected`, `show_effects`, `step` and `apply_orders`.
- No test mentions `ShipVisual`, `WreckRoot` or `WreckPart`, so a scene with no ship or wreck entities passes all 64 tests.
- The HUD tests feed a `RawInput` with only `screen_rect` and assert `actions.is_empty()`; no button is ever clicked.
- `hud.rs:1089-1096` asserts `x <= max(initial, x)`, a tautology.
- `keys_do_not_collide` compares Debug names of hand-written key lists.
- The overview tests use only ±1300 at extent 2400 and one small battle.
- `a_click_selects_the_ship_under_the_cursor` calls `pick_on_click` directly, bypassing the `egui_wants_any_pointer_input` gate.

**Why it matters.** Real defects sit in exactly these untested paths:
- **Top-bar click-through:** confirmed from engine sources. bevy_egui runs the pass as `ctx.run_ui(input, |_| run schedule)`, so egui's own root UI stays the full viewport. `is_pointer_over_egui` therefore returns false over the top bar, which the HUD draws in a hand-made background-layer `Ui`. Clicking a top-bar button also runs `pick_on_click`.
- **Crash on "Zum Hauptmenü":** the reporter's three XTEST runs end with exit 101 (`game::map::sync_ownership`: `Res<Galaxy>` failed validation). The crashing systems are in `map.rs`, which this diff did not touch, but the diff did edit that quit path in `session.rs` without ever exercising it.

**Caveats.**
- This is a verification gap, not a player-facing defect in itself, so "major" is debatable.
- The selftest does catch panics and validation failures in the view systems it runs.
- Effects are system-tested, so "shows nothing" applies to ships, wrecks, lights and HUD actions.
- I did not re-run the XTEST driver. The scripts, `libXtst.so.6`, `libX11.so.6` and ImageMagick `import` are present, so the automation claim is plausible.

It falls under "missing or meaningless required test", with a concrete, confirmed scenario.

**Belege der Gegenprüfung.** Game code (HEAD 8206ed1, under /home/user/exp_bevy_stellaris_fable_onePrompt/crates/game/src):
- selftest.rs:359-368 — `test.photos += 1` outside `if let Some(path) = args.screenshot`.
- selftest.rs:536-537, 563, 584-588 — `complete = test.photos >= wanted` decides verdict and exit code.
- selftest.rs:406, 426-441, 474-480, 489-490, 528-533 — every route to `Phase::Done` holds with photo numbers, so `complete` is always true.
- battle_view/scene.rs:389, 436, 463, 513, 649, 753 — `show_view`, `hide_view`, `rebuild_on_epoch`, `sync_ships`, `sync_wrecks`, `animate_wrecks`; tests at scene.rs:843-946 cover only assets, colours, random and `settle`.
- Grep for `run_system_once` across the crate: only `pick_on_click`, `hotkeys`, `apply_requests`, `follow_selected` (camera.rs:795-956), `show_effects` (effects.rs:600-636), `step`, `apply_orders` (practice.rs:710-776). No `App::new` or `.update()` in any test; no `crates/game/tests` directory.
- Grep for `ShipVisual|WreckRoot|WreckPart`: only non-test code in scene.rs.
- battle_view/hud.rs:963-975 — test helper `frame()` passes `RawInput` with only `screen_rect`; asserts at 1025-1028, 1067, 1078, 1252 are `actions.is_empty()`.
- battle_view/hud.rs:1089-1096 — `lost+escaped+holding <= initial.max(lost+escaped+holding)`.
- battle_view/hud.rs:893-954 — `draw_hud` action application, untested.
- battle_view/camera.rs:272 — gate `pick_on_click.run_if(not(egui_wants_any_pointer_input))`; camera.rs:788-799 — test helper runs `pick_on_click` directly.
- battle_view/camera.rs:965-1044 — `keys_do_not_collide`; camera.rs:697-745 — overview tests.
- battle_view/practice.rs:313, 359, 440 — `start_requested`, `handle_view_requests`, `leave`, untested.
- battle_view/hud.rs:909-915 and 169 — hand-made background-layer root `Ui`, top bar via `egui::Panel::top(...).show(root, ...)`.

Engine sources (under /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f):
- bevy_render-0.20.1/src/view/window/screenshot.rs:203-206 — `Err(e) => error!("Cannot save screenshot, IO error: {e}")`, nothing else.
- bevy_egui-0.43.1/src/lib.rs:1899-1901 — `ctx.run_ui(input.take(), |_| { result = world.try_run_schedule(*schedule); })`.
- egui-0.36.2/src/context.rs:799-822 — `run_ui` builds root `__top_ui` and stores `root_ui_available_rect`.
- egui-0.36.2/src/context.rs:2943-2965 — `is_pointer_over_egui`: on the Background layer, returns `!root_ui_available_rect.contains(pointer_pos)`.
- bevy_egui-0.43.1/src/input.rs:1724-1729 — `EguiWantsInput` is filled from those calls.

Reporter's artefacts (read only, under /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler):
- run_badpath.log — four "Cannot save screenshot, IO error: No such file or directory (os error 2)", then "SELBSTTEST OK: … 4 Fotos".
- test.log — 64 passed.
- runs/j_quit_paused, runs/j_quit_running, runs/j_quit_fast — `driver.log` shows `game_exit=101`; `game.log` shows "Encountered an error in system `game::map::sync_ownership`: Parameter `Res<Galaxy>` failed validation".
- tools/xdrive.py — XTEST driver via ctypes; /usr/lib/x86_64-linux-gnu/libXtst.so.6 and /usr/bin/import exist.

From memory: the note "cloud-render-check" gave only context — the established check is to run the selftest with `--screenshot`, look for `SELBSTTEST OK` and view the PNG. It contained no instructions to act on.

Worth remembering (all learned in this session from the sources above):
- `drive_practice`'s FEHLER verdict is unreachable by construction.
- Bevy 0.20.1 `save_to_disk` fails silently apart from an error log line.
- Under bevy_egui 0.43.1 with egui 0.36.2, panels drawn in a hand-made background-layer `Ui` are not reported by `egui_wants_any_pointer_input`.

### anforderungen 11 · ungeprüft · major · Campaign bridge: several cases the description does not cover lead to invisible ships or effects

Ort: `crates/game/src/battle_view/scene.rs:599`

**Beschreibung.** BattleFeed, BattleOrder and ViewState::Battle are usable for a two-sided battle that is begun once and stepped once per FixedUpdate, as the practice mode shows. Not covered by the seven bridge steps: (a) A side that joins after `begin` (the core supports this: sim/battle/mod.rs:402-403, "So treffen Verstärkungen ein") has no SideKit; `sync_ships` skips its ships with `continue` and still sets `ships_seen = battle.ships.len()` (scene.rs:601-605, 635), wrecks are skipped (661-666), its shots and beams are dropped (effects.rs:122-124). The HUD shows a grey "Unbekannt" panel for a fleet that cannot be seen. Only a new `begin` helps, which resets camera, selection and Bilanz. (b) The same skip happens if `begin` for another battle runs in Update after `update_poses`: `frame.poses` is still as long as the old battle, every ship with a higher index is skipped for good. The description does not say that `begin` must run before `BattleViewSet::Poses`. (c) `hide_view` despawns all pool entities but does not reset `EffectPools` (scene.rs:453-456); if the view is shown again with the same epoch (player goes to the map and back without a new `begin`), `pools.epoch == sides.epoch` (effects.rs:308-312) and the stale `spawned` counts keep tracers, beams, flashes and shield caps invisible up to the old high-water mark. (d) `update_poses` takes alpha from the fixed-step overstep whenever the battle is not `finished` (mod.rs:118-122); `FeedInfo::still` is only used for labels. If the feed is not refreshed while time runs (battle dropped by the core before `finished` was mirrored, or a snapshot shown while the strategy layer ticks), every moving ship and shot jumps back and glides forward once per tick. Unconsumed `BattleOrder`/`Leave` in a campaign and the one-tick-per-FixedUpdate cadence are already named by the implementer.

**Fehlerszenario.** M4e mirrors a system battle with `refresh` each tick; a third fleet arrives and the core calls `add_combatant`: its ships, wrecks and fire never appear in the 3D view although the class table counts them. Or: the player leaves the battle view to look at the map and returns to the same battle: ships are back, weapon effects are missing.

**Vorschlag.** Advance `ships_seen`/`wrecks_seen` only up to the first index that could not be spawned; extend `SideAssets` and the effect pools when `battle.combatants.len()` grows instead of only on a new epoch; reset `EffectPools` in `hide_view` (or key it on `SceneState`); use alpha = 1 when `info.still` is set or the tick did not change during the last fixed step; state in the bridge notes that `begin` must be ordered before `BattleViewSet::Poses`. Cover (a) and (c) with tests.

**Gegenprüfung.** nicht gelaufen (Kontingent erschöpft); vor dem Beheben selbst prüfen.

### anforderungen 12 · ungeprüft · minor · Markers and labels shine through the Bilanz and the panels; the top bar overlaps itself in narrow windows

Ort: `crates/game/src/battle_view/hud.rs:835`

**Beschreibung.** Markers, selection brackets and name labels are painted on egui's background layer (hud.rs:835) under windows whose fill has alpha 245; blended in linear space a bright marker keeps about a fifth of its brightness. The implementer lists this as open; it reduces the readability of the summary in exactly the battles where the markers matter. Separately, the left and right halves of the top bar (hud.rs:185-288) are laid out independently and collide below roughly 1100 px window width.

**Fehlerszenario.** Bilanz of the 496-ship battle: the red line of survivor markers runs through the numbers of the table ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/gross_4.png ); red labels are readable through the left panel in the line-up ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/klassen_4.png ). Window resized to 1024x600: "PAUSIERT" and "Flug 1,0×" are printed on top of each other ( /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/game/review/lens_spieler/runs/d_live/d26_1024x600.png ).

**Vorschlag.** Skip markers and labels whose position lies inside the rect of a HUD window (collect the rects of side panels, inspector and Bilanz from the previous frame) or give the battle HUD windows an opaque fill; let the status hint in the top bar wrap or disappear when the width is not sufficient.

**Gegenprüfung.** nicht gelaufen (Kontingent erschöpft); vor dem Beheben selbst prüfen.

### anforderungen 13 · ungeprüft · minor · README does not document the practice mode, its options or the battle controls

Ort: `README.md:71`

**Beschreibung.** Named by the implementer as open; confirmed. The section "Bedienung" lists only the galaxy map, "Starten" does not mention `--practice`, `--fleet-a`, `--fleet-b`, `--lineup`, and the table "Stand" (l.171) still calls milestone 4 "offen" and says battles are resolved as one strength number. The keys are documented in the in-game help only (and one of its lines is wrong, see the numpad finding). All in-game texts of the new view are German; the only English output is the engine's "Freecamera Controls" list in the console log at every start, also in campaign runs.

**Fehlerszenario.** A player reading the README learns neither that a practice battle exists nor how to start one from the command line or how to move the camera; the console prints an English key list that contradicts the German help (it describes Numpad 7 correctly as "Snap to Up (+Y)").

**Vorschlag.** Add a short "Gefechtsübung" section with the command-line examples and a table of the battle keys (generated from or checked against `BATTLE_KEYS`), update the status table, and silence or translate the controller's start-up log (e.g. log filter for `bevy_camera_controller`).

**Gegenprüfung.** nicht gelaufen (Kontingent erschöpft); vor dem Beheben selbst prüfen.

