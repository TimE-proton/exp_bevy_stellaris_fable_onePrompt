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

Kriege, Bestellungen und Flottenprogramme nach Drehbuch, solange keine KI Krieg führt und
rüstet. Jede Angabe beginnt mit dem Jahr und darf beliebig oft stehen:

- `--war JAHR:A:B`: Gemeinschaft A erklärt B den Krieg und schickt ihre Flotten im
  Blockadeauftrag in deren Heimatsystem. `--peace JAHR:A:B` schließt Frieden.
- `--order JAHR:WER:SCHIFFE[@ORT]`: WER (Nummer ab 0 oder `alle`) bestellt die Schiffe bei
  der Werft der Hauptwelt, mit `@kolonie` bei der Kolonie mit den meisten Bauplätzen.
- `--navy JAHR:WER:FAKTOR`: WER hält seine Marine von da an auf dem FAKTOR-fachen der
  Startflotte und bestellt jeden Monat nach, was fehlt. `1` ersetzt Verluste, `2`
  verdoppelt die Flotte, `0` beendet das Programm.

Die Ausgabe zeigt die Chronik, abgelehnte Befehle, die Bilanz jedes Gefechts samt Ausgang
und je Gemeinschaft Staatskasse, Kriegsmüdigkeit und Zusammenhalt. `sweep`, `hash` und
`fleets` nehmen dieselben Angaben: So gehen Gefechte, Verluste und Nachbau auch in Sweeps
und Prüfsummen ein.

```bash
cargo headless fleets --seed 7 --years 20 --order 10:alle:speer:1,dolch:2
```

Flotten, Schiffe, Vorräte, Werften und Bauaufträge je Gemeinschaft nach einer Partie mit
demselben Drehbuch; `--ships` listet jedes Schiff einzeln, `--polity N` zeigt nur eine
Gemeinschaft, `--save datei.ron` schreibt den Spielstand weg.

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

Die Tabelle gilt für die Karte der Partie. Die Schlachtansicht hat eine eigene Steuerung,
siehe unten.

### Gefechtsübung

„Gefechtsübung“ im Hauptmenü stellt zwei Flotten gegeneinander und zeigt ihr Gefecht in der
3D-Schlachtansicht. Die Übung braucht keine Partie und verändert keine; der Spieler führt
den blauen Verband.

| Im Aufstellungsdialog | Bedeutung |
|---|---|
| Vorlagen | „Geplänkel“, „Linienschlacht“, „Träger gegen Schlachtschiffe“ und „Großgefecht“ (rund 200 Schiffe je Seite) füllen beide Flottenlisten |
| Flottenliste je Verband | `entwurf:anzahl`, mit Kommas getrennt, etwa `speer:4,waechter:6`. Die Entwürfe nennt der Dialog, darunter `wespe` (Jäger), `dolch` (Korvette), `waechter` (Fregatte), `speer` (Kreuzer), `bastion` (Schlachtschiff) und `horst` (Träger, bringt Bordjäger mit). Je Verband höchstens 1000 Schiffe, Bordjäger mitgezählt, und von jedem genannten Entwurf mindestens eines. Unter dem Feld steht, was die Liste ergibt oder warum sie nicht gilt |
| Formation | Linie, Keil, Schirm oder Zange |
| Doktrin | offensiv, ausgewogen oder defensiv |
| Seed | Gleiche Aufstellung und gleicher Seed ergeben denselben Verlauf |

Die Übung beginnt angehalten bei 00:00: erst umsehen, dann mit der Leertaste oder „Weiter“
die Zeit laufen lassen. Am Ende zeigt die Bilanz Sieger und Verluste; „Neu starten“ stellt
dasselbe Gefecht noch einmal an den Anfang, „Feld ansehen“ blendet die Bilanz aus.

Ohne Hauptmenü, von der Kommandozeile:

```bash
cargo run -- --practice --fleet-a bastion:2,speer:6 --fleet-b horst:2,waechter:8 --seed 9
```

