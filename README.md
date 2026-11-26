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
cargo headless run --seed 7 --years 30 --war 10:0:1 --war 12:2:1 --peace 20:0:1 \
    --navy 11:alle:1 --order 14:1:speer:1,dolch:2@kolonie
```

Kriege, Bestellungen und Flottenprogramme nach Drehbuch: Ob Krieg herrscht, entscheidet
noch keine KI; geführt werden die Flotten von ihr. Jede Angabe beginnt mit dem Jahr und
darf beliebig oft stehen:

- `--war JAHR:A:B`: Gemeinschaft A erklärt B den Krieg. `--peace JAHR:A:B` schließt
  Frieden. Rüstung und Flotten führt die KI beider Seiten; ruht ihre militärische
  Führung (`--ai`), schickt das Drehbuch die Flotten von A im Blockadeauftrag in das
  Heimatsystem von B.
- `--preset grenzkrieg`: Gemeinschaft 0 erklärt zu Beginn des Jahres 25 ihrem nächsten
  Nachbarn den Krieg und schließt zu Beginn des Jahres 45 Frieden. Mit
  `grenzkrieg-angreifer-ruht` und `grenzkrieg-nachbar-ruht` erhalten die Flotten der einen
  Seite keine Befehle: der Vergleich einer geführten mit einer untätigen Flotte.
- `--ai WER:ART`: militärische Führung durch die KI für WER (Nummer ab 0 oder `alle`):
  `aktiv` (Vorgabe), `flotte-ruht` (rüstet, aber die Flotten bleiben, wo sie sind) oder
  `ruht` (keine Rüstung, keine Flottenbefehle; für Drehbücher, die alles selbst führen).
- `--order JAHR:WER:SCHIFFE[@ORT]`: WER (Nummer ab 0 oder `alle`) bestellt die Schiffe bei
  der Werft der Hauptwelt, mit `@kolonie` bei der Kolonie mit den meisten Bauplätzen.
- `--navy JAHR:WER:FAKTOR`: WER hält seine Marine von da an auf dem FAKTOR-fachen der
  Startflotte und bestellt jeden Monat nach, was fehlt. `1` ersetzt Verluste, `2`
  verdoppelt die Flotte, `0` beendet das Programm.

Nennt eine Angabe eine Gemeinschaft, die es an dem Tag nicht gibt, ergeht der Befehl
trotzdem und steht abgelehnt im Protokoll; bei `--order`, `--navy` und `--ai` weist das
Programm außerdem auf eine Nummer hin, mit der keine Gemeinschaft beginnt.

Die Ausgabe zeigt die Chronik, abgelehnte Befehle, die Bilanz jedes Gefechts samt Ausgang
und je Gemeinschaft Staatskasse, Kriegsmüdigkeit und Zusammenhalt. `sweep`, `hash` und
`fleets` nehmen dieselben Angaben: So gehen Gefechte, Verluste und Nachbau auch in Sweeps
und Prüfsummen ein. Der Sweep nennt dazu gebaute, verlorene und stillgelegte Schiffe,
Gefechte nach Ausgang, Angriffe, Blockadetage, Monate im Krieg und mit Soldrückstand und
den Anteil abgelehnter KI-Befehle; die CSV führt dieselben Zahlen je Gemeinschaft.

```bash
cargo headless battle --matrix --ron --seeds 0..16
```

Die Kontertabelle `military.counter` für `balance.ron`, abgeleitet aus der Gefechtsmatrix
(jede Klasse gegen jede bei gleichem Bauwert). Ohne `--ron` die Matrix selbst.

```bash
cargo headless fleets --seed 7 --years 20 --order 10:alle:speer:1,dolch:2
```

Flotten, Schiffe, Vorräte, Werften, Bauaufträge und der Staatshaushalt je Gemeinschaft
nach einer Partie mit demselben Drehbuch; `--ships` listet jedes Schiff einzeln,
`--months` die letzten zwölf Haushaltsmonate, `--polity N` zeigt nur eine Gemeinschaft,
`--save datei.ron` schreibt den Spielstand weg.

```bash
cargo headless fleets --seeds 0..8 --years 10 --start-fleets 2
```

Die Haushaltsübersicht über mehrere Partien: Einnahmen, jeder Anspruch als Anteil der
Einnahmen, Ausschüttung, Monate mit Defizit und unter der Rücklage, Soldrückstand, dazu
Geld und Bedarfsdeckung der Haushalte. Zum Vergleichen: `--start-fleets N` (jede
Gemeinschaft beginnt mit N Startflotten), `--research-share X`, `--tax X` (Lohnsteuersatz
aller Gemeinschaften, per Befehl am ersten Tag) und `--fleet-cost F` (Faktor auf die
laufenden Kosten der Flotte; 0 schaltet sie ab). Mit `--start-fleets`,
`--research-share` oder `--fleet-cost` ruht die militärische Führung der KI, damit die
Flotten bleiben, wie sie sind; `--ai alle:aktiv` schaltet sie wieder ein.

```bash
cargo headless research --seed 7 --years 100
```

Forschung je Gemeinschaft und Jahrzehnt: Einnahmen, Etat, Punkte, Stufen je Zweig. Mit
`--seeds 0..8` die Übersicht über viele Partien.

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
  Steuersatz und Rücklage. Was der Staat mit seinen Einnahmen tut, regelt sein Haushalt.
- **Geldmenge.** Fällt das Preisniveau unter das Ziel, schöpft die Notenbank Geld für den
  Staat. Die Schöpfung ist ausgewiesen und Teil der Geldsummenprüfung.

Alle Gewichte und Schwellen stehen in `assets/data/balance.ron` unter `finance`.

## Staatshaushalt

- **Ein Haushalt je Gemeinschaft und Monat.** Am Monatsersten steht fest, was der
  abgeschlossene Monat eingebracht hat: Lohnsteuer, Zölle der Kontore, Dividenden auf
  staatliche Anteile. Aus der Kasse bedient der Staat dann seine Ansprüche in fester
  Rangfolge (`economy::Claim`):
  1. **Schuldendienst:** Zinsen und fällige Anleihen. Den Zins neuer Anleihen misst der
     Markt an der Schuld im Verhältnis zu den laufenden Einnahmen des Staates, geglättet
     über die letzten Monate.
  2. **Grundleistungen:** laufende Verwaltung und Versorgung, ein fester Anteil
     (`civil_share`) der Einnahmen, gemittelt über zwölf Monate und gerechnet zum
     üblichen Lohnsteuersatz: Von den Einnahmen geht ab, was an Lohnsteuer wirklich
     einging, und hinzu kommt, was der übliche Satz auf dieselbe Lohnsumme gebracht
     hätte. Sie gehen an die Haushalte.
  3. **Flotte:** Sold der Besatzungen und Ersatzteile der Wartung.
  4. **Beschaffung:** Werften, Nachschub und Instandsetzung. Bezahlt wird, wenn gekauft
     wird, den ganzen Monat über; am Monatsersten wird der Voranschlag zurückgelegt.
     Ausgeben darf sie, was in der Kasse liegt und niemandem sonst bewilligt ist, auch
     die Rücklage und was im Monat hinzukommt (Anleihen, Verkäufe), nur nicht die
     laufenden Einnahmen des Monats: Sie gehören dem nächsten Abschluss.
  5. **Kontore:** Einlagen in Kontore, deren Kasse leer ist, gegen neue Anteile.
  6. **Forschung:** der eingestellte Anteil der Einnahmen.
  7. **Rücklage:** Was übrig ist, füllt sie bis zu ihrem Ziel auf.
  8. **Ausschüttung:** Erst vom Rest darüber geht ein Anteil (`handout_share`) an die
     Haushalte.
- **Kürzung.** Reicht das Geld nicht, wird von hinten gekürzt, mit den Folgen, die jeder
  Anspruch selbst hat: Ausgebliebener Sold wird zum Soldrückstand und drückt die Moral
  der Besatzungen, unbezahlte Forschung kommt langsamer voran, Werften, Nachschub und
  Instandsetzung warten. Die Ansprüche 1 bis 5 greifen notfalls die Rücklage an, die
  Forschung nie: Sie lebt von dem, was der Monat nach ihnen übrig lässt, und von dem, was
  schon über der Rücklage lag. Die Wartung der Flotte ist zu den Preisen des
  Monatsersten bewilligt; werden die Ersatzteile im Monat teurer, kauft sie aus der
  freien Kasse weiter wie die Beschaffung.
- **Rüstung kostet Wohlfahrt.** Sold und Gehälter gehen an die, die dem Staat dienen
  (Besatzungen, Forschende), nicht an die Haushalte der Welten. Der Staatsdienst kauft am
  Markt des Sitzes seiner Gemeinschaft ein, was Haushalte kaufen; Ersatzteile und
  Treibstoff verbraucht die Flotte selbst. Was Flotte und Forschung kosten, fehlt der
  Ausschüttung: Eine Gemeinschaft, die für beides nichts ausgibt, schüttet gut doppelt so
  viel aus, und ihre Haushalte decken mehr von ihrem Bedarf.
- **Größenordnung** (gemessen über die Seeds 0 bis 7, erste zehn Jahre): Die Startflotte
  kostet rund 9 % der laufenden Staatseinnahmen, die Forschung zum üblichen Etat rund
  13 %; jede weitere Flotte kostet noch einmal so viel. Mit der doppelten Flotte halbiert
  sich die Ausschüttung. Das Vierfache trägt der Haushalt ohne höhere Steuer nicht: Die
  Ansprüche verlangen mehr, als die Monate einbringen, die Kasse fällt unter die
  Rücklage, die Ausschüttung endet, und wer sich das Geld nicht leiht, bleibt Sold
  schuldig. Eine höhere Lohnsteuer hilft, weil die Grundleistungen am üblichen Satz
  bemessen sind; sie kostet die Haushalte Kaufkraft.
- **Lesemodell.** `Economy::budget(gemeinschaft)` liefert reine Daten: den laufenden
  Monat, die letzten zwölf abgeschlossenen (`history`) und ihre Summe (`year()`). Je
  Monat die Einnahmen, je Anspruch verlangt und bezahlt (`line`), den Rest nach den
  Ansprüchen (`remainder`), die Ausschüttung (`handout`), was aus Geldmangel gekürzt
  wurde (`unfunded`, auch was der Wartung im Monat an Geld fehlte), Kasse und
  Rücklagenziel.

Die Stellschrauben stehen in `assets/data/balance.ron`: `civil_share`, `handout_share` und
`state_reserve_per_million` unter `economy`, Sold und Wartung unter `military`, der
Forschungsetat unter `research`.

## Flotten, Werften und Nachschub

- **Schiffe.** Eine Flotte besteht aus Schiffen mit Entwurf, Rumpf- und Modulzustand,
  Munition, Treibstoff und Bordjägern. Ihre Stärke ist abgeleitet: Bauwert mal Zustand,
  geeicht an der Startflotte. Kaperfahrt, Geleitschutz und Blockade rechnen mit dieser Zahl.
- **Werften.** Eine Welt baut Schiffe, wenn in ihrem Zweig Rüstung genug Kapital steht:
  Die Werft entsteht bei `yard_open_capital` und besteht, bis das Kapital unter
  `yard_close_capital` fällt; Reste, die nur noch verfallen, machen keine. Je
  `yard_capital_per_slot` gibt es einen Bauplatz, mindestens einen. Die Hauptwelt hat auch
  ohne Rüstungskapital einen. Der Staat kauft täglich den Tagesanteil der Bauteile am Markt
  des Systems und zahlt der Rüstungsfirma der Welt einen Aufschlag, an dem ihre
  Anteilseigner mitverdienen. Fehlt ein Bauteil oder Geld, stockt der Bau: Eine blockierte
  Werft steht still, weil keine Ware mehr ankommt. Ein neues Schiff kommt mit leeren
  Magazinen und Tanks.
- **Stand eines Auftrags.** Jeder Bauauftrag sagt, wo er steht und woran es liegt
  (`BuildOrder::status`, in Worten `status_text`): im Bau, wartet auf einen Bauplatz,
  stockt (mit dem Bauteil, das fehlt, oder weil das Geld fehlt), fertig im Dock, oder er
  ruht, weil die Welt keine Werft mehr hat. Ein ruhender Auftrag verliert nichts: Kehrt
  die Werft binnen `yard_grace_days` zurück, geht der Bau weiter. Danach verfällt er wie
  ein gestrichener Auftrag, ohne Erstattung; die Chronik nennt den abgeschriebenen Betrag.
- **Eigenes Gebiet.** Einkaufen kann eine Gemeinschaft für Flotten und Werften in jedem
  System, in dem sie eine Welt besitzt, auch wenn dort eine andere die Mehrheit stellt.
  Liegt sie mit dieser im Krieg, verkauft deren Kontor ihr nichts.
- **Nachschub.** Im eigenen, nicht blockierten Gebiet fasst eine Flotte Munition und
  Treibstoff vom Markt nach, bezahlt aus der Staatskasse. Marschieren kostet Treibstoff,
  und auch wer in einem System liegt, verbraucht täglich einen Anteil seines Tanks
  (`fuel_per_day`). Zu Hause kauft der Staat ihn laufend nach; fern der Heimat läuft eine
  Flotte in Monaten trocken. Mit leerem Tank schleppt sie sich nur noch mit einem
  Bruchteil ihrer Fahrt, und sie sperrt kein System, kapert nicht und gibt kein Geleit:
  Eine Blockade weit von zu Hause hält nur, wer seine Flotten ablöst.
- **Instandsetzung.** Im eigenen Gebiet mit Werft, gegen Rumpfsegmente vom Markt; verlorene
  Bordjäger kommen aus ihren eigenen Bauteilen.
- **Sold und Wartung.** Monatlich der Sold, ein Anteil am Bauwert aller Schiffe
  (`upkeep_per_month`); er geht an die Besatzungen, die ihn am Markt des Sitzes ihrer
  Gemeinschaft ausgeben. Dazu täglich die Ersatzteile der Wartung: je Monat ein Anteil
  der Bauteile jedes Schiffs (`maintenance_per_month`), gekauft am selben Markt, wo immer
  die Flotte liegt. Eine Flotte steht so in Preisen und Beständen der Rüstungsgüter, auch
  im Frieden. Bewilligt der Staatshaushalt der Flotte weniger, als sie verlangt, wächst
  der Soldrückstand der Gemeinschaft.
- **Befehle.** Schiffe bestellen (einen bestimmten Entwurf oder eine Klasse, dann wählt die
  Werft den besten Entwurf, den die Gemeinschaft beherrscht) und Aufträge streichen,
  Flotten teilen und vereinigen, Aufstellung und Doktrin als stehende Befehle, Rückzug aus
  einem laufenden Gefecht, Schiffe stilllegen.
- **Stilllegen.** Eine Flotte, die im System einer eigenen Werft liegt, kann Schiffe für
  immer stilllegen (`LayUpShips`). Sie verlassen das Spiel, Sold und Wartung enden. Einen
  Anteil ihrer Bauteile (`layup_salvage`) verkauft der Staat am Markt des Systems, soweit
  die Kasse des Kontors reicht; der Rest ist Schrott. Wer seine Flotte nicht mehr
  bezahlen kann, hat damit neben Steuern und Anleihen einen dritten Weg.
- **Rangfolge der Ausgaben.** Flotte (Sold und Wartung) und Beschaffung (Werften,
  Nachschub, Instandsetzung) sind zwei Ansprüche des Staatshaushalts; sie gehen
  Forschung, Rücklage und Ausschüttung vor und greifen notfalls die Rücklage an.

Die Stellschrauben stehen in `assets/data/balance.ron` unter `military`.

## Militärische Führung der KI

Ob Krieg oder Frieden herrscht, entscheidet die KI noch nicht (das kommt mit der
Diplomatie); Kriege erklären der Spieler oder ein Drehbuch. In beidem verhält sie sich
nach einer Grundhaltung. Befehle erteilt sie nur über `crates/sim/src/ai/levers.rs`, nie
für die Gemeinschaft des Spielers und nur, wenn sich etwas ändern soll.

- **Was sie weiß.** Von fremden Flotten, was die Karte zeigt: Ort, Marschrichtung, ob sie
  kämpfen oder sich sammeln, und ihre Schiffe nach Klasse und Bauwert. Zustand und
  Vorräte kennt sie nur von den eigenen; die Technik des Gegners geht mit der Schätzung
  ein, die das Spiel anbietet. Alles davon steht in `ai/estimate.rs`.
- **Kräfte schätzen.** Die Kontertabelle `military.counter` sagt, wie gut eine Klasse
  gegen eine andere bei gleichem Bauwert abschneidet; sie stammt aus der Gefechtsmatrix
  des Rechners. Die Kampfkraft einer Flotte gegen eine bestimmte andere ist ihr Wert je
  Klasse, gewichtet mit der Tabelle über die Mischung des Gegners; das Verhältnis beider
  ist die Schätzung. Im laufenden Gefecht zählt auf beiden Seiten dasselbe: was noch
  kämpft, mit seinem Bauwert.
- **Rüstung, monatlich.** Sold und Wartung sollen im Frieden `peace_share` der
  Staatseinnahmen kosten, mehr, wenn eine fremde Flotte nahe den eigenen Systemen die
  eigene übertrifft, und im Krieg das `war_factor`-Fache, nie über `max_share`. Die
  Einnahmen kommen in Schüben (Zölle fließen erst, wenn ein Kontor Überschuss hat), und
  ein Staat, dem das Geld ausgeht, nimmt auch weniger ein: Gemessen wird deshalb an der
  schwächeren Hälfte der letzten zwölf Monate (`lean_share`) und an einem trägen Mittel
  über Jahre (`revenue_memory_months`), am kleineren von beiden. Bestellt wird nur aus
  gefüllter Rücklage und nur, was in den Rest passt, den der Haushalt der letzten zwölf
  Monate nach allen Ansprüchen ließ (`room_use`); kein Schiff ist mehr wert als das
  ganze Ziel. Gewählt wird die Klasse mit der besten Summe gegen die Mischung der
  bedrohlichsten Gemeinschaft, mit einem Abschlag für das, was die eigene Flotte schon
  reichlich hat (`mix_weight`); gebaut wird der beste Entwurf, den die Gemeinschaft
  beherrscht. Wer rüstet, hält `reserve_months` Monate Flottenkosten zusätzlich als
  Rücklage.
- **Wenn der Sold ausbleibt.** Zuerst kürzt die KI ihren Forschungsetat und streicht
  wartende Aufträge. Bleibt im Frieden in `layup_after_months` der letzten zwölf Monate
  Sold aus, legt sie Schiffe still, aber nur, was die Einnahmen auf lange Sicht nicht
  tragen: Einen Einbruch von einigen Monaten sitzt die Flotte mit Soldrückstand aus.
  Danach baut sie im Frieden nicht gleich wieder: `quiet_months` lang nach
  ausgebliebenem Sold, `layup_quiet_months` lang nach einer Stilllegung.
- **Forschung.** Etat der Vorlage, im Krieg höher; gekürzt, solange der Haushalt ihn zwei
  Monate in Folge nicht trägt. Im Krieg wählt sie das Ziel selbst und zieht vor, was
  einen neuen Schiffsentwurf freischaltet.
- **Flotten im Frieden.** Versorgt und instand gesetzt, vereinigt, im Geleitschutz am
  Warteplatz: dem eigenen Engpasssystem, das der bedrohlichsten Gemeinschaft am nächsten
  liegt, sonst am Sitz. Neue Schiffe fassen erst Treibstoff und stoßen dann zur Flotte.
- **Flotten im Krieg, Verteidigung.** Einem Eindringling im eigenen Gebiet stellt sich
  die Flotte, wenn das Kräfteverhältnis mindestens `defend_odds` ist; sie läuft dazu
  erst aus, wenn der Tank für den Weg reicht, und bleibt danach dort stehen, bis sie den
  Platz nicht mehr halten kann. Steht ein klar überlegener Gegner bei ihr oder nebenan,
  weicht sie in einen Hafen aus, an dem und neben dem keiner steht.
- **Flotten im Krieg, Angriff.** Ab `offensive_odds` greift die Hauptflotte an, und zwar
  gemessen an allem, was im Umkreis von `response_jumps` Sprüngen um das Ziel steht und
  dort eingreifen kann. Sie sucht die Hauptflotte des Gegners, wenn sie stärker ist,
  sonst sperrt sie sein volkreichstes System, das vom nächsten eigenen Hafen aus in
  Reichweite liegt (`offensive_reach`). Ein Angriff beginnt im Hafen mit vollen Tanks
  und hält durch, solange das Verhältnis `defend_odds` nicht unterschreitet. Sie bricht
  ab unter `break_off_supply` Vorrat, wenn der Rückweg nicht mehr im Tank ist, oder nach
  `break_off_loss` Verlust, läuft einen Hafen an (eine Werft, wenn der Tank reicht) und
  kommt wieder, wenn Vorrat und Zustand `ready_supply` erreichen. Im Gefecht befiehlt
  sie den Rückzug, sobald das Kräfteverhältnis unter `retreat_odds` fällt.
- **Kein Hin und Her.** Wer einen Angriff vor einem stärkeren Gegner aufgibt, sich aus
  einem Gefecht zurückzieht oder ausweichen muss, bleibt `caution_days` lang in seinen
  Häfen. Eine fremde Flotte, mit der die eigene im Gefecht stand, sucht sie
  `rematch_days` lang nicht wieder auf; ging in dem Gefecht nichts verloren, ist die
  andere nur ausgewichen, und dann gilt `elusive_days`, auch wenn sie im eigenen Gebiet
  steht. Marschiert wird außerhalb eines Angriffs nur auf Wegen, an denen kein Gegner
  steht; eine Flotte mit leerem Tank, die eben weichen musste, bleibt liegen.
- **Für spätere Planer.** Flotten in `AiState::reserved` lässt die Grundhaltung in Ruhe;
  heute trägt niemand etwas ein. Über `Headless::set_military_mode` (und `--ai`) lässt
  sich die Führung je Gemeinschaft abschalten.

Die Stellschrauben stehen in `assets/data/balance.ron` unter `military.ai`.

## Technologie

- **Forschung.** Jede Gemeinschaft gibt monatlich einen Anteil ihrer laufenden
  Staatseinnahmen (Lohnsteuer, Zölle, Dividenden) für Forschung aus, soweit ihr Haushalt
  es nach den vorrangigen Ansprüchen hergibt. Das Geld ist das Gehalt der Forschenden:
  Es geht an den Staatsdienst, nicht an die Haushalte der Welten. Aus ihm werden
  Forschungspunkte, mehr davon bei großer Bevölkerung. Fünf Zweige mit je sechs Stufen
  (`techs.ron`).
- **Wirkungen.** Wirtschaft: Ausstoß der Werke, Fahrt der Frachter. Antrieb: Fahrt der
  Flotten auf der Karte und der Schiffe im Gefecht. Waffen und Verteidigung: Schaden,
  Reichweite, Schilde, Panzerung, Rumpf. Eine Flotte kämpft mit der Technik, die ihre
  Gemeinschaft beim Eintritt ins Gefecht beherrscht.
- **Entwürfe.** Manche Schiffsentwürfe setzen Technologien voraus. Die Werft nimmt nur
  Aufträge für Entwürfe an, die die Gemeinschaft beherrscht.
- **Einblick.** Den genauen Stand kennt nur die Gemeinschaft selbst. Andere halten je
  Zweig eine Spanne für möglich, die mit ihrem Einblick enger wird: durch Besitz an den
  Betrieben der anderen und durch Gefechte. Wer gegeneinander gekämpft hat, kennt die
  Technik des Gegners danach besser; ohne neue Gefechte verblasst das wieder.
- **Wissenstransfer.** Was Gemeinschaften in der Nähe erkennbar beherrschen, ist billiger
  zu erforschen; im Krieg ruht der Austausch.

Die Stellschrauben stehen in `assets/data/balance.ron` unter `research`.

## Gefechte

- **Beginn.** Liegen am Tagesanfang einsatzbereite Flotten verfeindeter Gemeinschaften im
  selben System, beginnt ein Gefecht: jede Flotte eine Partei, mit ihrer Aufstellung und
  Doktrin, der Technik ihrer Gemeinschaft und jedem Schiff in dem Zustand, in dem es
  gerade ist. Die Moral der Besatzungen sinkt mit der Kriegsmüdigkeit daheim und mit
  ausstehendem Sold. Wer später dazukommt und mit einer Partei im Krieg liegt, greift am
  nächsten Morgen als Verstärkung ein.
- **Verlauf.** Gerechnet wird im taktischen Rechner (`sim::battle`), ein Schritt je Takt,
  zwei Sekunden Gefechtszeit je Kalendertag. Ein Gefecht zweier Startflotten dauert rund
  sieben Wochen. Solange es läuft, nimmt eine Flotte keinen Marschbefehl an; neue
  Aufstellung und Doktrin gelten sofort, und der Rückzug lässt sich befehlen. Jeden Morgen
  steht in den Flotten, was das Gefecht bisher gekostet hat: Zerstörte Schiffe sind fort
  (auch aus Sold und Stärke), die übrigen tragen ihre Schäden.
- **Ausscheiden.** Eine Flotte verlässt ein Gefecht am Morgen, nachdem sie keinen Gegner
  mehr auf dem Feld hat (etwa nach einem Friedensschluss) oder kein eigenes Schiff mehr,
  auch wenn andere weiterkämpfen. Ohne Gegner bleibt sie einsatzbereit im System. Führt
  niemand unter den Beteiligten mehr Krieg, endet das Gefecht als Ganzes.
- **Ende.** Was aus jedem Schiff geworden ist, sagt der Rechner: verloren (zerstört, oder
  treibend dem Gegner überlassen), entkommen oder noch auf dem Feld. Die Überlebenden
  behalten ihre Schäden und was von Munition und Treibstoff übrig ist; überlebende
  Bordjäger kehren auf Träger ihrer Flotte zurück, soweit die Hangars reichen. Wer das
  Feld hält, entscheidet sich je Seite, nicht je Flotte; zur Seite einer Flotte gehört,
  wer in dem Gefecht einen Gegner mit ihr teilt (nicht schon, wer nach einem
  Friedensschluss nicht mehr ihr Gegner ist). Wer noch Schiffe auf dem Feld hat, bleibt. Wer das
  Feld einem Gegner überlassen hat, wurde geschlagen und weicht aus: in ein Nachbarsystem
  (woher er kam, sonst eigenes Gebiet, sonst wo kein Feind steht). Wer das Feld geräumt
  hat, während die eigene Seite es hält oder noch darum kämpft, bleibt im System und
  sammelt sich hinter den eigenen Linien; verliert die Seite das Feld doch noch, zieht er
  mit ab. Gegner aus demselben Gefecht weichen nie in dasselbe System aus, notfalls
  führt der Weg über mehrere Sprünge.
- **Sammelfrist.** Wer ausweicht oder hinter den Linien bleibt, sammelt sich
  `regroup_days` lang und ist so lange aus dem Spiel: Die Flotte beginnt kein Gefecht,
  wird nicht angegriffen, zählt weder für Blockade noch für Kaperfahrt, Geleitschutz oder
  die Verteidigung eines Orbits und nimmt keinen Marschbefehl an; ihren Abzug führt sie zu
  Ende. Teilen entbindet keinen Teil davon, und eine Flotte, die sich sammelt, lässt sich
  nicht mit einer einsatzbereiten vereinigen. Dieselben Gegner stehen einander deshalb
  frühestens nach Ablauf der Frist wieder gegenüber, und nur, wenn dann eine Seite auf
  die andere marschiert oder neben ihr gewartet hat.
- **Waffenruhe.** Nach einem Friedensschluss erklären die beiden einander `truce_days`
  lang (mindestens die Sammelfrist) keinen neuen Krieg. Frieden und sofort wieder Krieg
  beginnt also kein laufendes Gefecht von vorn; wer das Feld schon geräumt hatte, als
  der Frieden kam, zieht ab und sammelt sich wie jeder andere.
- **Ohne Entscheidung.** Kommt keine Seite gegen die andere an, lösen sich alle im Patt
  voneinander; ebenso endet ein Gefecht an der Zeitgrenze des Rechners. Dann hat niemand
  gewonnen und niemand eine Niederlage erlitten. Wer im eigenen System steht, bleibt
  dort; alle anderen weichen aus und sammeln sich. Dasselbe Gefecht beginnt also nicht
  am nächsten Morgen von vorn.
- **Blockade und Gefecht.** Ein Gefecht ändert an einer Sperre erst etwas, wenn es
  entschieden ist. Eine bestehende Sperre hält, solange die Blockadeflotte auf dem Feld
  kämpft: Ein Entsatz muss sie schlagen, anzukommen und wieder abzudrehen öffnet das
  System nicht. Ein verteidigter Orbit bleibt offen, solange die Verteidiger kämpfen.
- **Folgen.** Verluste heben die Kriegsmüdigkeit im Verhältnis zum verlorenen Anteil an
  der ganzen Flotte, und mit ihr steigen Anleihezinsen und Unruhe in den Regionen.
  Gemessen wird an der Flotte, wie sie in das Gefecht ging, mindestens aber an einer
  Startflotte: Das letzte Schiff einer aufgeriebenen Flotte wiegt, was es wert ist. Eine
  schwere Niederlage (das Feld einem Gegner überlassen und dabei einen großen Anteil
  verloren) drückt die Kurse aller Betriebe der Gemeinschaft. Was verschossen und
  beschädigt wurde, kauft der Staat zu Hause am Markt nach. Im System bleibt ein
  Trümmerfeld, das über die Jahre verblasst. Die Gegner kennen die Technik des jeweils
  anderen danach besser. Der letzte Stand eines beendeten Gefechts, Schiff für Schiff,
  bleibt noch drei Tage lesbar (`Battles::concluded`), höchstens für acht Gefechte; die
  Bilanz (`Battles::report`) bleibt länger.
- **Werften.** Ein fertiges Schiff bleibt im Dock, solange feindliche Flotten im System
  stehen und keine eigene, einsatzbereite es aufnehmen kann.

## Stand

| Meilenstein | Inhalt | Stand |
|---|---|---|
| 1 | Workspace, Galaxie, Planeten, Karte, Kamera, Zeitsteuerung | fertig |
| 2 | Wirtschaft: Produktion, lokale Preise, Frachter, Warenflüsse | fertig |
| 3 | Aktienmarkt, Besitzverteilung, Zusammenhalt, Abspaltung | fertig |
| 4 | Raumkampf mit Schiffsklassen und Modulen | Kern fertig: Schiffe, Werften, Nachschub, Gefechte in der Partie, Forschung mit Wirkung auf Flotten und Gefechte, Staatshaushalt mit fester Rangfolge der Ansprüche, militärische Grundhaltung der KI (Rüstung, Flottenführung in Frieden und Krieg); offen: Oberfläche |
| 5 | Belagerung, Orbitalgeschütze, Boden- und Untergrundkampf | offen |
| 6 | KI-Gemeinschaften, Diplomatie, verborgene Dispositionen, Enden | offen |
| 7 | Speichern/Laden im Menü, Balance, Feinschliff, Tutorial | offen |

Bereits in Meilenstein 1 enthalten, weil spätere Stufen darauf bauen: Gemeinschaften mit
Planetenbesitz, Koloniegründung, Befehlsprotokoll mit Wiedergabe, Spielstände.
