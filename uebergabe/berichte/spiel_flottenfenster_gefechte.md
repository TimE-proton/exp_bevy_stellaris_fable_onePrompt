# Spiel: Haushalt, Flottenfenster, Gefechte der Partie

## summary

Alle sechs Schritte sind auf wip/m4-spiel (e15fee0) umgesetzt, gepusht, Arbeitsbaum sauber. f053104 ist als echter Merge eingebracht; crates/sim, headless und assets sind unverändert gegenüber f053104 (kein Lesehelfer im Kern nötig). Das Fenster „Staat“ zeigt den Haushalt aus Economy::budget. Das Flottenfenster ist neu gegliedert (Flotten und Schiffe / Werften und Kosten) mit Vereinigen, Abteilen, Bestellen, Streichen; gesperrte Knöpfe nennen den Grund des Kerns, ermittelt durch einen Probelauf des Befehls auf einer Kopie der Flotten. Gefechte der Partie laufen in der 3D-Ansicht: gespeist aus Battles.active nach jedem Takt, mit der Uhr der Partie, Befehle als SimCommand, Ergebnis aus der Bilanz der Simulation. Endabnahme nach cargo clean: fmt, build, 178 Tests, clippy -D warnings alle Exit 0; beide Selbsttests OK, Fotos angesehen. Beweis mit echter Eingabe (xdotool, Xvfb :88, 1600x900; Fleet-Fenster zusätzlich bei 1280x720 angesehen): Krieg erklärt, Flotte marschiert, Gefecht bei Ithara in 3D verfolgt bis zum Ergebnis. Spiel und Xvfb per PID beendet. Aus dem Gedächtnis nichts verwendet außer dem bekannten Xvfb-Rezept der Aufgabe; keine Notiz verlangte etwas außerhalb der Aufgabe. Merkenswert: Das Spielfenster öffnet 1600 breit; für 1280x720 muss es per xdotool windowsize verkleinert werden. Bei „IM KRIEG“ verschiebt sich die Kopfleiste, Pause daher per Leertaste statt Klick.

## merge

Genau f053104 als echter Merge-Commit 67fb297 mit deutscher Nachricht; einziger Konflikt war die Meilenstein-Zeile im README. game kompilierte danach unverändert; ein Test war rot (Etat 25 % über dem neuen Höchstetat 18,75 %) und prüft jetzt mit Werten unter dem Höchstetat (1efe36f). Bedeutungsänderungen: Marsch war beim Sammeln schon gesperrt; die Texte zur Forschung nennen jetzt die Rangfolge des Haushalts; „Einnahmen“ im Staatsfenster zeigt Economy::state_income_of statt des Kassenzuwachses.

## budget_panel

crates/game/src/ui/budget.rs, Abschnitt „Haushalt“ im Fenster „Staatsfinanzen“ (Knopf „Staat“), das jetzt rollt. Tafel aus Budget::last() und Budget::year(): Einnahmen, jeder Anspruch in Rangfolge mit Rang und Claim::label (verlangt, bezahlt, offen in Rot), Rest der Einnahmen, Rücklage, Ausschüttung; darunter Kasse nach Abschluss, Rücklagenziel, übrige Bewegungen, unfunded, Soldrückstand der Flotte (Fleets::arrears_of) und ein Erklärtext zu Rangfolge und Kürzungen (wer die Rücklage angreifen darf, aus Claim::may_use_reserve; Ausschüttungsanteil aus balance). Gerechnet wird nur „offen“ = verlangt − bezahlt, wie BudgetMonth::unpaid.

## fleet_window

