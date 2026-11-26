# Übergabe (Stand 26.11.2026, 13:30 UTC)

Alles, was eine neue Sitzung braucht, um die Arbeit an KONFLUENZ fortzusetzen, nachdem der
Container abgeräumt wurde. Dieser Branch (`wip/uebergabe`) enthält gegenüber dem Hauptzweig
nur das Verzeichnis `uebergabe/`. Er wird nicht in den Hauptzweig gemergt und nach
Meilenstein 7 gelöscht.

## Entscheidungen des Auftraggebers

- 26.11.2026, 13 Uhr UTC: Das Kontingent des Hauptmodells war nach fünf Stunden mit vier
  parallelen Strängen erneut erschöpft (Reset 29.11.2026, 03:00 UTC). Freigegebener Plan
  (`restplan.md`): Zwischenstände sichern, Pause ohne automatischen Start; danach
  Meilenstein 4 zuerst, höchstens zwei Stränge gleichzeitig. Die Arbeit geht erst auf ein
  ausdrückliches Startsignal weiter.
- 26.11.2026, 07:40 UTC: Das Hauptmodell war wieder verfügbar; Teil II lief bis 12:40 UTC.
- 26.11.2026: Schlanke Prüfung je Paket (Umsetzung, zwei bis drei Prüfer, je Prüfer eine
  Sammel-Gegenprüfung, Nachbesserung).
- 26.11.2026: Zwischenstände dürfen als `wip/…`-Branches gepusht werden. Der Hauptzweig
  `claude/keen-franklin-aqi4h7` bleibt lauffähig; kein Pull Request ohne Auftrag.
- 26.11.2026: Auf das Hauptmodell wird gewartet; ohne Auftrag wird nichts auf andere Modelle
  verlagert.
- 25.11.2026: Stränge, die der Schutzfilter unter dem Hauptmodell stoppt (bisher der
  Gefechtsrechner), laufen auf dem Ausweichmodell weiter; jeder Stopp wird gemeldet.
- Welche Modelle gemeint sind, steht in den Projektnotizen der Sitzung, nicht im Repository.

## Branches

„Zwischenstand" heißt: Ein Agentenschritt wurde mittendrin abgebrochen und so, wie er war,
committet. Diese Commits sind unvollständig und ungeprüft.

| Branch | Inhalt | Letzter geprüfter Stand | Spitze |
|---|---|---|---|
| `claude/keen-franklin-aqi4h7` | Meilenstein 1–3, 3D-Schlachtansicht, Gefechtsübung | 8206ed1, lauffähig | 8206ed1 |
| `wip/m4-integ` | zusammengeführter Kern (Flotten aus Schiffen, Werften, Nachschub, Gefechte der Partie über den taktischen Rechner, Technologie), Behebung der Befunde der strategischen Ebene | 5ac904d (264 Kerntests grün, Sweeps sauber; `berichte/kern_zusammenfuehrung.md`). Danach vier Schritte der Befundbehebung bis 8758cd7 (Gefechte, Werften, Treibstoff und Verlustmaß, Drehbuch für Bestellungen): je Schritt vom Agenten gebaut, als Ganzes nicht abgenommen, ohne Bericht | 8ffaae6: Zwischenstand Staatshaushalt |
| `wip/m4-spiel` | Hauptzweig + behobene Befunde der Schlachtansicht | 5f997cb (130 Tests grün; `berichte/schlachtansicht_nachbesserung.md`; geprüft, Nachbefunde in `befunde/schlachtansicht_nachpruefung.md`) | 5ea3050: Zwischenstand der Nachbesserung |
| `wip/m5-rechner` | Belagerung: Tagesrechner mit Prognose, Belagerung in der laufenden Partie | bd85c0b (327 Kerntests grün; `berichte/belagerung_partie_1.md`). Danach 7e44f6f und ec9c98d (Landung, Übergabe, Plünderung): vom Agenten gebaut, ohne Bericht | 8d0023b: Zwischenstand Haken in Produktion und Kursen |
| `wip/m6-nachfolge` | ein Pfad für Besitzwechsel und das Ende einer Gemeinschaft, Schutzgebiete, die beiden Enden | 816d69c (grün; `berichte/nachfolge_umsetzung.md`). Prüfung halb fertig: `befunde/nachfolge.md` | 816d69c |
| `wip/m4-taktik`, `wip/m4-tech`, `wip/m4-sim` | Vorstufen | vollständig in `wip/m4-integ` enthalten | – |
| `wip/uebergabe` | dieses Verzeichnis | – | – |

### Bauzustand der Zwischenstände

