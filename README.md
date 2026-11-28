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
- `--preset eroberung`: Gemeinschaft 0 hebt vom Jahr 20 an Truppen aus, erklärt zu Beginn
  des Jahres 25 ihrem nächsten Nachbarn den Krieg, belagert zuerst dessen nächste Welt
  und dann seinen Sitz und schließt zu Beginn des Jahres 45 Frieden, wenn er dann noch
  besteht. Ihre Flotten führt das Drehbuch mit gewöhnlichen Befehlen; so zeigen Sweeps
  Belagerungen, Eroberungen und das Ende einer Gemeinschaft mit einem Angreifer nach
  festem Plan; den Verteidiger führt die KI.
  Der Sweep nennt dazu je Seed die Belagerungen nach Ausgang, die gefallenen Welten und
  die beendeten Gemeinschaften, dazu über alle Läufe die Feldzüge der KI, die mittlere
  Dauer einer Belagerung, die verlorenen Verbände, die Kolonien ohne Vorräte und, über
  die ganze Laufzeit gezählt, die Flottentage mit leerem Tank fern jedem eigenen Hafen
  mit der längsten solchen Folge einer Flotte (je Seed in den Spalten „Tanktage" und
  „Folge"); Tage einer Gemeinschaft, die nirgends mehr einen Hafen hat, stehen für sich.
  Gezählt werden alle Flotten, auch die des Angreifers, die das Drehbuch führt und die
  dort nach dem Plan über fremden Welten liegen bleiben.
- `--preset feldzug`: Das Drehbuch erklärt nur die Kriege. Die Gemeinschaften 0 bis 3
  erklären zu Beginn der Jahre 20, 25, 30 und 35 ihrem dann nächsten Nachbarn den Krieg
  und schließen je 15 Jahre später Frieden; Flotten, Feldzüge, Belagerungen und
  Verteidigung führt auf allen Seiten die KI (siehe „Wie die KI belagert und
  verteidigt").
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
cargo headless siege --reference
```

Der Tagesrechner der Belagerung ohne Partie: die drei Wege (aushungern, stürmen,
beschießen) für drei Bezugswelten im Vergleich. Ohne `--reference` der Verlauf Tag für
Tag; Bevölkerung, Verteidigung, Flotte und Befehle lassen sich über Optionen stellen
(`cargo headless siege --help`).

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

Das Spiel prüft sich mit `--selftest` selbst (`cargo run -p game -- --selftest 240 --seed 3
--screenshot bild.png`): Es startet eine Partie, lässt sie laufen und endet mit einem
Fehler, wenn die Kartenkamera nicht zeichnete, nichts von der Karte in ihrem Sichtfeld
lag oder ein verlangtes Foto nicht entstand. Ab 60 Frames zeigt es die Fenster
nacheinander, damit keines das andere verdeckt: erst „Belagerung“ (`bild_3.png`), dann
„Staatsfinanzen“ mit der Verteidigung und links die Tafel der eigenen Hauptwelt
(Schlussfoto `bild.png`). Eine frische Partie hat keine Belagerung: Das Fenster ist dann
leer, und die Ausgabe sagt „keine Belagerung in Sicht“. Mit einem geladenen Stand mitten
in einer Belagerung (`--load`, etwa aus `cargo headless run --preset eroberung … --save`)
besucht der Selbsttest ab 200 Frames auch die Orbitansicht (`bild_2.png`). Ob eine Tafel
etwas verdeckt, prüft er nicht; dafür sind die Fotos da. `--practice --selftest` prüft
die Gefechtsübung.

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
| Knöpfe oben rechts | Warenflüsse, Produktionsketten, Börse, Staat (Finanzen, Haushalt, Verteidigung), Forschung, Belagerung (mit der Zahl der eigenen Belagerungen), Flotten (Schiffe, Werften, Kosten) |
| F1 | Hilfe |
| Esc | Menü |

Mit Meilenstein 4 führt der Spieler Flotten und Werften, sieht den Staatshaushalt, lenkt
die Forschung, verfolgt die Gefechte der Partie in 3D und kann Schiffe stilllegen; die
anderen Gemeinschaften rüsten, führen ihre Flotten und legen Schiffe still, ohne dass
jemand sie lenkt. „Krieg“ und „Frieden“ stehen in der Übersicht bei den Gemeinschaften;
nach einem Friedensschluss gilt eine Waffenruhe, so lange ist der Knopf „Krieg“ gesperrt,
und eine Zeile unter der Liste nennt die verbleibenden Tage. Die Liste nennt je
Gemeinschaft die Zahl ihrer Schiffe und darunter jeden laufenden Krieg.

Die Tabelle gilt für die Karte der Partie. Die Schlachtansicht hat eine eigene Steuerung,
siehe unten.

Spielstände aus einer älteren Fassung des Spiels (Speicherformat unter 6) lassen sich nicht
mehr laden. Das Spiel sagt es beim Laden und lässt die laufende Partie, wie sie ist.

Befehle gelten mit dem nächsten Takt der Simulation. Steht die Zeit, sind sie vorgemerkt,
und ein Hinweis unten in der Mitte des Fensters sagt es. Was vorgemerkt ist, zählt für die Knöpfe schon:
Ein Befehl, den es nur einmal geben kann (Rückzug, Vereinigen, Streichen), wird nicht noch
einmal angeboten, und eine Werft zeigt ihre vorgemerkten Bestellungen. Lehnt die
Simulation einen Befehl ab, steht der Grund im Hinweis.

### Forschung

„Forschung“ in der Kopfleiste öffnet das Forschungsfenster mit zwei Reitern.

**Eigene Forschung.** Der Baum zeigt die fünf Zweige Wirtschaft, Antrieb, Waffen,
Verteidigung und Bodenkrieg als Spalten und ihre Stufen als Zeilen. Jede Technologie nennt
ihren Stand, ihre Kosten in Forschungspunkten, ihre Wirkungen und die Schiffsentwürfe, die
sie voraussetzen.

| Stand | Bedeutung |
|---|---|
| erforscht | Die Wirkungen gelten |
| in Arbeit | Das Ziel, in das die Forschungspunkte fließen, mit dem Fortschritt und der Zeit bis zum Abschluss beim Tempo des letzten Monats |
| Knopf „Erforschen“ | Alle Voraussetzungen sind erfüllt; der Knopf macht die Technologie zum Ziel. Was am bisherigen Ziel erarbeitet ist, bleibt erhalten |
| fehlt: … | Gesperrt; genannt sind die Voraussetzungen, die noch fehlen |

Über dem Baum stehen Etat und Ziel. Der Etat ist der Anteil der laufenden Staatseinnahmen,
der monatlich in die Forschung geht: am Regler einstellen und mit „Etat festsetzen“
bestätigen; er gilt ab dem nächsten Monatsersten. Im Haushalt steht die Forschung hinter
Flotte, Beschaffung und Kontoren und greift die Rücklage nicht an: Reicht das Geld nicht,
wird sie zuerst gekürzt. Zu Beginn
wählt eine Automatik das Ziel und prüft es jeden Monat. Ein selbst gesetztes Ziel gilt bis
zu seinem Abschluss; danach ruht die Forschung, bis ein neues gewählt ist oder „Automatik
einschalten“ die Wahl zurückgibt. Unter dem Baum steht die Summe der Wirkungen aller
erforschten Technologien.

**Einschätzung der anderen.** Den genauen Stand ihrer Forschung kennt nur jede Gemeinschaft
selbst. Der Reiter zeigt je Gemeinschaft und Zweig die Spanne der Stufen, die nach dem
eigenen Einblick möglich ist, etwa 0–4. In der Leiste daneben sind volle Kästchen bestätigt
(mindestens diese Stufe), getönte möglich und leere ausgeschlossen; eine einzelne grüne
Zahl ist genau bekannt. Der Einblick wächst durch Besitz an den Betrieben der anderen und
durch Gefechte gegen sie; was Gefechte an Einblick bringen, verblasst ohne neue wieder.

### Staat und Haushalt

„Staat“ in der Kopfleiste öffnet die Staatsfinanzen. Oben stehen Kasse, Einnahmen des
Vormonats, Schulden und Zins, darunter der **Haushalt**: der letzte abgeschlossene Monat
und die Summe der bis zu zwölf letzten Monate. Die Tafel nennt die laufenden Einnahmen,
dann jeden Anspruch in seiner Rangfolge (Schuldendienst, Grundleistungen, Flotte,
Verteidigung, Beschaffung, Kontore, Forschung) mit dem Verlangten, dem Bezahlten und dem, was offen
blieb (rot), den Rest der Einnahmen, und was in Rücklage und Ausschüttung ging. Offen
bleibt ein Anspruch, wenn das Geld fehlte oder der Markt die Ware nicht hergab. Unter der
Tafel steht ihr Zeitraum: Ein Haushaltsmonat reicht von einem Monatsabschluss zum nächsten
und wird aus den Einnahmen des Monats davor bezahlt; die „Einnahmen des Vormonats“ oben
gehören schon zum laufenden Haushaltsmonat. Darunter
stehen der Soldrückstand der Flotte und unter „Rangfolge und Kürzungen“, wer die Rücklage
angreifen darf und was eine Kürzung für Forschung, Werften und Besatzungen heißt.
Unter dem Haushalt steht die **Verteidigung** aller eigenen Welten (siehe „Belagerung und
Verteidigung“). Steuersatz, Rücklage und Anleihen werden im selben Fenster festgesetzt.

### Flotten

„Flotten“ in der Kopfleiste öffnet das Flottenfenster mit zwei Reitern.

**Flotten und Schiffe.** Die Liste nennt je eigener Flotte Ort und Zustand, Schiffe je
Klasse, Stärke, Munition und Treibstoff, den Auftrag (Wache, Blockade, Kaperfahrt,
Geleitschutz) sowie Aufstellung und Doktrin. Ein Klick auf den Namen zeigt darunter die
Flotte mit jedem Schiff: Klasse, Entwurf, Rumpf, Module, Bordjäger, Munition, Treibstoff;
beschädigte Schiffe sind markiert. Dort werden Auftrag, Aufstellung und Doktrin gewählt,
und dort wird umgegliedert: „Vereinigen mit …“ gibt die Schiffe an eine andere eigene
Flotte im selben System; ein Klick auf Schiffsnummern wählt Schiffe, „Abteilen“ stellt
mit ihnen eine neue Flotte auf. „Stilllegen“ nimmt die gewählten Schiffe für immer aus
dem Spiel; das geht nur, wo die Flotte in einem System mit eigener Werft liegt. Neben dem
Knopf steht, was der Verkauf der Bauteile am heutigen Markt bringt und was die Flotte
danach im Monat weniger kostet. „Auf der Karte zeigen“ führt die Karte zur Flotte. Ein
gesperrter Knopf nennt den Grund, mit dem die Simulation den Befehl ablehnen würde,
daneben oder im Schwebetext. Liegen fremde Flotten im gewählten System, nennt das Fenster
auch sie.

| Zustand | Was die Flotte annimmt |
|---|---|
| einsatzbereit, auf dem Marsch | „Marsch zum Ziel“ schickt sie in das System, das auf der Karte gewählt ist |
| im Gefecht | Keinen Marsch. „Rückzug“ lässt die Schiffe das Feld verlassen und ist nicht umkehrbar; neue Aufstellung und Doktrin gelten sofort. „Gefecht ansehen“ öffnet die Schlachtansicht |
| sammelt sich bis … | Nach dem Abzug aus einem Gefecht keinen Marschbefehl, bis die Frist abläuft; einen Abzug führt die Flotte zu Ende |

**Werften und Kosten.** Oben steht, was die Flotte kostet: die Zeilen Flotte (Sold und
Ersatzteile) und Beschaffung (Werften, Nachschub, Instandsetzung) des Haushalts. Darunter
jede eigene Werft mit Bauplätzen und Aufträgen: Fortschritt, Stand und, wenn ein Auftrag
wartet, der Grund (kein Bauplatz, ein Bauteil fehlt am Markt, dem Staat fehlt das Geld).
„Streichen“ nimmt einen Auftrag zurück; Verbautes wird nicht erstattet. Die Liste der
Entwürfe nennt Klasse, Bauwert, Bauzeit, Unterhalt je Monat und Bauteile; „Bestellen“
gibt ein Schiff bei der gewählten Werft in Auftrag, die Knöpfe der Klassen überlassen der
Werft die Wahl des besten beherrschten Entwurfs. Entwürfe, deren Technologie fehlt, sind
gesperrt und nennen sie. Unter der Liste steht, was ein weiteres Schiff des gewählten
Entwurfs die Flotte im Monat mehr kostet.

Auf der Karte sind Flotten Dreiecke in der Farbe ihrer Gemeinschaft. Ein roter Ring heißt:
im Gefecht; ein offener gelber Bogen: sammelt sich. Eine Flotte auf dem Marsch rückt jeden
Tag um die Strecke vor, die Antriebstechnik und Treibstoff hergeben. Mit leerem Tank
schleppt sie sich nur noch voran; dann sperrt sie kein System, kapert nicht und gibt kein
Geleit.

### Belagerung und Verteidigung

„Belagerung“ in der Kopfleiste öffnet das Fenster der Belagerungen; die Zahl in Klammern
nennt die, an denen die eigene Gemeinschaft als Belagerer oder Verteidiger beteiligt ist.
Kriegseinträge der Chronik über eine laufende Belagerung tragen den Knopf „Belagerung
zeigen“. Gezeigt werden die eigenen Belagerungen und fremde in Systemen, in denen eine
eigene Flotte oder Welt liegt (wie bei den Gefechten); Beobachter sehen jede.

**Belagerungen.** Die Liste nennt je Belagerung Welt, Belagerer, Verteidiger, Phase,
Dauer, Fortschritt und Lage. Ein Klick auf die Welt zeigt sie im Einzelnen: die fünf
Phasen als Leiste (Anmarsch, Einschließung, Beschuss, Bodenkrieg, Untergrund), gleich
darunter die Befehle der eigenen Seite, dann Dauer und Fortschritt, die Vorräte der
Garnison und den Nachschub der Landungstruppe in Tagen, die Anlagen der Welt (Stationen,
aktive Geschütze, Schild mit Ladung, Bunker), die Truppen beider Seiten, die Flotten im
Orbit mit Feuerkraft, Munition und Treibstoff und die Zonen. Reiter und Liste stehen
fest; was darunter nicht in das Fenster passt, rollt mit einem sichtbaren Rollbalken. Im
Untergrund (Phase 5) nennt „Fortschritt“ nicht die genommene Oberfläche, sondern wie
viel der Garnison gebrochen ist und was im Untergrund noch sitzt. Unter „Prognose“
stehen die drei Wege (Aushungern, Sturm, Flächenbeschuss) in ganzen Sätzen: ob die Welt
fällt, in wie vielen Tagen, und sonst, woran es liegt (Treibstoff, Munition, Schild,
Stationen, fehlende Truppen, eine Landung, die abgeschlagen würde), dazu die Verluste
beider Seiten, der Rumpf der Schiffe und die Beute. Hält der Belagerer den Orbit nicht,
steht dort „ruht, Orbit umstritten“, und statt der drei Wege steht ein Satz: Die
Prognose gilt erst wieder mit der Orbitalhoheit; was eine Truppe am Boden ohne sie noch
erreicht, rechnet sie nicht voraus.

| Wer | Befehle im Fenster |
|---|---|
| Belagerer | Feuerstufe (kein Feuer, stützen, gezielt, flächig), „Sturm“, Schwerpunkt, „Nach dem Fall plündern“; „Landen“ setzt Verbände der gewählten Flotte an Land (Stärke am Regler; solange eine Welle vorgemerkt ist, gibt es keine zweite); „Belagerung aufheben“ fragt erst nach |
| Verteidiger | Haltung der Garnison (vorwärts, Tiefe), „Geschütze schweigen“; „Welt übergeben“ fragt erst nach und sagt dabei, wenn es die letzte Welt ist und die Gemeinschaft damit endet |

Eine Rückfrage gilt nur, solange sie auf dem Schirm steht: Wird das Fenster geschlossen
oder die Ansicht gewechselt, ist sie verfallen.

**Flotten und Truppen.** Die Liste nennt je eigener Flotte Ort, Truppen an Bord und ihr
Ziel. Liegt die gewählte Flotte über einer fremden Welt, beginnt „Belagern“ die
Belagerung; über einer eigenen Welt gehen mit „Einschiffen“ reguläre Verbände samt
Nachschub für die gewählten Tage an Bord, und „An Land setzen“ gibt sie der Garnison
zurück. Dieselben Knöpfe stehen bei der gewählten Flotte im Fenster „Flotten“.

Der Nachschub wird am Markt des Systems gekauft, soweit Ware und der Etat für
Verteidigung und Beschaffung reichen; der Markt gibt je Kauf nur einen Teil seines
Bestands her. Neben „Einschiffen“ steht deshalb vor dem Klick, ob der verlangte
Nachschub heute zu haben ist, und sonst, für wie viele Tage es reicht und ob es am Markt
oder am Etat liegt (die Simulation rechnet die Einschiffung dazu auf einer Kopie). Kommt
weniger an Bord als verlangt, sagt es danach ein Hinweis, und er bleibt an der Flotte
stehen.

Hat eine Flotte den Auftrag, die Belagerung hat aber nicht begonnen, steht dort „wartet
auf die Orbitalhoheit“ mit dem Grund (die Flotte hat keinen Treibstoff, steht im Gefecht,
sammelt sich, oder eine feindliche Flotte liegt im System) und „Auftrag aufheben“.

Ein gesperrter Knopf nennt den Grund, mit dem die Simulation den Befehl ablehnen würde,
daneben oder im Schwebetext. Gefragt wird sie dazu einmal je Tag und Lage, nicht in
jedem Bild.

**Verteidigung.** Die Tafel „Verteidigung“ steht an jeder eigenen Welt (linke Tafel,
Reiter „Übersicht“) und im Fenster „Staatsfinanzen“ unter der Übersicht aller eigenen
Welten. Sie nennt je Anlage (Orbitalstationen, Bodengeschütze, Schild, Bunker) die Stufe,
die höchste Stufe nach dem Stand der Technik und den Ausbaustand (gelieferte Gütersätze
gegen die der nächsten Stufe), dazu reguläre Verbände und Depot mit ihren Grenzen, den
Unterhalt im Monat heute und unter dem Plan, was der Ausbau noch kostet und welche
Garnison eine besetzte Welt dieser Größe ruhig hält. Die Felder der Spalte „Plan“ ändern
das Ausbauziel; „Plan festsetzen“ schickt es ab, „Verwerfen“ nimmt die Änderung zurück.
Die Spalte „Ausbau“ misst am gezeigten Plan. Der Regler für reguläre Verbände rastet auf
halbe Verbände; ein Plan der Simulation, der nicht auf diesem Raster liegt (die Besatzung
einer eroberten Welt), wird gezeigt, wie er ist, und gilt nicht als geändert.
Darunter stehen Haltung und Feuerverbot der Garnison. Bezahlt wird aus der Zeile
„Verteidigung“ des Haushalts, im Rang gleich nach der Flotte.

**So belagert man.** (1) Krieg erklären (Liste der Gemeinschaften). (2) Über einer
eigenen Welt mit regulären Verbänden Truppen an Bord nehmen: Fenster „Belagerung“, Reiter
„Flotten und Truppen“, Flotte wählen, Stärke und Nachschubtage einstellen, „Einschiffen“.
(3) Die Flotte im Fenster „Flotten“ in das System der fremden Welt schicken. (4) Dort
„Belagern“. (5) Im Reiter „Belagerungen“ die Feuerstufe wählen und die Prognose lesen:
Halten Stationen den Orbit, muss die Flotte sie erst niederkämpfen; ein Schild fängt den
Beschuss ab, solange er Ladung hat. (6) Trägt die Prognose für „Sturm“, mit „Landen“
Truppen absetzen und „Sturm“ ankreuzen; sonst aushungern und auf Treibstoff und Munition
achten. (7) Fällt die Welt, wechselt sie den Besitzer; war es die letzte der
Gemeinschaft, endet diese.

**So verteidigt man.** Vor dem Krieg in der Tafel „Verteidigung“ der Welt den Plan
setzen (Stationen gegen die Einschließung, Geschütze gegen Schiffe im niedrigen Orbit,
Schild gegen Beschuss, Bunker, reguläre Verbände, Depot) und im Haushalt die Zeile
„Verteidigung“ bedienen. Wird die Welt belagert, im Fenster „Belagerung“ die Haltung der
Garnison wählen und, wenn Vorräte knapp sind, die Geschütze schweigen lassen. Entsatz
bringt eine eigene Flotte: Greift sie die Belagerer an, ruht die Belagerung, solange das
Gefecht dauert, und bricht, wenn der Belagerer den Orbit verliert.

**Marken auf der Karte.** Um jedes System mit einer sichtbaren Belagerung liegt ein Ring
aus fünf Abschnitten. So viele leuchten, wie Phasen erreicht sind, in der Farbe der
Phase: hellblau Anmarsch, gelb Einschließung, orange Beschuss, rot Bodenkrieg, violett
Untergrund. Ruht die Belagerung, ist der Ring grau und gestrichelt. Er liegt außerhalb
des doppelten roten Rings eines gesperrten Systems, beides bleibt lesbar; aus der Nähe
trägt auch die belagerte Welt einen Ring. Am System steht ein Knopf mit Nummer und Name
der Phase („3 Beschuss“, bei einer ruhenden „3 Beschuss · ruht“): Der Schwebetext nennt
Belagerer, Verteidiger, Lage und die Legende, ein Klick öffnet das Fenster „Belagerung“
bei dieser Welt.

**Orbitansicht.** „Ansehen“ neben dem Namen der Welt im Fenster „Belagerung“ zeigt die
Belagerung in 3D, mit Kamera und Schiffsmodellen der Schlachtansicht: die Welt,
die Flotten des Belagerers nach Schiffsklassen im Orbit (große Flotten im Maßstab, die
Tafel nennt ihn), Orbitalstationen auf ihrer Bahn, Geschütze und Bunker am Boden und den
Schild als Schale, so hell wie seine Ladung. Die Flotte rückt mit den Phasen näher an
die Welt. Gezeigt wird die Belagerung der Simulation, Tag für Tag mit der Uhr der
Partie; Pause und Geschwindigkeit gelten wie auf der Karte. Geschossen wird in der
Szene nur, wo der Tagesbericht der Simulation Beschuss, Gegenfeuer oder Bodenkampf
meldet: Feuer aus dem Orbit trifft den Schild oder die Oberfläche, Geschütze und
Stationen schießen zurück, nach einer Landung gehen Boote von der Flotte zu Boden. Am
Boden zeigen Kappen, die der Krümmung folgen, die Zonen in der Farbe dessen, der sie
hält; wo gekämpft wird,
pulsiert ein Ring, und im Untergrund flackert es im ganzen Landegebiet. Die Tafel links
nennt Phase, „Lage“ (Dauer, Fortschritt, Vorräte, Truppen, Flotte, Schild), die
„Prognose“ und unter „Meldungen“, was sich geändert hat, seit die Ansicht offen ist
(Phasenwechsel, Verluste, Landungen, genommene Zonen). Hält der Belagerer den Orbit
nicht, sagt ein Hinweis, dass die Belagerung ruht, und „Gefecht ansehen“ führt in das
laufende Gefecht. Wer belagert oder belagert wird, hat seine Befehle in der Ansicht
selbst: Das kleine Fenster „Befehle“ unten rechts trägt Feuerstufe, „Sturm“, „Landen“
und „Belagerung aufheben“ oder die Befehle der Garnison, sodass man eine Landung
befehlen und ihr zusehen kann. „Zur Karte“ verlässt die Ansicht, „Belagerungsfenster“
führt zur Karte mit dem Fenster „Belagerung“ bei dieser Welt, „Übersicht“ (Taste U) holt
Welt und Flotte ins Bild. Fällt die Welt oder wird die Belagerung aufgehoben, hält die
Partie an, und „Ausgang der Belagerung“ nennt in den Worten der Chronik, was geschah;
die Tafel links zeigt danach den „Stand am Ende der Belagerung“, ohne Prognose.

Jede Welt sieht nach ihrer Art aus, deterministisch aus ihrer Kennung erzeugt: Garten-
und Ozeanwelten mit Meeren, Küsten, Vegetation nach Breite und Temperatur und Eis, das
Strömungen weit vor- und zurückschieben und das am Rand in Schollen zerbricht, Steppen
trocken mit Salzpfannen, Eiswelten mit langen Bruchlinien und offenen Rinnen,
Felswelten voller Krater, Glutwelten mit tätigen Gebieten, glühenden Nähten und
Lavaseen unter erstarrter Haut, Giftwelten mit sauren Meeren unter einer dichten,
gelblichen Wolkendecke, Gasriesen mit Bändern und Wirbelstürmen. Welten mit
atembarer oder dichter Luft tragen Wolken, Schleier und einen Saum in der Farbe ihrer
Atmosphäre; dünne Luft hat nur den Saum, luftlose Welten haben nichts davon. Die Sonne
steht seitlich: Die Nachtseite ist dunkel, die Glut leuchtet darin, und Stadtlichter
gehen nur dort an, wo es Nacht ist; wie hell, folgt der Bevölkerung. Die Oberfläche
(Texturen 2048×1024, Wolken und Stadtlichter halb so groß) entsteht abseits des
Hauptfadens; bis sie fertig ist, zeigt die Welt ein Vorabbild, beim zweiten Besuch ist
sie sofort da, auch wenn sich die Bevölkerung inzwischen geändert hat. Kriegsschäden
dunkeln sie ab und löschen Stadtlichter. Aus nächster Nähe bleibt das Gelände weich:
Die Auflösung der Textur reicht dort nicht mehr für feine Einzelheiten.

**Ende einer Gemeinschaft.** Verliert eine Gemeinschaft ihre letzte Welt oder geht sie
in einer anderen auf, meldet es das Fenster „Eine Gemeinschaft endet“ mit dem Eintrag
der Chronik. Die Karte zeigt ihre Welten in der Farbe der neuen Besitzer, die Liste der
Gemeinschaften führt sie durchgestrichen als „beendet“. Trifft es die eigene
Gemeinschaft, hält die Partie an, und „Die eigene Gemeinschaft hat geendet“ sagt, was
geschah. Mit „Als Beobachter weiter“ läuft die Partie als Beobachterpartie weiter:
Karte, Fenster und Chronik bleiben offen, Befehle sind gesperrt, und die Kopfleiste
nennt den Grund.

### Gefechte der Partie ansehen

Beginnt ein Gefecht mit eigenen Schiffen, meldet es ein Hinweis am oberen Rand und die
Chronik. „Gefecht ansehen“ öffnet es in der 3D-Schlachtansicht: an der Marke des Systems
auf der Karte, im Flottenfenster und am Eintrag der Chronik. Fremde Gefechte lassen sich
ansehen, wo eine eigene Flotte liegt oder eine eigene Welt ist.

Gezeigt wird das Gefecht der Simulation, nicht eine Kopie mit eigenem Verlauf. Es läuft
mit der Zeit der Partie: Pause und Geschwindigkeit gelten für beide, und ein Kalendertag
sind zwei Sekunden Gefechtszeit (die Kopfleiste nennt Datum und Verhältnis). Die Tafel
der eigenen Flotte ändert Formation und Doktrin und befiehlt den Rückzug; die Befehle
gehen an die Simulation und gelten mit ihrem nächsten Takt. „Verlassen“ führt jederzeit
zurück zur Karte, das Gefecht läuft weiter. Endet es bei offener Ansicht, hält die Partie
an, und „Ergebnis des Gefechts“ zeigt den Ausgang, die Verluste je Flotte, wer das Feld
hält und wohin die anderen ausweichen; die Lagetafeln zeigen denselben Endstand. „Zur
Karte“ schließt die Ansicht; die Leertaste lässt die Partie weiterlaufen. Einen Neustart gibt es nur in der
Gefechtsübung.

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
  4. **Verteidigung:** die stehende Verteidigung der Welten: Sold der regulären Verbände,
     Unterhalt der Anlagen, Depots, Aushebung und Ausbau (siehe „Belagerung").
  5. **Beschaffung:** Werften, Nachschub und Instandsetzung. Bezahlt wird, wenn gekauft
     wird, den ganzen Monat über; am Monatsersten wird der Voranschlag zurückgelegt.
     Ausgeben darf sie, was in der Kasse liegt und niemandem sonst bewilligt ist, auch
     die Rücklage und was im Monat hinzukommt (Anleihen, Verkäufe), nur nicht die
     laufenden Einnahmen des Monats: Sie gehören dem nächsten Abschluss.
  6. **Kontore:** Einlagen in Kontore, deren Kasse leer ist, gegen neue Anteile.
  7. **Forschung:** der eingestellte Anteil der Einnahmen.
  8. **Rücklage:** Was übrig ist, füllt sie bis zu ihrem Ziel auf.
  9. **Ausschüttung:** Erst vom Rest darüber geht ein Anteil (`handout_share`) an die
     Haushalte.
- **Kürzung.** Reicht das Geld nicht, wird von hinten gekürzt, mit den Folgen, die jeder
  Anspruch selbst hat: Ausgebliebener Sold wird zum Soldrückstand und drückt die Moral
  der Besatzungen, unbezahlte Forschung kommt langsamer voran, Werften, Nachschub und
  Instandsetzung warten, unbezahlte Reguläre gehen. Die Ansprüche 1 bis 6 greifen notfalls die Rücklage an, die
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
  steht. Gibt es keinen solchen Weg, läuft eine Flotte fern jedem Hafen trotzdem den
  nächsten an (zuerst einen, an dem kein Gegner steht): Wer liegen bleibt, fährt seinen
  Tank leer und liegt danach fest.
- **Entsatz und Feldzüge.** Wird eine eigene Welt belagert, geht die Flotte dorthin
  zuerst, die Hauptwelt voran, und stellt sich dem Belagerer ab `siege.relief_odds`.
  Flotten, die belagern oder einen Feldzug gegen eine Welt führen
  (`AiState::is_reserved`), lässt die Grundhaltung in Ruhe; nur den Rückzug aus einem
  Gefecht befiehlt sie auch ihnen. Über `Headless::set_military_mode` (und `--ai`) lässt
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

## Besitzwechsel, Nachfolge und Unterstellung

- **Eine Stelle für jeden Wechsel.** Eine Welt wechselt nur über `sim::polity::succession`
  von einer Gemeinschaft zu einer anderen; dort entstehen nach Spielbeginn neue
  Gemeinschaften, und dort enden sie. Abspaltung, Übertritt, Eroberung und Vereinigung
  benutzen dieselben vier Vorgänge: gründen (`spawn`), übertragen (`transfer_planet`),
  beenden (`absorb`), unterstellen (`subordinate`). Jeder prüft zuerst und ändert dann;
  Geld, Anteile und Menschen bleiben dabei erhalten.
- **Übertragung einer Welt.** Mit der Welt gehen ihre Bevölkerung, ihre Betriebe und die
  Halter, die dort sitzen. Ein Teil der Anteile des alten Staates an ihren Betrieben geht
  an einen Empfänger, den der Anlass nennt; ihre Kurse geben nach; die Region beginnt mit
  dem Zusammenhalt und der Stufe, die der Anlass vorgibt. Bauaufträge des alten Besitzers
  bei ihrer Werft verfallen ohne Erstattung.
- **Fällt die Hauptwelt**, wird die bevölkerungsreichste verbleibende Welt Hauptwelt. Die
  Handelshäuser ziehen mit, ebenso ein Teil der alten Elite samt Besitz und Geld.
- **Ende einer Gemeinschaft.** Verliert eine Gemeinschaft ihre letzte Welt oder geht sie in
  einer anderen auf, endet sie, und eine Erbin tritt an ihre Stelle. Ihre Welten behalten
  Stufe und Zusammenhalt. Staatskasse, das Geld ihres Staatsdienstes, staatliche Anteile,
  Anleihen, Handelshäuser samt Frachtern und das Wissen gehen über; was die Erbin sich
  danach selbst schuldete, ist getilgt. Ihr Haushalt ist geschlossen, ihre Kriege und
  Waffenruhen enden, und die KI plant für sie nicht mehr. Auf friedlichem Weg gehen auch
  Flotten und Bauaufträge über; tritt sie bei, bleibt ihre Hauptwelt als Sitz kenntlich.
  Unter Zwang werden Flotten und Aufträge aufgelöst, auch mitten im Gefecht.
- **Ende im Gefecht.** Steht eine Flotte der Beendeten im Gefecht, werden die Bücher ihrer
  Partei geschlossen: Was sie bis dahin verloren hat, bleibt der Verlust der Beendeten und
  liegt als Trümmer im System. Eine übernommene Flotte kämpft für die Erbin weiter, sofern
  diese mit dem Gegner im Krieg liegt; die Erbin trägt nur, was die Flotte unter ihr
  verliert. Eine aufgelöste Flotte scheidet aus und belastet niemanden.
- **Unterstellung.** Eine Gemeinschaft kann einer anderen unterstehen. Sie behält Welten,
  Staatskasse und Flotten, teilt aber die Kriege ihres Oberhaupts und kann selbst weder
  Krieg erklären noch Frieden schließen. Oberhaupt und Unterstellte bilden einen Verbund:
  In ihm herrscht nie Krieg, und Ketten gibt es nicht. Kriege bestehen nur zwischen freien
  Gemeinschaften.
- **Neue Gemeinschaften.** Ein Freistaat übernimmt Steuersatz, Kultur und Technik des
  Reichs, das er verlässt, und beginnt ohne Staatskasse, Schulden, Flotte und Handelshaus;
  sein Haushalt wird am nächsten Monatsersten eröffnet. In einer Partie entstehen höchstens
  `endgame.max_free_states` Gemeinschaften durch Abspaltung; danach bleibt eine abtrünnige
  Region autonom.
- **Wege.** Jede Welt trägt, wie sie zu ihrer heutigen Gemeinschaft kam: eigen, beigetreten
  oder genommen, dazu ihre Bevölkerung und den kulturellen Abstand beider Gemeinschaften in
  diesem Augenblick. Eine genommene Welt bleibt genommen, durch jede spätere Vereinigung
  hindurch; erst wenn sie eine neue Gemeinschaft gründet, ist sie wieder eigen.
- **Kultur.** Jede Gemeinschaft hat drei Züge aus ihrer Vorlage (`polities.ron`):
  Offenheit, Gemeinsinn, Wehrhaftigkeit. Aus dem Abstand zweier Kulturen ergibt sich ihre
  Nähe.
- **Gespielte Gemeinschaft.** Endet die Gemeinschaft des Spielers oder wird sie
  unterstellt, ist er ausgeschieden; die Partie läuft ohne ihn weiter.
- **Prüfung.** Die harten Invarianten (`sim::harness::check_invariants`) prüfen zuerst die
  Regeln der Nachfolge: Alle Listen je Gemeinschaft sind gleich lang, jede lebende
  Gemeinschaft besitzt ihre Hauptwelt, ein Oberhaupt lebt und ist frei, und nichts im
  Zustand nennt eine Gemeinschaft, die geendet hat. Geschichte ist davon ausgenommen:
  Chronik, Befehlsprotokoll, Bilanzen beendeter Gefechte, aufgelöste Flotten und Zähler
  über die ganze Partie.

In einer Partie rufen die Abspaltung und die Eroberung diese Vorgänge auf: Jede eroberte
Welt wechselt über `transfer_planet` den Besitzer, auch eine Hauptwelt und die letzte
Welt einer Gemeinschaft. Vereinigung und Unterstellung folgen mit der Diplomatie.

## Belagerung

Der Kampf um einen Planeten zwischen einer Flotte im Orbit und der Verteidigung am Boden
(`crates/sim/src/siege/`). Je Planet steht dauerhaft eine Handvoll Verteidigungswerte
(Stationen, Geschütze, Schild, Bunker, reguläre Verbände, Miliz, Vorräte); eine
Belagerung legt darüber genau einen Datensatz. Der Tagesrechner ist eine reine Funktion
ohne Zufall; Prognose, Tests und Partie rufen dieselbe.

- **Fünf Phasen**, abgeleitet aus dem Zustand und nie gespeichert: Anmarsch (Stationen
  halten den Orbit und das System offen), Einschließung (die Garnison zehrt von ihren
  Vorräten, der Markt vor Ort streckt sie), Beschuss (gezielt oder flächig), Bodenkrieg
  (gelandete Truppen kämpfen Zone um Zone) und Untergrund (die Oberfläche ist verloren,
  Reguläre halten die Bunker). Eine Welt ohne Reguläre im Untergrund fällt aus dem
  Bodenkrieg; eine fünfte Phase hat sie nicht.
- **Drei Wege:** aushungern (langsam, die Beute bleibt heil), stürmen (Wochen, kostet
  Truppen), beschießen (schnell, kostet Beute, Zusammenhalt und eigene Kriegsmüdigkeit).
  Unter feindlichem Orbit wird die Garnison nicht versorgt, also endet jede Belagerung.
- **Befehle:** `BeginSiege`, `SetSiegeOrders` (Feuerstufe, Sturm, Schwerpunkt,
  Plünderung), `LiftSiege`, `EmbarkTroops`, `LandTroops` für den Belagerer;
  `SetGarrisonOrders` und `Capitulate` für den Verteidiger. Wer belagert wird, kann den
  Frieden nicht selbst schließen. Nachschub fährt mit den Truppen: Gehen die letzten
  Verbände einer Flotte von Bord, geht ihr Nachschub mit (zur Landungstruppe oder ins
  Depot der eigenen Welt, soweit es ihn fasst); ohne Truppe bleibt keiner an Bord.
- **Orbit.** Belagern kann eine Flotte, die im System liegt, einsatzbereit ist und
  Treibstoff hat. Im Orbit verbraucht sie, was jede liegende Flotte verbraucht
  (`military.fuel_per_day`). Wer einen Planeten einschließt, sperrt sein System für
  Handel und Nachschub, gleich mit welchem Auftrag seine Flotte dort liegt, wenn der
  Besitzer des Systems mit ihm im Krieg steht (wie bei der Blockade der Flotten). Liegt
  die belagerte Welt im eigenen System des Belagerers oder in dem eines Dritten, bleibt
  das System offen; die Garnison erreicht dessen Markt dann nicht und lebt allein vom
  Depot. Stationen und Geschütze der Welt können Schiffe zerstören: Sie zählen bei den
  verlorenen Schiffen und Flotten ihrer Gemeinschaft, und die Chronik nennt sie. Trifft ein
  Entsatz ein, beginnt nach den Regeln der Flotten ein Gefecht, und die Belagerung ruht:
  Niemand zehrt an Vorräten oder Garnison, die Sperre hält, solange die Flotte des
  Belagerers kämpft. Hält er danach den Orbit, geht dieselbe Belagerung weiter; liegt
  keine seiner Flotten mehr mit Treibstoff über dem Planeten, ist sie gebrochen und
  aufgehoben.
- **Eroberung.** Fällt eine gewöhnliche Welt (gestürmt, ausgehungert oder übergeben),
  wechselt sie am selben Tag den Besitzer. Die Anteile des alten Staates an ihren
  Betrieben gehen an den neuen; private Halter behalten ihre und zählen fortan als
  fremder Besitz. Die Region ist unruhig und besetzt. Ein Vorbesitz des Eroberers an den
  Betrieben schwächt die Miliz und mildert die Besatzung. Holt die Gemeinschaft, der die
  Welt genommen wurde, sie zurück, ist das eine Befreiung, und die Welt zählt im Endspiel
  wieder so, wie sie vor dem Verlust zählte (eigen, beigetreten oder erzwungen). Auf
  Befehl wird geplündert: Ein
  Teil der Kassen der Welt geht an die Staatskasse des Eroberers, Geld wird nur
  verschoben. Der Besitzwechsel läuft über den gemeinsamen Pfad der Nachfolge: Fällt
  eine Hauptwelt, zieht die Hauptwelt der Verlierer auf ihre bevölkerungsreichste
  verbleibende Welt, und ein Teil ihrer Elite geht mit; fällt ihre letzte Welt, endet
  ihre Gemeinschaft, und der Eroberer tritt ihr Erbe unter Zwang an.
- **Folgen.**
  - Produktion: Bodenkrieg legt Werke still (`PlanetEconomy::disruption`); der
    Stillstand klingt nach den Kämpfen von selbst ab.
  - Kurse: Betriebe mit Sitz auf einer belagerten Welt tragen einen Risikoabschlag;
    Beschuss, Eroberung und Plünderung drücken die Stimmung.
  - Zusammenhalt: „Besatzung" und „Kriegsschäden" sind eigene Posten. Eine Garnison
    fängt die Besatzung teilweise auf.
  - Kriegsmüdigkeit: eigene Verluste, Opfer des Beschusses, jeder volle Monat einer
    Belagerung, und der Fall einer Welt (er entlastet den Sieger).
- **Monatsschritt.** Die Miliz einer Welt, die nicht belagert wird, folgt Bevölkerung und
  Zusammenhalt. Kriegsschäden und Besatzung klingen ab; die Regionen spiegeln sie für den
  Zusammenhalt. Eine Besatzung endet, wenn die Welt den Eroberer auf anderem Weg verlässt.
- **Stehende Verteidigung.** Mit `SetDefensePlan` nennt der Besitzer je Welt sein Ziel:
  Stufen der Orbitalstationen, Geschütze, des Schilds und der Bunker (begrenzt durch die
  Technik), reguläre Verbände (höchstens ein Vielfaches der Miliz) und Tage im Depot.
  Bezahlt wird aus dem Anspruch „Verteidigung" des Staatshaushalts (`siege::arms`): am
  Monatsbeginn Sold und Unterhalt an den Staatsdienst, dann Aushebung (Güter und
  Menschen) und der Ausbau; täglich ein Dreißigstel dessen, was dem Depot fehlt (am
  ersten Tag des Monats erst nach dem Sold). Stufe n einer Anlage braucht n Gütersätze.
  Der Markt gibt je Kauf nur einen Teil seines Bestands her: Was er liefert, liegt
  bereit (`PlanetDefense::delivered`), und die Stufe steht, sobald ihr ganzer Satz da
  ist. Was nicht bezahlt wird, verfällt: Bleibt der Sold aus,
  geht je Monat ein Zwanzigstel der Regulären, Depots füllen sich nicht, Anlagen wachsen
  nicht. Heimatwelten beginnen mit Geschützen, Regulären und Vorräten; ihr Plan nennt,
  was steht.
- **Lesemodell** (`siege::view`): je Belagerung eine Karte mit Planet, Parteien, Phase
  (1 bis 5 mit Namen), Fortschritt, Vorräten, Anlagen, Truppen und den drei
  vorausgerechneten Wegen (ruht die Belagerung in einem Gefecht, nennt die Karte ihre
  Flotten weiter und als Grund den umstrittenen Orbit); je Welt die Karte der
  Verteidigung mit Plan, Grenzen und
  Kosten; und `command_block` nennt zu jedem Befehl der Belagerung den Grund, aus dem der
  Kern ihn jetzt ablehnen würde.
- **Chronik:** Beginn und Ende einer Blockade, Beginn einer Belagerung, jeder Wechsel
  der Phase, Schiffe und Flotten, die an die Verteidigung einer Welt verloren gehen,
  Übergabe, Plünderung, Verlust der Hauptwelt und Ende einer Gemeinschaft.
- **KI.** Die Gemeinschaften belagern und verteidigen selbst, siehe den nächsten
  Abschnitt. Das Drehbuch `eroberung` führt daneben einen Angreifer nach festem Plan vor.

Die Stellschrauben stehen in `assets/data/balance.ron` unter `siege`; die Tests in
`crates/sim/tests/siege.rs` spielen ganze Feldzüge über Befehle.

### Wie die KI belagert und verteidigt

`crates/sim/src/ai/siege.rs`. Ob Krieg herrscht, entscheidet die KI weiterhin nicht. Im
Krieg führt sie Feldzüge gegen Welten des Gegners und verteidigt die eigenen, mit
denselben Befehlen wie der Spieler und nie für dessen Gemeinschaft. Alle Schwellen stehen
in `balance.ron` unter `military.ai.siege`.

- **Was sie weiß.** Von einer fremden Welt die Anlagen (sie sind aus dem Orbit zu
  sehen) und die Miliz, die aus der Bevölkerung folgt. Reguläre und Vorräte kennt sie
  nur als Spanne, so breit wie `siege.intel_spread` des wahren Werts; wo die Wahrheit
  darin liegt, steht je Betrachter und Welt fest (`ai::estimate::world_estimate`).
  Geplant wird mit der oberen Grenze. Über die eigene Belagerung liest sie die Karte des
  Lesemodells mit den drei vorausgerechneten Wegen.
- **Zielwahl.** Liegt die wertvollste Flotte im Hafen und steht kein Gegner in einem
  System, das der Gemeinschaft gehört (nach der Mehrheit seiner Bevölkerung; eine eigene
  Welt in fremdem Gebiet zählt nicht), sucht die Führung ein Ziel: Wert der Beute (eine
  Hauptwelt zählt `capital_bonus`-fach), geteilt durch den Weg und durch die geschätzte
  Garnison, gemessen an den Plätzen an Bord. In Frage kommt nur, was vom nächsten
  eigenen Hafen höchstens `reach` des Tanks entfernt liegt, wo die Flotte allem
  gewachsen ist, was im Umkreis eingreifen kann (`odds`), wessen Stationen sie in
  `station_days` niederkämpft und was auf einem der drei Wege zu nehmen ist. Findet sie
  `widen_days` lang nichts, sucht sie weiter: bis `reach_wide` des Tanks und ab
  `odds_wide`. Ein Feldzug beginnt erst, wenn am Sammelplatz wenigstens die
  Besatzungstruppe für das Ziel bereitsteht.
- **Feldzug.** Eine Flotte, ein Ziel, drei Abschnitte. Sammeln: Die Flotte läuft die
  Hauptwelt an, fasst Treibstoff und Munition und schifft Truppen ein, für einen Sturm
  das `storm_ratio`-Fache der geschätzten Verteidiger, mindestens die Besatzungstruppe
  mit Aufschlag, mit `supply_days` Tagen Nachschub; `garrison_keep` der Stammgarnison
  bleibt zu Hause. Kommen Truppen, Treibstoff oder eine sichere Lage am Ziel in
  `muster_days` nicht zusammen, endet der Feldzug, und das Ziel bleibt wählbar. Marsch:
  über den eigenen Hafen, der dem Ziel am nächsten liegt; dort wartet sie, bis der Tank
  `sail_fuel` erreicht. Bevor sie den Sammelplatz und diesen Hafen verlässt, prüft sie
  das Ziel noch einmal wie bei der Wahl, mit den Häfen und der Schätzung von heute und
  den Truppen an Bord: Ging unterwegs ein näherer Hafen verloren oder wurde die Welt zu
  stark, gibt sie auf, statt loszufahren. Belagerung: `BeginSiege`, danach jeden Tag die
  Wahl des Wegs. Die Flotte eines Feldzugs gehört nicht der Grundhaltung der
  Flotten (`AiState::is_reserved`); im Gefecht befiehlt die Grundhaltung auch ihr den
  Rückzug.
- **Aushungern, stürmen, beschießen.** Unter den drei Wegen der Karte gilt der
  billigste, der die Welt nimmt, bevor der Rückweg nicht mehr im Tank ist, ohne die
  Flotte mehr als `hull_limit` ihres Rumpfs und die Truppe mehr als
  `siege.storm_loss_limit` zu kosten. Die Kosten zählen in Anteilen der Beute: Tage
  (`day_cost` und dazu Sold und Wartung der Flotte je Tag im Verhältnis zur Beute mal
  `upkeep_weight`, beides mal 0,5 bis 1,5 nach dem Wagemut der Gemeinschaft und mal
  `haste`, wenn Entsatz droht), verlorene Truppen, verlorene Beute, verlorener Rumpf. So wird
  ausgehungert, wo die Vorräte knapp sind, der Tank reicht und der Orbit sicher ist;
  gestürmt gegen schwache Garnisonen, fern der eigenen Häfen und unter Zeitdruck;
  beschossen, wo beides nicht geht und kein Schild schützt, nie aber, wo die
  Gemeinschaft selbst viel besitzt (`area_stake_max`). Gelandet wird, sobald der gewählte
  Weg es vorsieht; unter Flächenbeschuss erst, wenn der Beschuss schon seit dem Vortag
  gilt und die Probe des Sturms dann noch trägt. Geplündert wird, wenn die Kasse weniger als `sack_below_months`
  Monate Flotte deckt.
- **Aufheben.** Vor einem Entsatz, dem die Flotte nicht gewachsen ist; wenn der Rückweg
  sonst nicht mehr im Tank wäre (heim zum Auftanken, höchstens `sorties` Mal je
  Feldzug, und nur, wenn ein voller Tank vom nächsten Hafen aus über dem Ziel Zeit für
  einen der Wege ließe); wenn die Welt nur mit mehr Truppen zu nehmen ist und zu Hause
  welche stehen (holen und wiederkommen); und wenn kein Weg zum Ziel führt. Eine
  aufgegebene Welt wird `retry_days` lang nicht wieder Ziel, nach jedem weiteren
  Scheitern an ihr ein Vielfaches davon.
- **Nach dem Feldzug.** Endet ein Feldzug (aufgegeben, die Welt gefallen, der
  Sammelplatz verloren), läuft die Flotte den nächsten eigenen Hafen an, bevor die
  Grundhaltung sie übernimmt. Truppen, die dann noch an Bord sind, gehen im Frieden
  sofort und im Krieg `land_after_days` nach dem Ende des Feldzugs im nächsten eigenen
  Hafen an Land, wenn bis dahin kein neuer Feldzug sie braucht.
- **Verteidigungspläne, monatlich.** Jede Welt bekommt ein Ausbauziel nach ihrer Lage:
  Hauptwelt, bedrohte Welt (ein Kriegsgegner höchstens `defense_jumps` Sprünge entfernt,
  oder ein Engpass an fremdem Gebiet) oder Hinterland. Im Krieg gilt das ganze Ziel, im
  Frieden von Anlagen und Regulären nur der Anteil `defense_peace`; was steht, bleibt
  stehen. Ein Depot hält jede Welt immer: Ohne Vorräte löste sich ihre Miliz vom ersten
  Tag einer Einschließung an auf. Die Hauptwelt hält im Krieg dazu die Truppen für den
  Feldzug bereit. Kosten Sold und Unterhalt aller Pläne mehr als `defense_share` der
  Einnahmen, werden alle Pläne in Stufen gekürzt. Es zählen die Einnahmen auf lange
  Sicht, höchstens aber die der letzten drei Monate: Wer Gebiet verloren hat, nimmt
  weniger ein. Übersteigt schon der Unterhalt dessen, was steht,
  `dismantle_after_months` Monate in Folge den Etat, plant die KI auch stehende Anlagen
  ab. Bezahlt wird wie beim Spieler aus dem Anspruch „Verteidigung".
- **Belagerte Welten.** Die Geschütze schweigen unter `hold_fire_days` Tagen Vorrat,
  außer am Tag nach einer Landung. Mit Bunkern geht die Garnison in die Tiefe, wenn die
  gelandete Truppe ihr an der Oberfläche überlegen ist (`deep_ratio`). Entsatz: Eine
  Flotte macht dem Belagerer den Orbit streitig, wenn das Kräfteverhältnis mindestens
  `relief_odds` ist; die Hauptwelt geht vor.
- **Übergabe.** Eine Welt wird übergeben, wenn Halten sinnlos ist: Kein Entsatz ist
  möglich, eine Besatzungstruppe des Belagerers steht bereit, und entweder sind die
  Vorräte leer und von der Garnison steht weniger als `capitulate_garrison`, oder das
  Zentrum ist verloren, im Untergrund sitzt niemand, und die gelandete Truppe ist um
  `capitulate_ratio` überlegen. Ob eine Gemeinschaft überhaupt übergibt, hängt an ihrer
  Haltung: Wagemut und Standhaftigkeit stehen je Gemeinschaft für die ganze Partie fest
  (aus dem Seed; die verborgenen Dispositionen der Diplomatie ersetzen sie später). Wer
  standhafter ist als `capitulate_resolve`, hält bis zum Ende, und seine Hauptwelt
  übergibt nur, wer unter `capitulate_capital_resolve` liegt.
- **Für Drehbücher.** `Headless::set_military_mode`: Mit `flotte-ruht` plant die KI die
  Verteidigung weiter, führt aber weder Flotten noch Feldzüge; mit `ruht` plant sie
  nichts. Einen Verteidigungsplan, den ein Drehbuch gesetzt hat, lässt sie stehen
  (`AiState::defense_hold`).

Der Sweep `--preset feldzug` zeigt das in ganzen Partien; `crates/sim/tests/campaign.rs`
prüft einen solchen Krieg Tag für Tag und urteilt über mehrere Seeds zusammen: dass
Gemeinschaften enden, Truppen nach einem Feldzug an Land gehen, geschrumpfte
Gemeinschaften ihre Verteidigung kürzen und keine Flotte lange mit leerem Tank fern
jedem Hafen liegt. `crates/sim/tests/siege_ai.rs` prüft gestellte Lagen:
Garnisonsbefehle, Entsatz und seinen Vorrang, den Rückzug einer belagernden Flotte und
das Ausschiffen nach einem Feldzug.

## Die beiden Enden

Einmal im Monat wird gezählt. Souverän ist eine freie Gemeinschaft, deren Verbund
mindestens `endgame.sovereign_min_share` aller Menschen hält; kleinere freie
Gemeinschaften zählen nicht.

- **Metamorphose.** Es bleibt genau ein souveräner Verbund, und in ihm sind mehr Menschen
  beigetreten, als genommen wurden. Gezählt wird die Bevölkerung jeder Welt in dem
  Augenblick, in dem sie wechselte.
- **Auslöschung.** Es bleibt genau einer, aber die Genommenen überwiegen oder halten die
  Waage. Oder es bleibt keiner. Oder die Bevölkerung der Galaxie liegt
  `endgame.collapse_months` Monate in Folge unter `endgame.collapse_share` ihres
  Höchststands: Dann endet die Partie ohne Überlebende.

Jede Partie endet höchstens einmal. Mit dem Ausgang steht die Simulation still: Die Uhr
rückt nicht mehr vor, und die Prüfsumme ändert sich nicht mehr. `cargo headless run` nennt
Ende, Jahr und Überlebende; `cargo sweep` listet die Enden und schreibt sie in die letzten
beiden Spalten der CSV (`ending`, `ending_year`). Die Stellschrauben stehen in
`assets/data/balance.ron` unter `endgame`.

## Stand

| Meilenstein | Inhalt | Stand |
|---|---|---|
| 1 | Workspace, Galaxie, Planeten, Karte, Kamera, Zeitsteuerung | fertig |
| 2 | Wirtschaft: Produktion, lokale Preise, Frachter, Warenflüsse | fertig |
| 3 | Aktienmarkt, Besitzverteilung, Zusammenhalt, Abspaltung | fertig |
| 4 | Raumkampf mit Schiffsklassen und Modulen | fertig: Flotten und Werften, Staatshaushalt, Forschung, Gefechte der Partie in der 3D-Ansicht, militärische Führung der KI, Stilllegen von Schiffen |
| 5 | Belagerung, Orbitalgeschütze, Boden- und Untergrundkampf | fertig: Tagesrechner, Belagerung über Befehle in fünf Phasen, Eroberung über den gemeinsamen Besitzwechsel bis zum Ende einer Gemeinschaft, Verteidigung als Anspruch des Staatshaushalts, Folgen für Produktion, Kurse, Zusammenhalt und Kriegsmüdigkeit, Belagerung und Verteidigung durch die KI; im Spiel Fenster „Belagerung“, Tafel der Verteidigung, Marken auf der Karte, Orbitansicht, Meldung vom Ende einer Gemeinschaft |
| 6 | KI-Gemeinschaften, Diplomatie, verborgene Dispositionen, Enden | offen |
| 7 | Speichern/Laden im Menü, Balance, Feinschliff, Tutorial | offen |

Bereits in Meilenstein 1 enthalten, weil spätere Stufen darauf bauen: Gemeinschaften mit
Planetenbesitz, Koloniegründung, Befehlsprotokoll mit Wiedergabe, Spielstände.