crates/game/src/ui/fleets/{mod,detail,yards}.rs. Reiter „Flotten und Schiffe“: Liste mit Ort, Zustand, Schiffen je Klasse, Stärke, Munition, Treibstoff, Auftrag, Aufstellung, Doktrin, Marsch/Rückzug; gewählte Flotte mit Auswahlfeldern für die Befehle und jedem Schiff (Klasse, Entwurf, Rumpf, Module, Bordjäger, Munition, Treibstoff, „beschädigt“ markiert). Einen Besatzungszustand je Schiff führt der Kern nicht; gezeigt wird der Soldrückstand. Reiter „Werften und Kosten“: Haushaltszeilen Flotte und Beschaffung (letzter Monat, Summe, laufender Monat), jede Werft mit Bauplätzen und Aufträgen (Fortschritt, BuildOrder::status_text, bezahlt), Entwürfe mit Klasse, Bauwert, Bauzeit, Unterhalt je Monat, Bauteilen; gesperrte Entwürfe nennen die fehlende Technologie; Mehrkosten eines weiteren Schiffs (Differenz von fleet_claim/monthly_upkeep auf einer Kopie der Flotten). Befehle: MoveFleet, Retreat, SetStance, SetFleetOrders, MergeFleets, SplitFleet, BuildShip, BuildClass, CancelBuild. Gesperrt: Marsch im Gefecht/beim Sammeln/am Ziel; Vereinigen, Abteilen und Bestellen genau dann, wenn merge_fleets, split_fleet bzw. order_ship auf einer Kopie ablehnen – der Grund des Kerns steht daneben oder im Schwebetext; Klassenknopf ohne beherrschten Entwurf. Passt bei 1280x720 und 1600x900 (Test), Klicks erreichen die Karte nicht.

## campaign_battles

crates/game/src/battle_view/campaign.rs. Speisung: Nach jedem Takt der Simulation (FixedUpdate, BattleFeedSet::Campaign nach SimRoot) legt `follow` den Stand von Battles.get(id) in den BattleFeed; die Ansicht rechnet nichts (Test vergleicht sim::hash der gezeigten Kopie mit dem Gefecht der Simulation an mehreren Tagen). Zeit: ein Takt der Partie = ein Takt des Gefechts; Pause und Stufe der Partie gelten; die Kopfleiste zeigt Datum und „ein Kalendertag = 2,0 s Gefechtszeit“. Öffnen über „Gefecht ansehen“: Knopf an der Marke des Systems auf der Karte, im Flottenfenster, am Chronikeintrag; dazu ein Hinweis beim Beginn. Befehle: Formation/Doktrin → SetFleetOrders, Rückzug → Retreat, über die CommandQueue als Spielerbefehl (bei Pause Hinweis „Vorgemerkt“); „Neu starten“ gibt es in der Partie nicht; jede eigene Flotte im Gefecht ist befehlbar. Verlassen führt jederzeit zur Karte. Ende bei offener Ansicht: Fenster „Ergebnis des Gefechts“ aus BattleReport (Ausgang mit Sieger, Verluste und Entkommene je Klasse und Partei, Verlustanteil, wer das Feld hält, Verbleib/Ausweichziel aus Route und Sammelfrist der Flotte), „Zur Karte“. Regel für fremde Gefechte (can_watch): eigene immer; fremde nur in Systemen mit eigener liegender Flotte oder eigener Welt; Beobachter ohne Gemeinschaft alle. Laden, Neubeginn und Hauptmenü räumen die Ansicht ab (Sitzungsepoche). Übungsmodus unverändert.

## tests_added

178 Tests (vorher 170). Neu als Ganzspiel-Tests: Haushaltstafel zeigt Zeile für Zeile die Zahlen von Economy::budget; Schiff im Fenster bestellt → Bauauftrag, Klassenbestellung, Streichen, Schiff stößt in der gespielten Partie zur Flotte, volle Werft sperrt mit Grund, gesperrter Entwurf nennt Technologie; Abteilen und Vereinigen über das Fenster samt gesperrter Knöpfe mit Grund; Fenster passt bei beiden Größen, Klick erreicht die Karte nicht; Kampagnengefecht von der Kartenmarke geöffnet, Ansicht gleich Simulation über mehrere Tage, Pause, Doktrinbefehl und Rückzug erreichen die Simulation, Verlassen und Wiedereintritt aus dem Flottenfenster, Ergebnis am Ende gleich Bilanz; Laden und Hauptmenü bei offener Ansicht (Einstieg über die Chronik); Regel für fremde Gefechte. Dazu Einheitentest der Haushaltszeilen; Rundgang um Haushalt und Flottenfenster erweitert; README-Test um die neuen Beschriftungen.

