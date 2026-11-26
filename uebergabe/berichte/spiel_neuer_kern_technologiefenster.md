# B2

## summary

Commit 8758cd7 ist als echter Merge in wip/m4-spiel eingebracht, crates/game läuft auf dem neuen Kern. Karte und Flottenliste zeigen Flotten über Fleet::day_range, mit Gefecht (roter Ring) und Sammeln (bernsteinfarbener Bogen); im Gefecht gibt es „Rückzug“ statt Marsch. Das neue Technologiefenster („Forschung“ in der Kopfleiste) zeigt den Baum nach Feld und Stufe, setzt Ziel und Etat per SimCommand und führt die „Einschätzung der anderen“ als Spannen aus Research::estimate. Ältere Spielstände werden mit deutscher Meldung abgewiesen. Endabnahme auf 9c833d5 nach cargo clean: fmt, build, 170 Tests, clippy -D warnings für das Paket game alle Exit 0; Kampagnen- und Übungs-Selbsttest OK. Nicht gelaufen: Tests und Lint für den ganzen Workspace. Beweisfotos per echter Eingabe (xdotool auf Xvfb :87) im Kampagnenjahr 2407; Baum, Einschätzung, Karte und Flottenfenster stammen vom Endstand, die Fotos zu 1280x720 und zum alten Spielstand vom Vorgänger-Commit a1289d1 (danach nur noch der Hinweis „Vorgemerkt“ ergänzt). Spiel und Xvfb sind per PID beendet, der Arbeitsbaum ist sauber und gepusht. Aus dem Gedächtnis genutzt: nur das Xvfb/xdotool-Rezept und der Hinweis, dass saves/ im aktuellen Verzeichnis entsteht; keine Notiz verlangte etwas außerhalb der Aufgabe.

## merge

Genau 8758cd7 als echter Merge-Commit 4cf7f3d mit deutscher Nachricht, ohne Konflikte (README automatisch zusammengeführt). crates/sim, headless und assets sind identisch mit 8758cd7. Direkt nach dem Merge kompilierte game unverändert; zwei Tests waren rot (fehlendes contact_time in der Gefechtskopie, Annahme über die dritte Seite) und sind behoben, ohne Zusicherungen abzuschwächen.

## api_drift

- Flottenposition: Marsch wird jetzt über Fleet::day_range(data, research) gerechnet statt über eine feste Reichweite; ohne Treibstoff steht die Flotte und die Liste sagt „ohne Treibstoff“.
- Gefecht und Sammeln: Fleet::in_battle, regroup_until und Battles::of_fleet (retreating) speisen einen Flottenzustand (im Gefecht, im Rückzug, sammelt sich bis Datum, im Marsch, stationiert) für Karte und Liste.
- MoveFleet wird im Gefecht und beim Sammeln vom Kern abgelehnt; die Oberfläche bietet dort „Rückzug“ (SimCommand::Retreat) bzw. einen gesperrten Marsch-Knopf, und abgelehnte Spielerbefehle erscheinen als Meldung „Befehl abgelehnt: …“.
- Speicherformat 4: SaveError::Format wird zu einer deutschen Meldung über die ältere bzw. neuere Fassung; das Format wird dafür aus dem Dateikopf gelesen.
- Forschung wirkt auf die Wirtschaft: Produktionsreiter und Kettenfenster rechnen jetzt mit unit_economics_at und dem Ertragsfaktor des Besitzers; Frachter nutzen Logistics::freighter_speed.
- Gefechtsansicht: copy_state kopiert contact_time; das Patt wird als „PATT“ in Kopfleiste, Seitenfeldern und Zusammenfassung gezeigt.
- Tagesanteil der Kartenmarken korrigiert (0 bei Takt 0, sonst ((takt-1)%20+1)/20), damit Marken an der Tagesgrenze nicht zurückspringen.

## tech_window

crates/game/src/ui/tech.rs, geöffnet über „Forschung“ in der Kopfleiste. Reiter „Eigene Forschung“: Etat-Regler mit „Etat festsetzen“ (SetResearchBudget), Zielzeile mit Fortschritt, Restdauer und „Automatik einschalten“ (SetResearchTarget None), Baum als Raster aus fünf Feldern mal sechs Stufen mit Karten (erforscht, in Arbeit, erforschbar, fehlende Voraussetzung, Kosten, Wirkungen, „Voraussetzung für“ Entwürfe, Knopf „Erforschen“), darunter „Wirkung der eigenen Technik“ als Summe. Reiter „Einschätzung der anderen“: je Gemeinschaft Einblick in Prozent und je Feld ein Stufenstreifen (bestätigt, möglich, ausgeschlossen) mit Spanne aus Research::estimate, plus Legende. Abweichung vom Auftrag: die Einschätzung ist ein Reiter statt eines Abschnitts im selben Blatt, damit der Baum bei 1280x720 Platz hat. Fenster ist nicht in der Größe ziehbar, scrollt bei Bedarf und wird von der Kartenabdeckung erfasst.

## tests_added

cargo test -p game: 170 bestanden (vorher 151). Neu als Ganzspiel-Tests: Fenster öffnet und schließt bei 1280x720 und 1600x900; Klicks im Fenster erreichen die Karte nicht (mit Kontrollpunkten über freier Karte); Ziel setzen sendet den Befehl und die Simulation forscht daran, Automatik schaltet zurück; Etat-Befehl; gezeigte Einschätzung gleich Research::estimate und bei wenig Einblick breiter als der genaue Stand; Flotte im Gefecht bietet Rückzug und keinen Marsch; älterer, neuerer und unlesbarer Spielstand werden mit Meldung abgewiesen; abgelehnter Befehl wird gemeldet; Technologiefenster im Rundgang. Dazu Einheitentests für Tagesanteil, Marsch ohne Rücksprung, Antrieb und leeren Tank, Liegeplätze, Gefechts- und Sammelmarken, Technikzustand und Wirkungstexte, echtes Patt, README nennt die Fensterbeschriftungen.

