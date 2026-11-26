# Plan: KONFLUENZ – Kontingent erneut erschöpft: Zwischenstände sichern, pausieren, danach Meilenstein 4 zuerst

Stand 26.11.2026, 13:10 UTC. Ersetzt den Plan von 07:40 UTC.

## Kontext

Der Auftrag bleibt: das 4X-Spiel bis Meilenstein 7 fertigstellen. Seit der Wiederaufnahme
(07:40 UTC) liefen vier Stränge parallel. Gegen 12:40 UTC war das Kontingent des Hauptmodells
wieder erschöpft (Reset **Sonntag, 29.11., 03:00 UTC**). Alle vier Stränge wurden mitten in
einem Agentenschritt gestoppt.

- Alles Committete liegt auf GitHub (mit `git ls-remote` gegen die lokalen Stände geprüft).
- **Drei angefangene Schritte liegen uncommittet nur im Container** und wären verloren, wenn
  er abgeräumt wird: Staatshaushalt im Kern (23 Dateien, +831/−149), Nachbesserung der
  Schlachtansicht (7 Dateien, +1233/−188), Rückwirkungen der Belagerung (2 Dateien, +120).
  Sie zu committen ist Schritt 1 von Teil I.
- Verbrauch laut Sitzungsdaten seit 07:40 UTC: rund 4,2 Mio. Ausgabe-Tokens (rechnerisch
  ≈ 585 USD) in fünf Stunden. Sitzung gesamt: 14,2 Mio. (≈ 1.685 USD).
- 16 aktuelle Bilder sind verschickt (Partie, Fenster, Schlachtansicht, Großgefecht mit
  400 Schiffen).

## Stand nach diesem Lauf

| Paket | Branch (Commit) | Stand |
|---|---|---|
| A1 Kern zusammengeführt | `wip/m4-integ` 5ac904d | fertig: Gefechte der Partie über den neuen Rechner, Forschung wirkt auf Kampf, Tempo, Entwürfe; 264 Kerntests grün |
| A2 Befunde der strategischen Ebene, Staatshaushalt | `wip/m4-integ` 8758cd7 | committet: Gefechte (Abzug bindet, Flotten scheiden einzeln aus), Werften (Rüstungsbetriebe, Aufträge mit Stand und Grund), Treibstoff auf Station, Verlustmaß, Drehbuch für Bestellungen. **Angefangen:** Staatshaushalt (`crates/sim/src/economy/budget.rs` neu). Offen: Abschluss, Tests, Militär-KI, Prüfung |
| B1 Befunde der Schlachtansicht | `wip/m4-spiel` 5f997cb | alle 12 Befunde behoben, 130 Tests (vorher 64), Rundgang per Klick als Test. Prüfung fertig: 7 bestätigte Nachbefunde. **Angefangen:** deren Nachbesserung |
| D Nachfolge und Enden | `wip/m6-nachfolge` 816d69c | Umsetzung fertig und grün: ein Pfad für Besitzwechsel und das Ende einer Gemeinschaft, Schutzgebiete, die beiden Enden als reine Funktion mit monatlichem Urteil. Prüfung halb: 4 Befunde nicht gegengeprüft, zweiter Blickwinkel fehlt |
| E1 Belagerungsrechner | `wip/m5-rechner` 2764a4a | fertig |
| F1 Belagerung in der Partie | `wip/m5-rechner` ec9c98d | erste Hälfte fertig (Orbit, Tagesablauf, Eroberung gewöhnlicher Welten, fünf Befehle; 327 Kerntests grün). Zweite Hälfte: Landung, Übergabe, Plünderung committet. **Angefangen:** Haken in Produktion und Kursen. Offen: Kriegsmüdigkeit, Monatsbuchführung, Tests 10, 11, 15, 16 |

**Bestätigte Nachbefunde im Spiel** (Auftrag der Nachbesserung):