## photos

- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/foto_1_haushalt.png – Fenster Staatsfinanzen am 09.10.2400: Haushalt mit Abschluss 01.09., allen Ansprüchen, offenen Beträgen in Rot, Erklärung
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/foto_2a_flottenfenster_schiffe.png – Flottenfenster nach zwei Gefechten: Flotte „ohne Treibstoff“, Schiffe mit Rumpf/Modulen, eine Korvette „beschädigt“, gesperrte Knöpfe mit Grund
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/foto_2b_werft_auftrag.png – Reiter Werften und Kosten: Auftrag Dolch 63 % im Bau, Kostenzeilen des Haushalts, Entwürfe, Speer II gesperrt, Mehrkosten eines Schiffs
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/foto_3_karte_gefecht_marke.png – Karte am 10.07.2400: Gefecht bei Ithara mit roten Ringen, Knopf „Gefecht ansehen“ an der Marke und in der Chronik, Hinweis oben
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/foto_4_gefecht_der_partie_laeuft.png – dasselbe Gefecht in der 3D-Ansicht, laufend bei 1×, Kopfleiste mit Datum und „ein Kalendertag = 2,0 s Gefechtszeit“
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/foto_5_gefecht_ergebnis.png – Ergebnis des Gefechts: eigene Flotte hält das Feld, Verluste je Klasse, Gegner weicht nach Zeloth aus und sammelt sich bis 18.10.2400
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/f3.png – Reiter Werften und Kosten bei 1280x720 (Zwischenstand vor Schritt 4)
- /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/abschluss/kampagne.png und uebung_1.png bis uebung_4.png im selben Ordner – Fotos der beiden Selbsttests der Endabnahme

## files_changed

- README.md
- crates/game/src/battle_view/campaign.rs (neu)
- crates/game/src/battle_view/feed.rs
- crates/game/src/battle_view/hud.rs
- crates/game/src/battle_view/mod.rs
- crates/game/src/battle_view/practice.rs
- crates/game/src/battle_view/system_tests.rs
- crates/game/src/readme.rs
- crates/game/src/state.rs
- crates/game/src/ui/budget.rs (neu)
- crates/game/src/ui/finance.rs
- crates/game/src/ui/fleets.rs → crates/game/src/ui/fleets/mod.rs
- crates/game/src/ui/fleets/detail.rs (neu)
- crates/game/src/ui/fleets/yards.rs (neu)
- crates/game/src/ui/mod.rs
- crates/game/src/ui/panels.rs
- crates/game/src/ui/tech.rs
- crates/game/src/walkthrough.rs

## checks

Auf e15fee0 nach cargo clean -p sim -p game, Protokolle in /tmp/claude-0/-home-user-exp-bevy-stellaris-fable-onePrompt/3f2a4996-068c-56f0-93b7-939afbb9e098/scratchpad/spiel3/abschluss: cargo fmt -p game -- --check Exit 0; cargo build -p game Exit 0; cargo test -p game Exit 0 (178 bestanden, 0 fehlgeschlagen); cargo clippy -p game --all-targets -- -D warnings Exit 0. Kampagnen-Selbsttest Exit 0: „SELBSTTEST OK: 200 Frames, Datum 04.09.2400, Takt 4875, 16 von 90 Planeten besiedelt, 49 Kartenobjekte“, Foto angesehen. Übungs-Selbsttest Exit 0: „SELBSTTEST OK: Gefechtsübung, 193 Frames, 36 Schiffe, 21 Wracks, 4 Fotos“, Fotos 1 und 4 angesehen (2 und 3 nicht). Nicht gelaufen: Tests und Lint für sim und headless.

