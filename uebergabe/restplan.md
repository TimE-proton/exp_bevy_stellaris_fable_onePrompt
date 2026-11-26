# Plan: KONFLUENZ – jetzt sichern und pausieren, ab 29.11. Meilenstein 4 bis 7 fertigstellen

Stand 26.11.2026, 07:40 UTC.

## Kontext

Auftrag bleibt: das 4X-Spiel bis Meilenstein 7 fertigstellen. Meilenstein 1–3 liegen im
Hauptzweig `claude/keen-franklin-aqi4h7`. Meilenstein 4 lief in vier parallelen Strängen.

- **Kontingent des Hauptmodells erschöpft** (Reset Sonntag, 29.11., 03:00 UTC). Daran sind gescheitert:
  die Nachbesserung der Schlachtansicht, die Nachbesserung der strategischen Ebene und drei
  Gegenprüfungen.
- Die fertigen Stränge liegen **nur lokal im Container** und gehen verloren, wenn er
  abgeräumt wird. Dasselbe gilt für Entwürfe, Prüfbefunde und Agentenberichte.
- Verbrauch bisher laut Sitzungsdaten: rund 10 Mio. Ausgabe-Tokens (rechnerisch ≈ 1.100 USD);
  das Wochenkontingent war nach etwa 13 Stunden verbraucht. Die Prüf-Workflows kosteten je
  7–9 Mio. Agenten-Tokens und rund 7,5 Stunden.

**Deine Entscheidungen von heute**

1. Modell: auf das Hauptmodell der Sitzung warten. Jetzt nur sichern und pausieren, weiter nach dem Reset.
2. Prüftiefe: schlank (etwa 7 Agenten je Paket statt 20–30).
3. Sicherung: Zwischenstände dürfen als `wip/…`-Branches nach GitHub.

## Stand

| Teil | Wo | Stand |
|---|---|---|
| M1–M3 | Hauptzweig, gepusht | fertig |
| Gefechtsrechner | `wip/m4-taktik` 35801ce | fertig, geprüft, 18 Befunde behoben |
| Technologie | `wip/m4-tech` d314838 | fertig, geprüft, 13 Befunde behoben |
| Gefechtsrechner + Technologie zusammen | `wip/m4-integ` 9c63cad | Tests und Clippy grün (Kern, Headless) |
| Strategische Ebene (Schiffe, Werften, Nachschub, Gefechte in der Partie) | `wip/m4-sim` 4b64d65 | umgesetzt, geprüft; 17 bestätigte Befunde **offen** |
| 3D-Schlachtansicht, Gefechtsübung | Hauptzweig 8206ed1 | umgesetzt, geprüft; 19 bestätigte Befunde **offen**, 3 ungeprüft |
| Entwürfe M5 und M6 | Notizverzeichnis `entwurf/` (12 Dateien) | fertig |

Schwerste offene Befunde:

- Spiel: „Zum Hauptmenü" aus einer Partie lässt das Spiel abstürzen; Klicks und Mausrad
  wirken durch Leisten auf Szene und Karte durch; Inspektor versteckt Modulzeilen;
  Aufstellungsdialog nimmt jede Schiffszahl; Selbsttest besteht auch ohne sichtbare Szene.
- Strategische Ebene: erzwungener Rückzug aufhebbar; unentschiedene Gefechte starten endlos
  neu; Koloniewerften flackern, Aufträge frieren still ein; 30 Tage Unangreifbarkeit bei
  voller Bewegungsfreiheit; Stellunghalten kostet nichts; Flottenkosten ohne Spur im Haushalt.

## Teil I – jetzt, nach deiner Freigabe (eigenhändig, keine Agenten, keine Entwicklung)

1. **Stränge sichern**: `git push -u origin` für `wip/m4-taktik`, `wip/m4-tech`,
   `wip/m4-sim`, `wip/m4-integ`. Der Hauptzweig bleibt unverändert und lauffähig; kein
   Pull Request. Scheitert der Push technisch, sichere ich die Stränge über die
   GitHub-Schnittstelle (ein Commit je Strang) und melde es.
2. **Übergabe-Branch `wip/uebergabe`** (vom Hauptzweig, ein Commit, nur Dokumente unter
   `uebergabe/`), damit nach dem Abräumen des Containers nichts fehlt:
   - dieser Restplan und eine Kurzanleitung zum Wiedereinstieg;
   - die 12 Entwurfsdokumente für M5 und M6 (`M5_Synthese.md`, `M6_Synthese.md`, Entwürfe, Jury);
   - alle Prüfbefunde zu Schlachtansicht und strategischer Ebene im Wortlaut, mit Urteil
     und Begründung der Gegenprüfer (aus den Workflow-Protokollen exportiert);
   - die Agentenberichte mit Schnittstellennotizen, Messwerten und offenen Punkten
     (Gefechtsrechner, Technologie, strategische Ebene, Schlachtansicht);
   - die fünf Workflow-Skripte als Vorlagen.
   Vor dem Commit prüfe ich, dass keine persönlichen Daten darin stehen.