## photos

- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/f/foto_1_karte_flotten.png – Karte am 25.03.2407 mit zwei Flotten im Gefecht bei Zeloth (rote Ringe), Endstand 9c833d5.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/f/foto_2_flottenfenster.png – Flottenfenster: Flotte „im Gefecht“ mit Knopf „Rückzug“, Endstand.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/f/foto_2b_flotte_sammelt_sich.png – 01.05.2407: „sammelt sich bis 17.05.2407“, Marsch gesperrt, Bogen bei Yroa, Endstand.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/f/foto_3_forschung_baum.png – Technologiebaum am 01.05.2407 (2 von 30 erforscht, Zielrechner in Arbeit, Etat 22 %), Endstand.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/f/foto_4_forschung_einschaetzung.png – Einschätzung der anderen: Spannen 0–4, beim Irrlicht-Syndikat Antrieb 1–5 mit bestätigter Stufe 1, Endstand.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/f/foto_8_befehl_vorgemerkt.png – Hinweis „Vorgemerkt“ nach Rückzug bei stehender Zeit, Endstand.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/f/a1289d1/foto_5_alter_spielstand.png – F9 mit Format-1-Spielstand zeigt die deutsche Meldung; aufgenommen auf dem Vorgänger-Commit a1289d1, auf dem Endstand nicht wiederholt.
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/f/a1289d1/foto_6_forschung_1280x720.png und foto_7_baum_1280x720.png im selben Ordner – Technologiefenster bei 1280x720; ebenfalls von a1289d1, auf dem Endstand nur durch die Ganzspiel-Tests bei beiden Größen gedeckt.

## files_changed

- README.md
- crates/game/src/traffic.rs
- crates/game/src/session.rs
- crates/game/src/testing.rs
- crates/game/src/walkthrough.rs
- crates/game/src/readme.rs
- crates/game/src/ui/mod.rs
- crates/game/src/ui/tech.rs (neu)
- crates/game/src/ui/fleets.rs
- crates/game/src/ui/panels.rs
- crates/game/src/ui/economy.rs
- crates/game/src/ui/format.rs
- crates/game/src/battle_view/feed.rs
- crates/game/src/battle_view/hud.rs
- crates/game/src/battle_view/effects.rs

## checks

Endabnahme auf 9c833d5 nach cargo clean -p sim -p game, Protokolle in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel2/abschluss3: cargo fmt -p game -- --check Exit 0; cargo build -p game Exit 0; cargo test -p game Exit 0 (170 bestanden); cargo clippy -p game --all-targets -- -D warnings Exit 0. Kampagnen-Selbsttest: „SELBSTTEST OK: 200 Frames, Datum 04.09.2400, Takt 4875, 16 von 90 Planeten besiedelt, 49 Kartenobjekte“; Übungs-Selbsttest OK (193 Frames, 36 Schiffe, 21 Wracks, 4 Fotos); die Bilder beider Läufe wurden angesehen. Nicht gelaufen: Tests und Lint für den gesamten Workspace (sim, headless).

## commit

9c833d5 auf wip/m4-spiel, gepusht nach origin/wip/m4-spiel, Arbeitsbaum sauber. Folge: 4cf7f3d (Merge), e134ce3, f4bc9bc, f4cd739, b6f5d47, c74843c, a1289d1, 9c833d5.

## sim_defects

- SaveGame::from_ron prüft das Format erst nach dem vollständigen Dekodieren des Zustands; ein älterer Stand, der sich nicht mehr dekodieren lässt, liefert daher einen Lesefehler statt SaveError::Format. Das Spiel umgeht das, indem es das erklärte Format aus dem Dateikopf liest; nicht in crates/sim geändert.
- Randfall beobachtet: Eine Flotte kann in_battle tragen, ohne dass Battles::of_fleet einen Eintrag liefert; die Oberfläche zeigt dann „im Gefecht“ ohne Rückzugsstatus. Ursache im Kern nicht untersucht.

## open_issues

- Flottenfenster bleibt klein: Bauliste, Zusammenlegen, Teilen und Flottenetat sind nicht begonnen (laut Auftrag kein voller Umbau).
- Kampagnengefechte lassen sich nicht in der 3D-Ansicht betrachten; die 3D-Ansicht läuft weiter nur im Übungsmodus.
- Über den Auftrag hinaus ergänzt: Auswahl von Aufstellung und Doktrin im Flottenfenster, Meldungen „Befehl abgelehnt“ und „Vorgemerkt“ bei stehender Zeit, Ertragskorrektur im Produktionsreiter, Pfeilzeichen ersetzt (Schrift hat kein „→“).
- Einige Einzelheiten (z. B. Bedeutung der Stufenstreifen) stehen nur im Schwebetext und in der Legende.
- Die Fotos für 1280x720 und den alten Spielstand stammen von a1289d1 und wurden auf 9c833d5 nicht neu aufgenommen.
- Workspace-weite Tests und Lint sind nicht gelaufen; die README-Sätze zu Forschung und Flotten müssen bei einem späteren Etat-Merge nachgezogen werden.

