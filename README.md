# KONFLUENZ

Ein 4X-Strategiespiel in einer endlichen Galaxie. Wirtschaft, Finanzmärkte und Krieg hängen
kausal zusammen, und am Ende bleibt genau eine galaktische Gemeinschaft übrig: durch
Verschmelzung oder durch Auslöschung.

Rust, [Bevy](https://bevy.org) 0.20, Einzelspieler gegen KI-Gemeinschaften. Arbeitstitel.

## Starten

```bash
cargo run
```

Startet das Spiel. Der erste Build übersetzt Bevy vollständig und dauert einige Minuten.

```bash
cargo dev
```

Entwicklungsstart mit dynamisch gelinktem Bevy: nach Änderungen am eigenen Code wird in
Sekunden neu gelinkt. Nicht für Releases.

```bash
cargo headless run --seed 7 --years 50
```

KI gegen KI ohne Grafik, mit Chronik und Zusammenfassung.

```bash
cargo sweep --seeds 0..32 --years 100
```

Viele Seeds parallel, Kennzahlen als Tabelle (`--csv datei.csv` schreibt sie zusätzlich weg).

```bash
cargo headless economy --seed 7 --years 40
```

Volkswirtschaftliche Kennzahlen im Jahresabstand (Versorgung, Auslastung, Preisniveau,
Geldverteilung), dazu Markt und Werke eines Systems. Das Werkzeug zum Auswuchten.

```bash
cargo headless fleets --seed 7 --years 20 --order speer:1,dolch:2
```

Flotten, Schiffe, Vorräte, Werften und Bauaufträge je Gemeinschaft. Mit `--order` gibt
jede Gemeinschaft dieselben Schiffe bei der Werft ihrer Hauptwelt in Auftrag; `--ships`
listet jedes Schiff einzeln.

```bash
cargo headless hash --seed 7 --days 3600
```

Zustandsprüfsummen. Zwei Aufrufe mit gleichem Seed müssen dieselben Zahlen liefern.

### Voraussetzungen

- Rust 1.97.1 (wird über `rust-toolchain.toml` automatisch gewählt).
- Linux: ein Vulkan-Treiber und `libxkbcommon-x11-0`. Audio und Gamepad sind abgeschaltet,
  ALSA- und udev-Entwicklungspakete werden deshalb nicht gebraucht.
- Fenster laufen über X11 (auf Wayland-Desktops über XWayland). Natives Wayland:
  `cargo run --features wayland`, braucht `libwayland-dev` und `libxkbcommon-dev`.

### Prüfen

```bash
cargo test --workspace
```

```bash
cargo lint
```

`cargo lint` ist `clippy --workspace --all-targets -- -D warnings`. Die einfachen Befehle
`cargo test` und `cargo clippy` decken Kern und Spiel ab; das Headless-Programm ist nur
eine dünne Kommandozeile über `sim::harness` und wird mit `--workspace` mitgeprüft.

## Bedienung

| Eingabe | Wirkung |
|---|---|
| Linksklick | System wählen |
| Rechte oder mittlere Maustaste ziehen, WASD, Pfeile | Karte verschieben |
| Mausrad | Zoomen zum Mauszeiger |
| Leertaste | Pause |
| 1 – 6 | Geschwindigkeit ½× bis 25× (1× = ein Tag je Sekunde) |
| + / − | schneller / langsamer |
| F5 / F9 | Schnellspeichern / Schnellladen (`saves/schnell.ron`) |
| Reiter links | Übersicht, Markt (Preise mit Verlauf), Produktion (Werke, „Was sich hier lohnt"), Besitz (Zusammenhalt, Eigentümer) |
| Knöpfe oben rechts | Warenflüsse, Produktionsketten, Börse, Staatsfinanzen, Flotten |
| F1 | Hilfe |
| Esc | Menü |

## Aufbau

```
assets/data/*.ron   Spieldaten: Güter, Rezepte, Planetentypen, Galaxie, Gemeinschaften, Balance
crates/sim/         Simulationskern, ohne Rendering, Fenster oder Eingabe
crates/game/        Bevy-App: Karte, Oberfläche, Eingabe
crates/headless/    Kommandozeile über sim::harness
```

**Kern (`sim`).** Jedes Teilsystem ist ein Modul mit reinen Daten und Funktionen und einem
Bevy-Plugin, das den Zustand als Resource hält und Systeme in `FixedUpdate` einhängt.
`game` und `headless` laden dieselbe Plugin-Gruppe `SimPlugins`; die Reihenfolge eines
Takts steht an genau einer Stelle (`SimSet` in `crates/sim/src/lib.rs`).

**Determinismus.** Fester Takt (20 je Sekunde Spielzeit, 20 Takte je Tag), eigener
Zufallsgenerator mit Strömen je Teilsystem, einfädiges Schedule mit Mehrdeutigkeiten als
Fehler, Zustand nur in `Vec` und `BTreeMap`. Gleicher Seed, gleiche Befehle und gleiches
Programm ergeben denselben Verlauf. Bit-Gleichheit zwischen verschiedenen Plattformen oder
zwischen Debug- und Release-Build wird nicht zugesagt.

**Befehle.** Spieler und KI wirken nur über `SimCommand` auf die Simulation. Befehle von
außen werden mit Takt und laufender Nummer protokolliert; Seed, Datenprüfsumme und
Protokoll genügen für eine exakte Wiedergabe.

**Spieldaten.** Balance steht in `assets/data/*.ron` und wird beim Start geladen und
geprüft. Ändern, neu starten, fertig; neu übersetzt wird nichts. Ein anderes Verzeichnis
wählt `--data-dir PFAD` oder die Umgebungsvariable `KONFLUENZ_DATA`.

## Wie die Wirtschaft rechnet

- **Produktion.** Jeder Planet hat je Rezept einen Kapitalstock. Was ein Werk abwirft,
  ergibt sich aus dem lokalen Vorkommen, dem Lohn (viele Menschen, billige Arbeit), dem
  Energiebedarf (Kälte, Hitze, Schwerkraft), den Startkosten in den Orbit und den Preisen
  im System. Kapital wandert monatlich in das Rezept mit der höchsten erwarteten Rendite;
  Gebäudeboni gibt es nicht. Deckt der Erlös nicht einmal Material und Energie, steht das
  Werk still.
- **Preise.** Je System und Gut aus der Lagerreichweite: Bestand gegen kaufkräftige
  Tagesnachfrage, mit Boden, Decke und begrenztem Tagesschritt.
- **Warenflüsse.** Frachter der Handelshäuser kaufen billig, fahren Kante für Kante durch
  das Routennetz und verkaufen teuer. Ein blockiertes System erreicht niemand; Kaperer
  bringen Frachter auf, Geleitschutz hält sie ab. Fehlt die Lieferung, fehlen die
  Vorprodukte: Die Blockade wirkt über die Ware.
- **Geld.** Ganzzahlig und geschlossen: Es wird nur verschoben. Neu entsteht es allein für
  Neugeborene, ausgewiesen als Geldschöpfung. Ein Test und jeder Sweep prüfen die Summe.
- **Sicherungen gegen Verklemmen.** Kontore lagern nur bis zu einer Obergrenze, Werke
  laufen ohne Treibstoff im Notbetrieb weiter, Handelshäuser behalten eine Mindestflotte,
  junge Kolonien haben eine Schonfrist. Engpässe schmerzen, sind aber nie endgültig.

## Besitz und Zusammenhalt

- **Emittenten und Halter.** Firmen und Werften (je Planet und Wirtschaftszweig),
  Handelshäuser und die Kontore der Systeme geben Anteile aus. Halter sind Staaten, die
  Elite einer Region und der Bevölkerungsfonds einer Region. Zu welchem Reich ein Halter
  gehört, folgt dem Besitz seines Planeten.
- **Dividenden** fließen an die Halter, wo immer sie sitzen. Schießt die Hauptwelt Kapital
  in eine Kolonie nach, erhält sie neue Anteile: Gerade die erfolgreichen Kolonien gehören
  mit der Zeit dem Zentrum, und ihre Gewinne fließen ab.
- **Kurse** folgen den Gewinnen (Vielfaches des Jahresgewinns, nie unter einem Bruchteil
  des Buchwerts), gedrückt durch Risiko (Blockade, Krieg, feindliche Flotten, Unruhe,
  Misstrauen gegen den Staat) und durch Ereignisse, deren Wirkung abklingt.
- **Zusammenhalt** je Planet, 0 bis 100. Es bindet, wenn die Einheimischen am übrigen Reich
  beteiligt sind und der Besitz an ihren Betrieben breit gestreut ist. Es trennt, wenn das
  Zentrum die Mehrheit hält, Gewinne einseitig abfließen oder Fremde beteiligt sind; dazu
  kommen Versorgung, Kriegsmüdigkeit, Steuern, Blockade, Entfernung und Enteignungen. Der
  Reiter „Besitz" zeigt jeden Beitrag in Punkten.
- **Stufen** mit Verweildauern: loyal, unruhig, autonom (zahlt keine Steuern mehr),
  Abspaltung. Eine abtrünnige Region ruft einen Freistaat aus oder tritt zu der fremden
  Gemeinschaft über, die dort die Mehrheit hält.
- **Werkzeuge des Staates:** Anteile kaufen, anbieten, an eine Region verteilen,
  Übernahmeangebot, Enteignung (wirkt sofort, kostet Kurse und Vertrauen über Jahre),
  Staatsanleihen (Zins steigt mit Schulden und Kriegsmüdigkeit, Ausfall möglich),
  Steuersatz und Rücklage.
- **Geldmenge.** Fällt das Preisniveau unter das Ziel, schöpft die Notenbank Geld für den
  Staat. Die Schöpfung ist ausgewiesen und Teil der Geldsummenprüfung.

Alle Gewichte und Schwellen stehen in `assets/data/balance.ron` unter `finance`.

## Flotten, Werften und Nachschub

- **Schiffe.** Eine Flotte besteht aus Schiffen mit Entwurf, Rumpf- und Modulzustand,
  Munition, Treibstoff und Bordjägern. Ihre Stärke ist abgeleitet: Bauwert mal Zustand,
  geeicht an der Startflotte. Kaperfahrt, Geleitschutz und Blockade rechnen mit dieser Zahl.
- **Werften.** Eine Welt baut Schiffe, wenn in ihrem Zweig Rüstung Kapital steht; je
  `yard_capital_per_slot` ein Bauplatz, die Hauptwelt hat immer einen. Der Staat kauft
  täglich den Tagesanteil der Bauteile am Markt des Systems und zahlt der Rüstungsfirma
  der Welt einen Aufschlag, an dem ihre Anteilseigner mitverdienen. Fehlt ein Bauteil oder
  Geld, stockt der Bau: Eine blockierte Werft steht still, weil keine Ware mehr ankommt.
- **Nachschub.** Im eigenen, nicht blockierten System fasst eine Flotte Munition und
  Treibstoff vom Markt nach, bezahlt aus der Staatskasse. Marschieren kostet Treibstoff;
  mit leerem Tank schleppt sich eine Flotte nur noch mit einem Bruchteil ihrer Fahrt.
- **Instandsetzung.** Im eigenen System mit Werft, gegen Rumpfsegmente vom Markt.
- **Sold.** Monatlich ein Anteil am Bauwert aller Schiffe, aus der Staatskasse an die
  Haushalte. Reicht die Kasse nicht, wächst der Soldrückstand der Gemeinschaft.
- **Befehle.** Schiffe bestellen und Aufträge streichen, Flotten teilen und vereinigen,
  Aufstellung und Doktrin als stehende Befehle für künftige Gefechte.

Die Stellschrauben stehen in `assets/data/balance.ron` unter `military`.

## Stand

| Meilenstein | Inhalt | Stand |
|---|---|---|
| 1 | Workspace, Galaxie, Planeten, Karte, Kamera, Zeitsteuerung | fertig |
| 2 | Wirtschaft: Produktion, lokale Preise, Frachter, Warenflüsse | fertig |
| 3 | Aktienmarkt, Besitzverteilung, Zusammenhalt, Abspaltung | fertig |
| 4 | Raumkampf mit Schiffsklassen und Modulen | offen |
| 5 | Belagerung, Orbitalgeschütze, Boden- und Untergrundkampf | offen |
| 6 | KI-Gemeinschaften, Diplomatie, verborgene Dispositionen, Enden | offen |
| 7 | Speichern/Laden im Menü, Balance, Feinschliff, Tutorial | offen |

Bereits in Meilenstein 1 enthalten, weil spätere Stufen darauf bauen: Gemeinschaften mit
Planetenbesitz, Koloniegründung, Befehlsprotokoll mit Wiedergabe, Spielstände. Gefechte
auf der Karte sind noch grob aufgelöst (Stärke gegen Stärke, Tag für Tag), bis der
taktische Rechner aus Meilenstein 4 angeschlossen ist.