Gemessen am 26.11.2026 nach dem Sichern mit `cargo test --no-fail-fast`. Repariert wurde nichts.

- `wip/m4-spiel` 5ea3050: baut. `cargo test -p game`: 136 von 145 Tests grün. Rot sind neun
  neue Tests zu den Nachbefunden, deren Korrektur fehlt oder unfertig ist:
  `battle_view::camera::tests::a_new_battle_starts_with_the_normal_fly_speed`,
  `battle_view::hud::tests::a_newly_selected_ship_starts_at_the_top_of_its_module_list`,
  `map::tests::a_press_beside_a_panel_has_one_owner_even_without_a_run_up`, drei in
  `pointer::tests` (`a_grabbed_pointer_is_withheld_from_the_interface`,
  `a_press_just_beside_a_button_belongs_to_the_button`, `windows_cover_and_passive_areas_do_not`)
  und drei in `selftest::tests` (Kamera aus oder falsche Kamera).
- `wip/m4-integ` 8ffaae6: baut. Der Testlauf lief beim Schreiben noch; Zwischenergebnis: in
  den Unit-Tests ein Test rot (`fleet::supply::tests::an_empty_treasury_pays_what_it_has_and_records_arrears`),
  in `tests/military.rs` neun von 46 rot (Werft, Auffüllen, Unterhalt, Rückstände: alles, was
  am Geldfluss des Staates hängt), `battle`, `economy`, `finance` grün.
- `wip/m5-rechner` 8d0023b: Lauf stand noch aus.

## Inhalt

- `restplan.md`: der freigegebene Plan vom 26.11.2026, 13:10 UTC. Teil II ist die Arbeit ab
  dem Startsignal: Reihenfolge, Aufträge je Paket, Prüfbefehle.
- `entwurf/M5_Synthese.md`, `entwurf/M6_Synthese.md`: verbindliche Umsetzungsvorgaben für
  Belagerung sowie KI, Diplomatie und Enden. Die übrigen zehn Dateien sind die Entwürfe und
  Jury-Berichte, aus denen sie entstanden.
- `befunde/strategie.md`, `befunde/schlachtansicht.md`: Prüfbefunde der ersten Runde im
  Wortlaut mit Urteil und Begründung der Gegenprüfer. Die der Schlachtansicht sind behoben
  (5f997cb), die der strategischen Ebene zum größeren Teil (bis 8758cd7; offen sind der
  Staatshaushalt und die Tests der Gruppe 5).
- `befunde/schlachtansicht_nachpruefung.md`: Nachprüfung nach der Behebung. Bestätigt sind
  sieben verschiedene Befunde, einer davon schwer („Laden (F9)" in einer laufenden Partie
  beendet das Spiel, wenn der Spielstand eine kleinere Galaxie hat), dazu ein Hinweis zum
  gemeinsamen Bauverzeichnis (siehe unten).
- `befunde/nachfolge.md`: vier Befunde des Blickwinkels „erhaltung", nicht gegengeprüft. Der
  zweite Blickwinkel (Enden, Determinismus, Ehrlichkeit der Tests) hat nicht stattgefunden.
- `berichte/`: Abschlussberichte der Agenten mit Schnittstellennotizen (`api_notes`),
  Messwerten und offenen Punkten.
- `workflows/`: elf Workflow-Skripte als Vorlagen für Aufträge und Schemata.
  `m4-kern-befunde.js` ist das Skript für den Kernstrang (Befunde und Staatshaushalt, dann
  Militär-KI, dann Prüfung und Nachbesserung); sein erster Auftrag muss an den Zwischenstand
  angepasst werden.

## Wiedereinstieg in einem frischen Container

```bash
git fetch origin wip/m4-integ wip/m4-spiel wip/m5-rechner wip/m6-nachfolge wip/uebergabe
echo "/.worktrees/" >> .git/info/exclude
git worktree add .worktrees/integ wip/m4-integ
git worktree add .worktrees/spiel wip/m4-spiel
git worktree add .worktrees/belagerung wip/m5-rechner
git worktree add .worktrees/nachfolge wip/m6-nachfolge
git worktree add .worktrees/uebergabe wip/uebergabe
```

- Reihenfolge ab hier: `restplan.md`, Teil II. Zuerst Meilenstein 4 mit zwei Strängen
  (Kern: Staatshaushalt abschließen, Militär-KI, Prüfung; Spiel: Nachbesserung abschließen,
  Anpassung an den Kern, Technologie- und Flottenfenster, Gefechte der Partie in der
  3D-Ansicht). Die Stränge für Meilenstein 5 und 6 ruhen bis dahin.
