# Übergabe (Stand 26.11.2026, 10 Uhr UTC)

Alles, was eine neue Sitzung braucht, um die Arbeit an KONFLUENZ fortzusetzen, nachdem der
Container abgeräumt wurde. Dieser Branch (`wip/uebergabe`) enthält gegenüber dem Hauptzweig
nur das Verzeichnis `uebergabe/`. Er wird nicht in den Hauptzweig gemergt und nach
Meilenstein 7 gelöscht.

## Entscheidungen des Auftraggebers

- 26.11.2026 (später am Tag): Das Hauptmodell ist wieder verfügbar; die Arbeit an Teil II läuft seitdem.
- 26.11.2026: Schlanke Prüfung je Paket (Umsetzung, zwei bis drei Prüfer, je Prüfer eine
  Sammel-Gegenprüfung, Nachbesserung).
- 26.11.2026: Zwischenstände dürfen als `wip/…`-Branches gepusht werden. Der Hauptzweig
  `claude/keen-franklin-aqi4h7` bleibt lauffähig; kein Pull Request ohne Auftrag.
- 25.11.2026: Stränge, die der Schutzfilter unter dem Hauptmodell stoppt (bisher der Gefechtsrechner),
  laufen auf dem Ausweichmodell weiter; jeder Stopp wird gemeldet.
- Welche Modelle gemeint sind, steht in den Projektnotizen der Sitzung, nicht im Repository.

## Branches

| Branch | Inhalt | Stand |
|---|---|---|
| `claude/keen-franklin-aqi4h7` | Meilenstein 1–3, 3D-Schlachtansicht, Gefechtsübung | lauffähig, unverändert (8206ed1) |
| `wip/m4-integ` | zusammengeführter Kern: Flotten aus Schiffen, Werften, Nachschub, Gefechte der Partie über den taktischen Rechner, Technologie | Zusammenführung fertig (5ac904d, 264 Kerntests grün; Bericht `berichte/kern_zusammenfuehrung.md`). In Arbeit: 15 offene Befunde, Staatshaushalt, Militär-KI, Prüfung |
| `wip/m4-spiel` | Hauptzweig + behobene Befunde der Schlachtansicht (Absturz „Zum Hauptmenü", Durchklicken, Inspektor, Dialog, Selbsttest) | in Arbeit; danach Anpassung an den neuen Kern, Flotten- und Technologiefenster |
| `wip/m5-rechner` | Belagerung: Zustand, Stellschrauben, reiner Tagesrechner mit Prognose (Bericht `berichte/belagerungsrechner_umsetzung.md`) | Rechner fertig (2764a4a); in Arbeit: Belagerung in der laufenden Partie (Schritt 4 des Entwurfs) |
| `wip/m6-nachfolge` | ein Pfad für Besitzwechsel und das Ende einer Gemeinschaft, Schutzgebiete, die beiden Enden | in Arbeit (Basis 5ac904d) |
| `wip/m4-taktik`, `wip/m4-tech`, `wip/m4-sim` | Vorstufen | vollständig in `wip/m4-integ` enthalten |
| `wip/uebergabe` | dieses Verzeichnis | – |

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
git fetch origin wip/m4-integ wip/m4-spiel wip/m5-rechner wip/m6-nachfolge wip/uebergabe
echo "/.worktrees/" >> .git/info/exclude
git worktree add .worktrees/integ wip/m4-integ
git worktree add .worktrees/spiel wip/m4-spiel
git worktree add .worktrees/belagerung wip/m5-rechner
git worktree add .worktrees/nachfolge wip/m6-nachfolge
git worktree add .worktrees/uebergabe wip/uebergabe
```

- Bildprüfung: siehe Projektnotiz `cloud-render-check` (Pakete für Software-Vulkan,
  `xvfb-run … --selftest N --screenshot datei.png`). Für Klicktests zusätzlich `xdotool`
  und `imagemagick` installieren; Ablauf: `Xvfb :77 -screen 0 1600x900x24`, warten bis
  `/tmp/.X11-unix/X77` existiert, Spiel mit `DISPLAY=:77` starten, `xdotool mousemove X Y click 1`,
  Foto mit `import -window root datei.png`. Tastendrücke von `xdotool` kommen im Spiel nicht
  an, Mausklicks schon.
- Erster Bau des Spiels etwa 12 bis 20 Minuten, Kern allein wenige Minuten.
- Reihenfolge ab hier (`restplan.md`, Teil II, leicht umgestellt): Kern (Befunde, Staatshaushalt,
  Militär-KI) und Spiel (Anpassung, Technologiefenster, Flottenfenster, Gefechte der Partie in
  der 3D-Ansicht) ergeben Meilenstein 4. Für Meilenstein 5 werden Kern, `wip/m6-nachfolge` und
  `wip/m5-rechner` zusammengeführt: Die Übergabe eroberter Planeten in
  `crates/sim/src/siege/conquest.rs` ist als Nahtstelle markiert und wird durch
  `transfer_planet` ersetzt; Beschaffung und Unterhalt der Verteidigung (Schritt 5 des Entwurfs)
  werden ein Anspruch im Staatshaushalt.
- `SAVE_FORMAT` wird nur beim Zusammenführen in den Hauptzweig erhöht, nie auf den Strängen.

## Was man wissen muss

- Die Simulation ist chaotisch: Jede Änderung an der Reihenfolge der Systeme oder an den
  Geldflüssen des Staates verschiebt alle Verläufe. Tests, die an einem Seed hängen, müssen
  dann neu begründet werden (nie die Aussage abschwächen, sondern das Maß verbessern).
- Die Headless-Werkzeuge lesen `assets/data` relativ zum aktuellen Verzeichnis: immer im
  jeweiligen Arbeitsverzeichnis starten oder `--data-dir` angeben.
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