| Option | Wirkung |
|---|---|
| `--practice` | Sofort eine Gefechtsübung starten |
| `--fleet-a LISTE`, `--fleet-b LISTE` | Flotte des blauen und des roten Verbands; ohne Angabe die Vorlage „Linienschlacht“ |
| `--seed N` | Wählt den Verlauf der Übung (ohne Angabe 1) |
| `--lineup` | Schaustellung statt Gefecht: je ein Schiff jeder Klasse nebeneinander, die Zeit steht |
| `--selftest [N]` | Die Übung läuft von allein, höchstens N Frames (ohne Angabe 240), dann beendet sich das Programm. Es endet mit einem Fehler, wenn die Schlachtkamera dabei nicht zeichnete, nichts vom Gefecht in ihrem Sichtfeld lag oder ein verlangtes Foto nicht entstand |
| `--screenshot DATEI` | Im Selbsttest Fotos speichern. Aus `bild.png` werden `bild_1.png` (Übersicht), `bild_2.png` (Nahaufnahme), `bild_3.png` (Trümmerfeld) und, wenn das Gefecht zu Ende kam, `bild_4.png` (Bilanz); die Schaustellung liefert vier Fotos der Klassen |

### Schlachtansicht

| Eingabe | Wirkung |
|---|---|
| W A S D | Kamera vor, zurück, seitwärts |
| E / Q | Kamera steigen / sinken |
| Rechte Maustaste halten | Umsehen |
| M | Umsehen ein- und ausschalten, ohne die Maustaste zu halten |
| Umschalt links | schneller fliegen |
| Mausrad | Fluggeschwindigkeit ändern |
| Ziffernblock 7 / 1 / 3 | Blick von oben / von vorn / von der Seite (mit Strg: Gegenseite) |
| U | Übersicht: alle Schiffe auf dem Feld im Blick |
| G | Zum Schwerpunkt des Gefechts |
| Linksklick | Schiff wählen (ins Leere: Auswahl aufheben) |
| F | Gewähltes Schiff heranholen (ohne Auswahl: Gefechtsschwerpunkt) |
| V | Gewähltes Schiff verfolgen |
| T | Taktische Einblendung: Flottenanker, Reichweiten, Ziellinie |
| N | Namen an nahen Schiffen |
| Leertaste, 1 – 6, + / − | Pause und Geschwindigkeit wie in der Partie |
| Esc | Gefechtsübung verlassen (in der Partie: Menü) |
| F1 | Hilfe |

Die Kopfleiste zeigt Gefechtszeit und Geschwindigkeit; rechts stehen „Übersicht“, „Gefecht“,
„Taktik“, „Namen“, „Hilfe“ und „Verlassen“, nach dem Ende des Gefechts auch „Bilanz“. Die
Tafeln beider Verbände zählen je Klasse, was kämpft, was sich zurückzieht und noch auf dem
Feld ist, was ohne Energie treibt, was verloren und was entkommen ist. Jede Zeile ergibt
zusammen die Anfangsstärke der Klasse; Jäger an Bord zählen bei ihrem Träger mit. Die
Bilanz am Ende zählt genauso.

Die Tafel des eigenen Verbands (links) ändert während des Gefechts Formation und Doktrin
und befiehlt den Rückzug; ein befohlener Rückzug lässt sich nicht zurücknehmen. Ein Klick
auf ein Schiff öffnet seine Tafel mit Rumpf, Schild, Panzerung je Seite, Munition,
Treibstoff, Moral, Ziel und allen Modulen; „Heranholen“ und „Verfolgen“ führen die Kamera
zu ihm. Klicks, Züge und Mausrad auf Leisten und Tafeln wirken nicht auf die Szene
dahinter. Beim Umsehen ist der Mauszeiger gefangen und unsichtbar. Solange gehört die Maus
allein der Kamera: Das Mausrad ändert die Fluggeschwindigkeit, und ein Klick gilt weder
einer Tafel noch einem Schiff.

