# Übergabe (Stand 26.11.2026)

Alles, was eine neue Sitzung braucht, um die Arbeit an KONFLUENZ fortzusetzen, nachdem der
Container abgeräumt wurde. Dieser Branch (`wip/uebergabe`) enthält gegenüber dem Hauptzweig
nur das Verzeichnis `uebergabe/`. Er wird nicht in den Hauptzweig gemergt und nach
Meilenstein 7 gelöscht.

## Entscheidungen des Auftraggebers

- 26.11.2026: Auf das Hauptmodell der Sitzung warten. Fortsetzung erst nach dem Reset des Kontingents
  (Sonntag, 29.11.2026, 03:00 UTC) und erst auf Startsignal; kein automatischer Start.
- 26.11.2026: Schlanke Prüfung je Paket (Umsetzung, zwei bis drei Prüfer, je Prüfer eine
  Sammel-Gegenprüfung, Nachbesserung).
- 26.11.2026: Zwischenstände dürfen als `wip/…`-Branches gepusht werden. Der Hauptzweig
  `claude/keen-franklin-aqi4h7` bleibt lauffähig; kein Pull Request ohne Auftrag.
- 25.11.2026: Stränge, die der Schutzfilter unter dem Hauptmodell stoppt (bisher der Gefechtsrechner),
  laufen auf dem Ausweichmodell weiter; jeder Stopp wird gemeldet.
- Welche Modelle gemeint sind, steht in den Projektnotizen der Sitzung, nicht im Repository.

## Branches

| Branch | Commit | Inhalt | Stand |
|---|---|---|---|
| `claude/keen-franklin-aqi4h7` | 8206ed1 | Meilenstein 1–3, 3D-Schlachtansicht, Gefechtsübung | lauffähig; 19 bestätigte Befunde der Schlachtansicht offen |
| `wip/m4-taktik` | 35801ce | taktischer Gefechtsrechner, ausgewuchtet | fertig, geprüft, nachgebessert |
| `wip/m4-tech` | d314838 | Technologie und Forschung im Kern | fertig, geprüft, nachgebessert |
| `wip/m4-integ` | 9c63cad | Hauptzweig + Gefechtsrechner + Technologie | Tests und Clippy grün für `sim` und `headless`; `crates/game` dagegen noch nicht gebaut |
| `wip/m4-sim` | 4b64d65 | Schiffe, Werften, Nachschub, Unterhalt, Gefechte in der Partie | geprüft; 17 bestätigte Befunde offen; gegen die alte Gefechts-Schnittstelle gebaut |
| `wip/uebergabe` | – | dieses Verzeichnis | – |

## Inhalt

- `restplan.md`: der freigegebene Plan (Teil II ist die Arbeit ab dem 29.11.).
- `entwurf/M5_Synthese.md`, `entwurf/M6_Synthese.md`: verbindliche Umsetzungsvorgaben für
  Belagerung sowie KI, Diplomatie und Enden. Die übrigen zehn Dateien sind die Entwürfe und
  Jury-Berichte, aus denen sie entstanden.
- `befunde/strategie.md`, `befunde/schlachtansicht.md`: alle Prüfbefunde im Wortlaut mit
  Urteil und Begründung der Gegenprüfer. „bestätigt" heißt offen.
- `berichte/`: Abschlussberichte der Agenten mit Schnittstellennotizen (`api_notes`),
  Messwerten und offenen Punkten.
- `workflows/`: die fünf Workflow-Skripte als Vorlagen für Aufträge und Schemata.

## Wiedereinstieg in einem frischen Container

```bash
git fetch origin wip/m4-integ wip/m4-sim wip/m4-taktik wip/m4-tech wip/uebergabe
echo "/.worktrees/" >> .git/info/exclude
git worktree add .worktrees/integ wip/m4-integ
git worktree add .worktrees/sim wip/m4-sim
git worktree add .worktrees/uebergabe wip/uebergabe
```

- Bildprüfung: siehe Projektnotiz `cloud-render-check` (Pakete für Software-Vulkan,
  `xvfb-run … --selftest N --screenshot datei.png`). Für Klicktests zusätzlich `xdotool`
  und `imagemagick` installieren; Ablauf: `Xvfb :77 -screen 0 1600x900x24`, warten bis
  `/tmp/.X11-unix/X77` existiert, Spiel mit `DISPLAY=:77` starten, `xdotool mousemove X Y click 1`,
  Foto mit `import -window root datei.png`. Tastendrücke von `xdotool` kommen im Spiel nicht
  an, Mausklicks schon.
- Erster Bau des Spiels etwa 12 bis 20 Minuten, Kern allein wenige Minuten.
- Danach `restplan.md`, Teil II: zuerst Paket A (Kern zusammenführen), parallel D und E.

## Was man wissen muss

- Probemerge von `wip/m4-sim` in `wip/m4-integ`: Konflikte in `crates/headless/src/main.rs`,
  `crates/sim/src/economy/{capital,mod,production}.rs` und `crates/sim/src/harness.rs`.
- `wip/m4-sim` kennt `Battle::results`, `Fate`, `CombatantSetup::with_war_weariness` und das
  Patt noch nicht (siehe `berichte/gefechtsrechner_nachbesserung.md`). Das Patt löst zugleich
  den Befund „unentschiedene Gefechte starten endlos neu".
- Die Forschung kostet eine Gemeinschaft bisher nichts Echtes
  (`berichte/technologie_nachbesserung.md`, Punkt 12); der Befund „Flottenkosten ohne Spur
  im Haushalt" hat dieselbe Ursache. Beides gehört zusammen gelöst.
- Agenten arbeiten nie im Hauptverzeichnis, nur in `.worktrees/…`: Ein Stop-Hook verlangt
  dort Commit und Push aller offenen Änderungen, sobald ein Zug endet.
- Je Workflow laufen auf der 4-Kern-Maschine höchstens zwei Agenten gleichzeitig; bei vier
  parallelen Strängen dauert jeder Bau ein Mehrfaches.
- `cargo`-Ausgaben nie durch `tail` oder `grep` beurteilen, sondern in eine Logdatei
  umleiten und `$?` prüfen.
- Der Blockadetest in `crates/sim/tests/economy.rs` hängt am Seed (derzeit Seed 4).
- Aufträge an Agenten in neutraler Spielmechanik-Sprache formulieren.