## commit

e15fee06207d0a56fd116ac2ddd65a5b91b51f3f auf wip/m4-spiel, gepusht (origin/wip/m4-spiel zeigt darauf), Arbeitsbaum sauber. Folge: 67fb297 (Merge f053104), 1efe36f (Forschungsfenster/Höchstetat), fd298d3 (Haushalt), a1b960c (Flottenfenster), 9ea4fb7 (Gefechte der Partie in 3D mit Tests), e15fee0 (Rundgang, README).

## sim_defects

- Finance::polities[..].revenue (finance/plugin.rs Zeile ~174) schätzt die Staatseinnahmen weiter aus dem Zuwachs der Staatskasse. Seit dem Haushalt wächst die Kasse kaum noch: Im Staatsfenster stand „Einnahmen 0 je Monat“ am Tag 100 von --seed 3 bei laufenden Einnahmen von rund 5.400. Der Wert geht in bonds.rs (annual_revenue) in den Anleihezins ein. Die Oberfläche zeigt jetzt Economy::state_income_of; der Kern ist unverändert.
- Kein Kerndefekt, aber auffällig: In --seed 3 stockt der erste Bauauftrag am Tag 76 mit „Rumpfsegmente am Markt zu knapp“, und die Zeile Flotte zeigte im zweiten Monat 415 von 2.148 Credits bezahlt bei Soldrückstand 0 % (Ersatzteile am Markt nicht zu haben). „Offen“ im Haushalt bedeutet also oft fehlende Ware, nicht fehlendes Geld; die Tafel sagt das dazu.
- Das abgeschlossene Gefecht verschwindet im selben Takt aus Battles.active, in dem sein letzter Schritt gerechnet wird; der Endzustand des Rechners ist von außen nicht mehr lesbar. Die Ansicht zeigt deshalb den Stand einen Takt vor dem Ende und das Ergebnis aus BattleReport.

## open_issues

- Der Knopf an der Kartenmarke liegt in der Ebene der Karte: Der Klick erreicht auch die Karte und wählt dort in der Regel das System des Gefechts. Liegt die Marke unter einer Seitentafel, gibt es den Knopf dort nicht (Flottenfenster und Chronik bleiben).
- Der Chronik-Knopf steht an jedem Kriegseintrag des Systems seit Gefechtsbeginn; ist der Eintrag aus der Chronik gerollt, muss der Spieler hochrollen.
- Fremde Gefechte zeigen alle Einzelheiten der fremden Schiffe (Module, Moral); eine Abstufung nach Einblick gibt es nicht, nur die Ortsregel.
- Die Ansicht bleibt offen, wenn die eigene Flotte vorzeitig ausscheidet; der Spieler sieht dann als Zuschauer weiter zu, auch wenn er nicht mehr im System ist.
- Der Probelauf für gesperrte Knöpfe kopiert Fleets je Prüfung (einige Male je Frame bei offenem Fenster); bei sehr großen Flottenzahlen wäre ein Prüfhelfer im Kern billiger.
- Bauwert der Entwürfe ist der Richtpreis; was ein Bau zu Tagespreisen kostet, weist der Kern nicht öffentlich aus (order_month_cost ist privat).
- Ein Befehl zum Abwracken fehlt im Kern weiterhin; das Fenster kann Flottenkosten nur zeigen, nicht senken.
- Marsch per Kartenziel bleibt wie bisher: Ziel ist das auf der Karte gewählte System; „Auf der Karte zeigen“ ändert diese Auswahl.
- 1280x720 ist für Flotten- und Staatsfenster durch Tests gedeckt und für das Flottenfenster angesehen; die Schlachtansicht der Partie und das Ergebnisfenster wurden nur bei 1600x900 fotografiert.
- Der Rundgang-Test öffnet kein Kampagnengefecht (das decken die Systemtests); Tests und Lint des ganzen Workspace liefen nicht.