- Zwischenstände: Der nächste Agent eines Strangs sieht sich zuerst den Commit
  „Zwischenstand vor Kontingentende" an (`git show --stat HEAD`), übernimmt ihn oder macht
  ihn mit `git revert` rückgängig, und arbeitet dann seinen Auftrag ab. Die Historie wird
  nicht umgeschrieben.
- Für Meilenstein 5 werden Kern, `wip/m6-nachfolge` und `wip/m5-rechner` zusammengeführt:
  Die Übergabe eroberter Planeten (`hand_over` in `crates/sim/src/siege/conquest.rs`) ist als
  Nahtstelle markiert und wird durch `transfer_planet` ersetzt; Beschaffung und Unterhalt der
  Verteidigung werden ein Anspruch im Staatshaushalt.
- `SAVE_FORMAT` wird nur beim Zusammenführen in den Hauptzweig erhöht, nie auf den Strängen.
- Bildprüfung: siehe Projektnotiz `cloud-render-check` (Pakete für Software-Vulkan,
  `xvfb-run … --selftest N --screenshot datei.png`). Für Klicktests zusätzlich `xdotool`
  und `imagemagick` installieren; Ablauf: `Xvfb :77 -screen 0 1600x900x24`, warten bis
  `/tmp/.X11-unix/X77` existiert, Spiel mit `DISPLAY=:77` starten, `xdotool mousemove X Y click 1`,
  Foto mit `import -window root datei.png`.
- Erster Bau des Spiels etwa 12 bis 20 Minuten, Kern allein wenige Minuten.

## Was man wissen muss

- Die Simulation ist chaotisch: Jede Änderung an der Reihenfolge der Systeme oder an den
  Geldflüssen des Staates verschiebt alle Verläufe. Tests, die an einem Seed hängen, müssen
  dann neu begründet werden (nie die Aussage abschwächen, sondern das Maß verbessern).
- Die Headless-Werkzeuge lesen `assets/data` relativ zum aktuellen Verzeichnis: immer im
  jeweiligen Arbeitsverzeichnis starten oder `--data-dir` angeben.
- **Gemeinsames Bauverzeichnis:** cargo beurteilt die Frische der Workspace-Crates
  unabhängig vom Pfad des Checkouts. Wird aus zwei Checkouts in dasselbe `CARGO_TARGET_DIR`
  gebaut, kann ein Lauf stillschweigend das `sim` des anderen benutzen; `sim` backt außerdem
  seinen Quellpfad in den Pfad der Spieldaten ein. Regel: In das gemeinsame `target/` baut nur
  ein Arbeitsverzeichnis (der Spielstrang), Kopien bekommen ein eigenes Zielverzeichnis, und
  vor Abnahmeläufen `cargo clean -p sim -p game -p headless` (der Engine-Bau bleibt erhalten).
- Klicktests: Laut zwei Agenten vom 26.11. kommen auch Tastendrücke von `xdotool` an, wenn
  das Fenster vorher mit `xdotool windowfocus <Fenster>` den Fokus bekommt. Züge brauchen
  `mousedown` und `mousemove_relative`. Zifferntasten setzen das Tempo und heben die Pause auf.
- Das ganze Spiel lässt sich ohne Fenster und Grafikkarte testen; das Rezept steht in
  `crates/game/src/testing.rs` auf `wip/m4-spiel`.
- Friedliche Sweeps enthalten keine Abspaltung. Die Regressionsprüfung für Besitzwechsel ist
  der Sweep mit Kriegsdrehbuch; der Befehl steht in `berichte/nachfolge_umsetzung.md`.
- Kontingent: Vier parallele Stränge verbrauchten es in fünf Stunden. Auf vier Kernen
  dauerte dabei ein Testlauf des Kerns 4 bis 10 Minuten; das Warten kostet zusätzlich. Daher
  höchstens zwei Stränge gleichzeitig.
- Die Forschung und die Flotte kosteten eine Gemeinschaft bis 8758cd7 nichts Echtes; der
  Staatshaushalt (Zwischenstand 8ffaae6) soll das beheben.
- Agenten arbeiten nie im Hauptverzeichnis, nur in `.worktrees/…`: Ein Stop-Hook verlangt
  dort Commit und Push aller offenen Änderungen, sobald ein Zug endet.
- `cargo`-Ausgaben nie durch `tail` oder `grep` beurteilen, sondern in eine Logdatei
  umleiten und `$?` prüfen.
- Der Blockadetest in `crates/sim/tests/economy.rs` hängt am Seed (derzeit Seed 4).
- Aufträge an Agenten in neutraler Spielmechanik-Sprache formulieren.