3. **Notizen im Projektgedächtnis**: Stand und Branches, wo die Übergabe liegt, deine drei
   Entscheidungen, Reset-Zeitpunkt.
4. Aufgabenliste aktualisieren, Kurzmeldung mit Branch-Namen und Commits. Danach Pause.

Ein automatischer Start nach dem Reset wird **nicht** eingeplant, weil er dein neues
Wochenkontingent ohne dein Zutun anbrechen würde. Du schreibst ab So., 29.11., 03:00 UTC
„weiter" (hier oder in einer neuen Sitzung auf demselben Repo). Wenn du lieber einen
geplanten Start möchtest, sag es bei der Freigabe.

## Teil II – ab 29.11., nach deinem Startsignal

### Wiedereinstieg

- Branches per Namen holen (`git fetch origin wip/m4-integ` usw.), Arbeitsverzeichnisse
  unter `.worktrees/` neu anlegen, `uebergabe/` lesen, Werkzeuge für die Bildprüfung
  einrichten (Notiz `reference_cloud_render_check`), erster Bau des Spiels (≈ 20 Minuten).

### Arbeitsweise je Paket (schlank)

1. **Umsetzen**: ein bis zwei Agenten nacheinander im Arbeitsverzeichnis des Pakets;
   Commits dort, früh und oft.
2. **Prüfen**: zwei bis drei Prüfer mit getrennten Blickwinkeln (Korrektheit und Geldfluss;
   Determinismus und Integration; Anforderungen und Ehrlichkeit der Tests; bei Spielpaketen
   Bedienung mit echten Klicktests über `xdotool` unter Xvfb).
3. **Gegenprüfen**: je Prüfer ein Skeptiker für alle Befunde dieses Prüfers.
4. **Nachbessern**: ein Agent behebt die bestätigten Befunde, alle Pflichtbefehle grün.
5. Ich führe zusammen, lasse Tests, Clippy, Sweeps und Selbsttests selbst laufen, sehe mir
   die Bilder an und pushe.

- Modell: das Hauptmodell der Sitzung. Das Ausweichmodell nur dort, wo der Schutzfilter das Hauptmodell stoppt (bisher der
  Gefechtsrechner; beim Belagerungsrechner möglich). Jeder Stopp wird gemeldet.
- **Nach jedem Agentenschritt wird der Branch des Pakets gepusht**; lokal ungesicherte
  Arbeit gibt es nicht mehr.
- Höchstens drei Pakete gleichzeitig (4 Kerne). Agenten arbeiten nie im Hauptverzeichnis.

### Meilenstein 4 abschließen

- **Paket A – Kern zusammenführen** (`.worktrees/integ`): `wip/m4-sim` einmergen (Konflikte in
  `crates/headless/src/main.rs`, `crates/sim/src/economy/{capital,mod,production}.rs`,
  `crates/sim/src/harness.rs`); `crates/sim/src/fleet/battles.rs` auf die neue
  Gefechts-Schnittstelle umstellen (`Battle::results`, `Fate`,
  `CombatantSetup::with_war_weariness`, Patt); Forschung verdrahten (`Research::effects` →
  `CombatModifiers` und Flottentempo, `GameData::best_design` nach bekannten Technologien,
  `Research::add_intel` nach Gefechten); die 17 Befunde der strategischen Ebene beheben,
  darunter ein echter Haushaltszwang, damit Flotten und Forschung um dasselbe Geld konkurrieren.
- **Paket B – Spiel an den neuen Kern, Befunde der Schlachtansicht** (`.worktrees/spiel`):
  Übersetzungsfehler in `crates/game` beheben, dann die 19 Befunde (Absturz in
  `crates/game/src/session.rs`, Durchklicken in `battle_view/{hud,camera}.rs` **und**
  `crates/game/src/map.rs`, Inspektor, Dialog, Selbsttest, kleinere), README zur Gefechtsübung.
- **Paket C – Militär-KI und Oberfläche**:
  Kern: `crates/sim/src/ai/levers.rs` (Schiffbau nach Etat und Kontertabelle aus
  `cargo headless battle --matrix`, Flotten im Frieden und im Krieg), Kriegsskript im
  Harness, Sweep-Kennzahlen.
  Spiel: Gefechte der Partie in der 3D-Ansicht (`BattleFeed`/`BattleOrder` aus
  `Battles.active`), Gefechtsmarken auf der Karte, Flottenfenster (Schiffe, Bauliste,
  Befehle), Technologiefenster (Baum, Ziel, Etat, Schätzungen über andere).
- Abschluss: README, Meilenstein-Commit im Hauptzweig, Push, Bilder.

### Meilenstein 5 – Belagerung (nach `uebergabe/entwurf/M5_Synthese.md`)