1. Schwer: „Laden (F9)" in einer laufenden Partie beendet das Spiel, wenn der Spielstand
   eine kleinere Galaxie hat (`crates/game/src/session.rs`; derselbe Mechanismus wie beim
   behobenen Absturz „Zum Hauptmenü").
2. Mausblick (M oder rechte Taste): Ein Klick wirkt auf Tafel und Szene zugleich
   (`crates/game/src/pointer.rs`).
3. Die Rollposition der Modulliste wandert ins nächste Gefecht (`battle_view/hud.rs`).
4. Ein Fenster als sehr schmaler Streifen beendet die Schlachtansicht
   (`battle_view/camera.rs`).
5. Der Selbsttest zählt Sichtbarkeit für irgendeine aktive Kamera (`battle_view/scene.rs`).
6. Ein schneller Klick auf die Griffleiste neben einer Tafel wirkt auf Tafel und Karte.
7. Die Fluggeschwindigkeit des vorigen Gefechts gilt im nächsten weiter.

Zwei weitere Befunde haben die Gegenprüfer verworfen.

**Ungeprüfte Befunde der Nachfolge:** `DistributeShares` kann Anteile an den Staat einer
beendeten Gemeinschaft geben; in zwei Fällen werden Verluste aus einem laufenden Gefecht der
Erbin angelastet; alte Spielstände werden noch angenommen (erledigt sich, weil ich
`SAVE_FORMAT` bei der Zusammenführung erhöhe).

**Eigene Beobachtungen aus den Bildern von heute** (gehen in die Aufträge der Oberfläche):

- Verbandstafeln der Schlachtansicht: Schiffe im Rückzug stehen in keiner der Spalten
  „kämpft / verloren / entkommen" (im Großgefecht fehlten 6 Schlachtschiffe und 4 Träger).
- Seitentafeln und Chronik der Partie sind leicht durchsichtig, Kartenbeschriftung scheint durch.
- Eigener Klicktest: Klicks auf Leisten ändern die Auswahl auf der Karte nicht mehr;
  Partie → Hauptmenü → Gefechtsübung lief ohne Absturz.

## Teil I – sofort nach deiner Freigabe (eigenhändig, ohne Agenten)

1. **Sichern, als Erstes:** in `.worktrees/spiel`, `.worktrees/integ` und
   `.worktrees/belagerung` je ein Commit „Zwischenstand vor Kontingentende: unvollständig,
   ungeprüft" auf dem jeweiligen `wip/…`-Branch, dann `git push -u origin <branch>`.
   Hauptzweig unverändert, kein Pull Request.
2. **Bauzustand festhalten** (nur Rechenzeit): ein Hintergrundlauf mit `cargo test -p sim`
   in `integ` und `belagerung` und `cargo test -p game` in `spiel`. Das Ergebnis (baut,
   welche Tests rot) kommt in die Übergabe. Repariert wird jetzt nichts.
3. **Übergabe-Branch `wip/uebergabe`:** Abschlussberichte von B1, D und F1 nach
   `uebergabe/berichte/`; die Nachbefunde im Spiel mit Urteil und die ungeprüften Befunde
   der Nachfolge nach `uebergabe/befunde/`; die vier Workflow-Skripte nach
   `uebergabe/workflows/`; `uebergabe/README.md` (Stand, Branch-Tabelle, letzter geprüfter
   Commit je Strang, was Zwischenstand ist) und `uebergabe/restplan.md` (dieser Plan).
   Export mit dem vorhandenen `scratchpad/uebergabe_tools/bericht.py`.
4. **Projektnotiz** und Aufgabenliste auf den neuen Stand.
5. **Platz schaffen** (frei sind 9,8 GB): regenerierbare Bau- und Prüfkopien im
   Notizverzeichnis löschen, nach einem Blick hinein: `spiel/{nb,pruefung,bin}`,
   `nachfolge/{pruefung,mutcopy}`, `game/review`, `sim/{probe_regroup,target_probe,fix}`,
   `tech/fix`, `taktik/{old,exp,copy}` (zusammen etwa 14 GB). Bilder und Logs bleiben.
6. Kurzmeldung mit Commits. Danach **Pause, kein automatischer Start.** Du schreibst nach
   dem Reset „weiter", hier oder in einer neuen Sitzung auf demselben Repo.

Modell: Wie am 26.11. entschieden, warte ich auf das Hauptmodell und verlagere nichts auf andere
Modelle. Wenn du die Zeit bis zum Reset doch mit dem Ausweichmodell überbrücken willst (etwa nur die drei
angefangenen Schritte abschließen), sag es bei der Freigabe.

## Teil II – nach dem Reset, auf dein Startsignal

### Haushalten mit dem Kontingent (meine Detailentscheidungen)

- **Meilenstein 4 zuerst**, vollständig bis in den Hauptzweig. Erst danach M5, M6, M7. Die
  gesicherten Stränge für M5 und M6 ruhen so lange.
- **Höchstens zwei Stränge gleichzeitig** (Kern und Spiel). Mit vier Strängen auf vier
  Kernen dauerte ein Testlauf des Kerns 4 bis 10 Minuten; das Warten kostet Tokens, weil
  jede Abfrage den ganzen Kontext des Agenten neu liest.
- Prüftiefe bleibt „schlank" wie von dir entschieden. Sie macht etwa ein Viertel des
  Verbrauchs aus; der große Posten ist die Umsetzung.
- Agenten setzen auf den Zwischenständen auf: erst sichten, dann übernehmen oder
  verwerfen. Als Beleg je Korrektur genügt ein Test oder ein Foto.
- Sweeps, Hash-Vergleiche, Selbsttests, Klicktests und Bilder mache ich selbst zwischen den
  Agentenschritten.

### Meilenstein 4 abschließen

Kern (`.worktrees/integ`, Skript `scratchpad/workflows/m4-kern-befunde.js` mit angepasstem
ersten Auftrag):

1. **A2 abschließen:** Staatshaushalt mit fester Reihenfolge der Ansprüche (Schuldendienst;
   Sold und Unterhalt; Schiffbau, Auffüllen, Reparatur; Forschung; Reserve; Ausschüttung an
   die Haushalte) in `crates/sim/src/economy/budget.rs`, `fleet/supply.rs`, `tech/step.rs`,
   `finance/plugin.rs`; Lesemodell je Gemeinschaft; ehrliches Maß für
   `larger_economies_research_faster_in_a_running_game` in `crates/sim/tests/tech.rs`;
   Sweep-Tabelle vorher/nachher.
2. **C-Kern, Militär-KI:** `crates/sim/src/ai/levers.rs` als einzige KI-Datei mit Flotten-,
   Werft- und Forschungsbefehlen; Rüstung nach Etatanteil und Bedrohung; Klassenwahl nach
   einer Kontertabelle aus `cargo headless battle --matrix`; Flottenführung in Frieden und
   Krieg; Kriegsskript „Grenzkrieg"; Kennzahlen am Ende von `RunSummary` und CSV.
3. Prüfung (Korrektheit und Geldfluss; Determinismus und Integration; Anforderungen und
   Ehrlichkeit der Tests), je eine Sammel-Gegenprüfung, Nachbesserung.

Spiel (`.worktrees/spiel`, gemeinsamer Engine-Bau in `target/`):

1. **B1-Nachbesserung abschließen** (die 7 Nachbefunde oben).
2. **B2:** Kern einmergen, API-Drift beheben (`crates/game/src/traffic.rs`: Flottenposition
   über `Fleet::day_range`; Zustände „im Gefecht" und „sammelt sich"; `Retreat` statt
   `MoveFleet` im Gefecht), **Technologiefenster** (Baum, Ziel, Etat, Schätzungen über andere).
3. **C-Spiel:** Gefechte der Partie in der 3D-Ansicht (`BattleFeed`/`BattleOrder` aus
   `Battles.active`, Verlassen und Befehle über die Brücke), Gefechtsmarken auf der Karte,
   **Flottenfenster** (Schiffe, Bauliste, Befehle, Haushalt), dazu die beiden Beobachtungen
   aus den Bildern.
4. Prüfung (Bedienung mit echten Klicktests; Anforderungen), Gegenprüfung, Nachbesserung.

Abschluss: beide Stränge in den Hauptzweig, `SAVE_FORMAT` erhöhen, README, alle Prüfbefehle,
Bilder, Bericht. **Haltepunkt.**

### Meilenstein 5 – Belagerung

- **F1 abschließen** (`.worktrees/belagerung`): Haken in `economy/production.rs` und
  `finance/shares.rs`, Kriegsmüdigkeit, Buchführungshälfte von `siege_month`, Tests 10, 11,
  15, 16.
- **D fertig prüfen** (`.worktrees/nachfolge`): Gegenprüfung der vier Befunde, zweiter
  Blickwinkel (Enden, Determinismus, Ehrlichkeit der Tests), Nachbesserung.
- **F2:** Kern, Nachfolge und Belagerung zusammenführen; die Nahtstelle `hand_over` in
  `crates/sim/src/siege/conquest.rs` durch `transfer_planet` ersetzen (Hauptwelt fällt,
  Gemeinschaft endet; Tests 12, 13); Beschaffung und Unterhalt der Verteidigung als Anspruch
  im Staatshaushalt (`procure_day`, `SetDefensePlan`); Lesemodell; `RunSummary.sieges`.
- Prüfung des ganzen Belagerungsmoduls, Nachbesserung.
- **G:** Belagerungs-KI und Sweeps (offene Balance: Aushungern dominiert, Kolonien ohne
  Vorräte fallen kampflos, Blockade ohne Treibstoff). **H:** Belagerungsansicht im Spiel.
  Meilenstein-Commit. **Haltepunkt.**

### Meilenstein 6 – KI, Diplomatie, Enden (nach `uebergabe/entwurf/M6_Synthese.md`)

- **I:** Beziehungen, Verträge, Vorschläge, Embargo, Durchfahrt, Kriegsstand und
  Friedensbedingungen, Schutzgebiete; Flottenregeln kennen den Block (Geleit, Blockade,
  Nachschub bei Schutzmacht und Schutzgebiet).
- **J:** verborgene Dispositionen (`choice_probs`, Anlässe, `diplomacy.ron`, Indizien).
- **K:** operative KI und Sweeps mit Skriptspielern; jede Partie endet genau einmal.
- **L:** Spiel: „Diplomatie", „Analyse", Endspiel-Leiste, Abschlussbilder; die Partie hält
  nach dem Ende an, auch wenn die eigene Gemeinschaft aufgeht. **Haltepunkt.**

### Meilenstein 7 – Abschluss

- **M:** Speichern/Laden im Menü, Tutorial, UI-Feinschliff, Balance aus Sweeps (32 Seeds,
  bis 250 Jahre; offene Punkte des Gefechtsrechners auf dem Ausweichmodell), Abschlussprüfung, README,
  `wip/…`-Branches erst nach Rückfrage aufräumen.

## Verifikation

- Teil I: `git ls-remote origin 'refs/heads/wip/*'` zeigt die neuen Commits; `git status
  --short` ist in allen Arbeitsverzeichnissen leer; die Projektnotiz zeigt den neuen Stand.
- Teil II, Kern: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo fmt --check`, `cargo headless hash --seed 3 --days 3600` zweimal und mit
  `--exact`, `cargo sweep --seeds 0..8 --years 60` (auch mit Kriegsskript) ohne
  Invariantenverletzung.
- Teil II, Spiel: `xvfb-run -a -s "-screen 0 1600x900x24" target/debug/game --selftest N
  --seed 3 --screenshot …`, auch mit `--practice`; Klickpfade mit `xdotool` gegen ein
  laufendes Fenster; Bilder ansehen und dir schicken.
- Enden (M6): Sweep über viele Seeds, jede Partie endet mit genau einem der beiden Enden.

## Aufwand und Risiken

- Teil I: etwa 30 bis 40 Minuten, fast nur Testläufe im Hintergrund; kaum Verbrauch.
- Teil II: grob 55 Agentenschritte. Heute waren es 13 in fünf Stunden. Der Rest ist also
  etwa das Vierfache des heutigen Laufs, gemessen am ersten Wochenkontingent **rund zwei
  Wochenkontingente** (Unsicherheit ±50 %). Meilenstein 4 allein braucht etwa ein Viertel
  davon.
- Die Zwischenstände können unbrauchbar sein. Dann verwirft sie der nächste Agent; der
  Verlust ist je Strang höchstens eine knappe Stunde Agentenarbeit.
- Unverändert: Push auf den Hauptzweig nur lauffähig, kein Pull Request ohne Auftrag, Texte
  deutsch, am Gefechtsrechner arbeite ich nicht eigenhändig, Bildprüfung nur mit
  Software-Rendering, an jedem Haltepunkt kannst du stoppen.