Die Übersicht rückt so weit ab, dass jedes Schiff auf dem Feld neben den offenen Tafeln zu
sehen ist. Die Blicke des Ziffernblocks gelten dem gewählten Schiff, ohne Auswahl allen
Schiffen auf dem Feld.

Kleine Markierungen stehen für Schiffe, die aus der Ferne kaum zu sehen sind: Dreieck = Jäger
und Korvette, Raute = Fregatte, Pfeil = Kreuzer, Sechseck = Schlachtschiff, Rechteck = Träger.
Hohl = auf dem Rückzug, grau = treibt, Kreuz = Wrack.

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
- **Sold.** Monatlich ein Anteil am Bauwert aller Schiffe, aus der Staatskasse an die
  Haushalte. Reicht die Kasse nicht, wächst der Soldrückstand der Gemeinschaft.
- **Befehle.** Schiffe bestellen (einen bestimmten Entwurf oder eine Klasse, dann wählt die
  Werft den besten Entwurf, den die Gemeinschaft beherrscht) und Aufträge streichen,
  Flotten teilen und vereinigen, Aufstellung und Doktrin als stehende Befehle, Rückzug aus
  einem laufenden Gefecht.
- **Rangfolge der Ausgaben.** Die Staatskasse bedient ihre Ansprüche in fester
  Reihenfolge. Am Monatsersten kehrt der Staat zuerst die Hälfte dessen, was über seiner
  Rücklage liegt, an die Haushalte aus und bedient seine Schulden. Dann kommt die Flotte:
  der Sold, danach täglich Nachschub, Instandsetzung und Werften, Flotten und Aufträge in
  Nummernfolge; sie greifen notfalls die Rücklage an. Zuletzt die Forschung: Sie bekommt
  nur, was dann noch über der Rücklage liegt.

Die Stellschrauben stehen in `assets/data/balance.ron` unter `military`.

## Technologie

- **Forschung.** Jede Gemeinschaft gibt monatlich einen Anteil ihrer laufenden
  Staatseinnahmen (Lohnsteuer, Zölle, Dividenden) für Forschung aus. Das Geld ist das
  Gehalt der Forschenden und fließt an die Haushalte; aus ihm werden Forschungspunkte,
  mehr davon bei großer Bevölkerung. Fünf Zweige mit je sechs Stufen (`techs.ron`).
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
  wer mit ihr nicht im Krieg liegt. Wer noch Schiffe auf dem Feld hat, bleibt. Wer das
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
  anderen danach besser.
- **Werften.** Ein fertiges Schiff bleibt im Dock, solange feindliche Flotten im System
  stehen und keine eigene, einsatzbereite es aufnehmen kann.

## Stand

| Meilenstein | Inhalt | Stand |
|---|---|---|
| 1 | Workspace, Galaxie, Planeten, Karte, Kamera, Zeitsteuerung | fertig |
| 2 | Wirtschaft: Produktion, lokale Preise, Frachter, Warenflüsse | fertig |
| 3 | Aktienmarkt, Besitzverteilung, Zusammenhalt, Abspaltung | fertig |
| 4 | Raumkampf mit Schiffsklassen und Modulen | Kern fertig: Schiffe, Werften, Nachschub, Gefechte in der Partie, Forschung mit Wirkung auf Flotten und Gefechte; offen: Militär-KI, Oberfläche |
| 5 | Belagerung, Orbitalgeschütze, Boden- und Untergrundkampf | offen |
| 6 | KI-Gemeinschaften, Diplomatie, verborgene Dispositionen, Enden | offen |
| 7 | Speichern/Laden im Menü, Balance, Feinschliff, Tutorial | offen |

Bereits in Meilenstein 1 enthalten, weil spätere Stufen darauf bauen: Gemeinschaften mit
Planetenbesitz, Koloniegründung, Befehlsprotokoll mit Wiedergabe, Spielstände.