- **Paket D – Besitzwechsel, Nachfolge, Enden** (parallel zu A): ein einziger Pfad für
  Eigentümerwechsel und das Ende einer Gemeinschaft, den M5 und M6 beide brauchen:
  `crates/sim/src/polity/succession.rs` (`spawn`, `transfer_planet`, `absorb`,
  `subordinate`), `crates/sim/src/endgame.rs` (`Endgame`, reine Funktion `classify`,
  monatliches Urteil, Partie hält nach dem Ende an), `apply_secession` in
  `crates/sim/src/finance/cohesion.rs` darauf umgestellt, Invarianten in
  `crates/sim/src/harness.rs`, Stellschrauben `endgame` in `assets/data/balance.ron`.
- **Paket E – Belagerungsrechner und stehende Verteidigung** (parallel zu A; Schritte 1, 3,
  5 des Entwurfs): `crates/sim/src/siege/{mod,step,plugin}.rs`, reiner Tagesrechner mit
  Prognose, Tests 1–8, Beschaffung und Unterhalt im Frieden, Befehle `SetDefensePlan`,
  `SetGarrisonOrders`, Werkzeug `cargo headless siege`.
- **Paket F – Belagerung in der Partie** (Schritte 4, 6; braucht A, D, E): `orbit.rs`,
  Tagesablauf, acht Befehle, Eroberung über `transfer_planet`, Haken in Produktion, Kursen
  und Zusammenhalt, Lesemodell, Tests 9–16.
- **Paket G – Belagerungs-KI und Sweeps** (Schritt 7), **Paket H – Belagerungsansicht** (Schritt 8).

### Meilenstein 6 – KI, Diplomatie, Enden (nach `uebergabe/entwurf/M6_Synthese.md`)

- **Paket I** – Beziehungen, Verträge, Vorschläge, Embargo, Durchfahrt, Sperrguthaben,
  Kriegsstand und Friedensbedingungen, Schutzgebiete (Schritte 3–4; braucht D).
- **Paket J** – verborgene Dispositionen: `choice_probs`, Anlässe, `diplomacy.ron`,
  Indizien und Einschätzung ohne Blick in den Kopf der KI (Schritte 5–6).
- **Paket K** – operative KI (Haltung, Flotten, Anteile) und Sweeps mit Skriptspielern;
  Ziel: jede Partie endet genau einmal (Schritte 7–8).
- **Paket L** – Spiel: Fenster „Diplomatie" und „Analyse", Endspiel-Leiste, Abschlussbild
  „Metamorphose" / „Auslöschung", Auto-Pause.

### Meilenstein 7 – Abschluss

- **Paket M** – Speichern/Laden im Menü, Tutorial, UI-Feinschliff, Balance aus Sweeps
  (32 Seeds, bis 250 Jahre; offene Punkte des Gefechtsrechners), Abschlussprüfung des ganzen
  Repos, README, `wip/…`-Branches und Arbeitsverzeichnisse aufräumen.

### Reihenfolge

1. A ‖ D ‖ E
2. B ‖ C-Kern, dann C-Spiel → **Meilenstein 4 im Hauptzweig**
3. F ‖ I
4. G ‖ J ‖ H → **Meilenstein 5**
5. K ‖ L → **Meilenstein 6**
6. M → **Meilenstein 7**

Nach jedem Meilenstein: `cargo run` startet, Tests und Clippy grün, Commit, Push,
Kurzmeldung mit Bildern, Detailentscheidungen und Verbrauch. Dort kannst du jederzeit stoppen.

## Verifikation

- Teil I: `git ls-remote origin 'wip/*'` zeigt die fünf Branches mit den erwarteten Commits;
  `git status` im Hauptverzeichnis sauber; Notizen mit `memory_list` sichtbar.
- Teil II, Kern: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --check`, `cargo headless hash --seed 3 --days 3600` zweimal und mit `--exact`,
  `cargo sweep --seeds 0..8 --years 60` (ab Paket C auch `--war`) ohne Invariantenverletzung.
- Teil II, Spiel: `xvfb-run -a -s "-screen 0 1600x900x24" cargo run -q -p game -- --selftest N`
  und `--practice`, Bildschirmfotos ansehen; Klickpfade mit `xdotool` gegen ein laufendes Fenster.
- Enden (M6): Sweep über viele Seeds, jede Partie endet mit genau einem Ende.

## Aufwand und Risiken

- Teil I: wenige Minuten, kaum Verbrauch.
- Teil II: grob 20–30 Stunden Laufzeit. Auch schlank geprüft kann der Rest **mehr als ein
  Wochenkontingent** brauchen; dann sichere ich wieder und melde mich.
- Unverändert: Push auf den Hauptzweig nur lauffähig, kein Pull Request, Texte deutsch,
  am Gefechtsrechner arbeite ich nicht eigenhändig, Bildprüfung nur mit Software-Rendering.
- Die Zahlen in den Entwürfen M5/M6 sind ungetestete Schätzungen; Balance entsteht aus Sweeps.
