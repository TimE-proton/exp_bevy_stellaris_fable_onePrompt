//! Wem gehört die Maus: der Oberfläche oder der Welt dahinter (Karte, Schlacht)?
//!
//! egui meldet nur Fenster und Flächen, die es selbst verwaltet. Leisten und Seitentafeln
//! zeichnet das Spiel in ein eigenes Wurzel-Ui auf der Hintergrundebene, und von denen
//! weiß egui nichts. Außerdem stammt, was egui meldet, aus dem vorigen Frame und gilt für
//! die Stelle, an der der Zeiger damals stand. Deshalb führt das Spiel selbst Buch:
//!
//! - [`UiCover`] hält fest, welche Teile des Fensters die Oberfläche belegt, und
//!   beantwortet für jede Stelle: Liegt hier Oberfläche? Gilt ein Tastendruck hier ihr?
//! - [`PointerOwner`] entscheidet einmal je Frame, wem Zeiger, Tasten und Mausrad gehören.
//!   Jede Eingabe hat genau einen Besitzer: die Oberfläche, die Welt oder, solange der
//!   Zeiger zum Umsehen gefangen ist, allein die Kamera.
//! - [`WorldMouse`] gibt Klicks, Züge und das Mausrad nur dann an die Welt weiter, wenn
//!   sie ihr gehören.
//!
//! Jedes System, das mit der Maus auf die Welt wirkt, fragt [`WorldMouse`] statt der rohen
//! Eingabe. Was der Welt nicht gehört, bekommt auch die Freiflugsteuerung der Engine nicht
//! zu sehen, und was der Oberfläche nicht gehört, bekommt egui nicht zu sehen.

use bevy::ecs::system::SystemParam;
use bevy::input::InputSystems;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy_egui::{EguiInput, EguiPreUpdateSet, PrimaryEguiContext, egui};

/// Was die Oberfläche vom Fenster belegt, Stand ihres letzten Durchlaufs.
#[derive(Resource, Default)]
pub struct UiCover {
    /// Kontext der Oberfläche: Er kennt Fenster, Aufklappmenüs und andere schwebende
    /// Flächen und jedes Bedienelement des letzten Durchlaufs.
    ctx: Option<egui::Context>,
    /// Was Leisten und Seitentafeln der Hintergrundebene frei lassen.
    free: Option<egui::Rect>,
    /// Durchlauf der Oberfläche, aus dem `free` stammt.
    pass: u64,
}

impl UiCover {
    /// Wurzel-Ui auf der Hintergrundebene über das ganze Fenster. Leisten und Seitentafeln
    /// gehören hier hinein; am Ende meldet [`UiCover::record`], was sie belegen.
    pub fn root(ctx: &egui::Context, name: &'static str) -> egui::Ui {
        egui::Ui::new(
            ctx.clone(),
            name.into(),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(ctx.viewport_rect()),
        )
    }

    /// Hält fest, was die Leisten und Seitentafeln von `root` frei gelassen haben. Zeichnen
    /// im selben Durchlauf mehrere Systeme in eigene Wurzeln, gilt die Schnittmenge.
    pub fn record(&mut self, root: &egui::Ui) {
        let pass = root.ctx().cumulative_pass_nr();
        let free = root.available_rect_before_wrap();
        self.free = Some(match self.free {
            Some(earlier) if self.pass == pass && self.ctx.is_some() => earlier.intersect(free),
            _ => free,
        });
        self.pass = pass;
        self.ctx = Some(root.ctx().clone());
    }

    /// Liegt an dieser Stelle (logische Pixel des Fensters) irgendein Teil der Oberfläche?
    pub fn covers(&self, point: Vec2) -> bool {
        let Some(ctx) = &self.ctx else {
            return false;
        };
        let pos = egui::pos2(point.x, point.y);
        // Ein offenes Aufklappmenü fängt jeden Klick ab, auch den daneben: Der schließt es.
        if ctx.any_popup_open() {
            return true;
        }
        // Fenster und andere schwebende Flächen kennt egui selbst; Flächen, die keine
        // Eingabe annehmen (der Hinweis am oberen Rand), zählt es nicht mit.
        if ctx
            .layer_id_at(pos)
            .is_some_and(|layer| layer.order != egui::Order::Background)
        {
            return true;
        }
        // Leisten und Seitentafeln der Hintergrundebene.
        self.free.is_some_and(|free| !free.contains(pos))
    }

    /// Liegt ein Bedienelement so nah, dass egui einen Tastendruck an dieser Stelle ihm
    /// zuschlägt, obwohl hier keine Oberfläche liegt? Griffe zum Größerziehen ragen über
    /// den Rand ihrer Tafel hinaus, und ein Klick, der einen Knopf um wenige Pixel
    /// verfehlt, gilt noch ihm.
    ///
    /// Gefragt wird, wie egui selbst zu Beginn seines nächsten Durchlaufs zuordnet: mit
    /// den Bedienelementen des letzten Durchlaufs und seinem Fangradius. Die Antwort gilt
    /// deshalb sofort für jede Stelle, auch wenn der Zeiger eben erst dort eintrifft.
    fn reacts_near(&self, point: Vec2) -> bool {
        let Some(ctx) = &self.ctx else {
            return false;
        };
        let pos = egui::pos2(point.x, point.y);
        let reach = ctx.global_style().interaction.interact_radius;
        // Was in einer Fläche liegt, die keine Eingabe annimmt, übergeht egui bei der
        // Zuordnung; gemeldet wird es trotzdem.
        let passive = passive_areas(ctx);
        ctx.interactive_rects_last_pass()
            .iter()
            .filter(|rect| !passive.iter().any(|area| area.contains_rect(**rect)))
            .any(|rect| rect.distance_sq_to_pos(pos) <= reach * reach)
    }

    /// Gehört ein Tastendruck an dieser Stelle der Oberfläche?
    pub fn owns(&self, point: Vec2) -> bool {
        self.covers(point) || self.reacts_near(point)
    }
}

/// Rechtecke der sichtbaren Flächen, durch die Klicks hindurchgehen (der Hinweis am oberen
/// Rand).
fn passive_areas(ctx: &egui::Context) -> Vec<egui::Rect> {
    let visible: Vec<egui::LayerId> = ctx.memory(|memory| {
        memory
            .layer_ids()
            .filter(|layer| memory.areas().is_visible(layer))
            .collect()
    });
    visible
        .into_iter()
        .filter_map(|layer| egui::AreaState::load(ctx, layer.id))
        .filter(|area| !area.interactable)
        .map(|area| area.rect())
        .collect()
}

/// Wo der Zeiger in einem Frame steht, von der Oberfläche aus gesehen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerSpot {
    /// Über einem Teil der Oberfläche: Dort blättert das Mausrad in ihren Listen.
    pub over_ui: bool,
    /// Ein Tastendruck an dieser Stelle gilt der Oberfläche. Das reicht ein wenig weiter
    /// als `over_ui`, siehe [`UiCover::owns`].
    pub press_for_ui: bool,
    /// Gefangen und unsichtbar (Umsehen in der Schlacht): Der Zeiger zeigt auf nichts.
    pub grabbed: bool,
}

impl PointerSpot {
    /// Über der Welt, fern von jeder Oberfläche.
    pub const WORLD: Self = Self {
        over_ui: false,
        press_for_ui: false,
        grabbed: false,
    };
    /// Über der Oberfläche.
    #[cfg(test)]
    pub const UI: Self = Self {
        over_ui: true,
        press_for_ui: true,
        grabbed: false,
    };
    /// Zum Umsehen gefangen.
    pub const GRABBED: Self = Self {
        over_ui: false,
        press_for_ui: false,
        grabbed: true,
    };
}

/// Wem die Maus in diesem Frame gehört.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct PointerOwner {
    spot: PointerSpot,
    /// Tasten, die über der Oberfläche gedrückt wurden und seither gehalten werden. Ein
    /// solcher Zug gehört bis zum Loslassen der Oberfläche, auch wenn er sie verlässt.
    taken: Vec<MouseButton>,
}

impl PointerOwner {
    /// Führt Buch über ein Frame. `spot`: Wo steht der Zeiger jetzt?
    pub fn update(&mut self, spot: PointerSpot, buttons: &ButtonInput<MouseButton>) {
        self.spot = spot;
        self.taken.retain(|button| buttons.pressed(*button));
        if spot.press_for_ui && !spot.grabbed {
            for button in buttons.get_just_pressed() {
                if !self.taken.contains(button) {
                    self.taken.push(*button);
                }
            }
        }
    }

    /// Steht der (sichtbare) Zeiger über der Oberfläche?
    pub fn over_ui(&self) -> bool {
        self.spot.over_ui && !self.spot.grabbed
    }

    /// Ist der Zeiger zum Umsehen gefangen? Dann gehört die Maus allein der Kamera: Der
    /// Zeiger ist unsichtbar, also gilt ein Klick weder einer Tafel noch einem Schiff.
    pub fn grabbed(&self) -> bool {
        self.spot.grabbed
    }

    /// Wurde die Taste über der Oberfläche gedrückt (und noch nicht losgelassen)?
    pub fn taken_by_ui(&self, button: MouseButton) -> bool {
        self.taken.contains(&button)
    }
}

/// Die Maus, soweit sie der Welt gehört. Ersetzt in allen Systemen, die Karte oder
/// Schlacht mit der Maus bedienen, den direkten Griff nach der Eingabe.
#[derive(SystemParam)]
pub struct WorldMouse<'w> {
    buttons: Res<'w, ButtonInput<MouseButton>>,
    scroll: Res<'w, AccumulatedMouseScroll>,
    owner: Res<'w, PointerOwner>,
}

impl WorldMouse<'_> {
    /// In diesem Frame über der Welt gedrückt, mit sichtbarem Zeiger: ein Klick, der auf
    /// etwas zeigt. Solange der Zeiger gefangen ist, gibt es keinen.
    pub fn just_pressed(&self, button: MouseButton) -> bool {
        self.buttons.just_pressed(button)
            && !self.owner.taken_by_ui(button)
            && !self.owner.grabbed()
    }

    /// Gehalten, und der Zug begann über der Welt. Er gilt weiter, wenn der Zeiger dabei
    /// über eine Tafel gerät.
    pub fn pressed(&self, button: MouseButton) -> bool {
        self.buttons.pressed(button) && !self.owner.taken_by_ui(button)
    }

    pub fn any_pressed(&self, buttons: impl IntoIterator<Item = MouseButton>) -> bool {
        buttons.into_iter().any(|button| self.pressed(button))
    }

    /// Gehört das Mausrad der Welt? Über der Oberfläche blättert es dort.
    pub fn wheel_is_free(&self) -> bool {
        !self.owner.over_ui()
    }

    /// Rasten, um die das Mausrad in diesem Frame gedreht wurde; 0 über der Oberfläche.
    pub fn wheel_lines(&self) -> f32 {
        if !self.wheel_is_free() {
            return 0.0;
        }
        match self.scroll.unit {
            MouseScrollUnit::Line => self.scroll.delta.y,
            MouseScrollUnit::Pixel => self.scroll.delta.y / 40.0,
        }
    }
}

pub struct PointerPlugin;

impl Plugin for PointerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiCover>()
            .init_resource::<PointerOwner>()
            .add_systems(
                PreUpdate,
                (
                    // Nach der Eingabe dieses Frames und vor allem, was sie auswertet.
                    track_pointer.after(InputSystems),
                    (
                        keep_wheel_from_world,
                        // Zwischen dem Sammeln der Eingaben für die Oberfläche und ihrem
                        // Durchlauf.
                        keep_grabbed_pointer_from_ui
                            .after(EguiPreUpdateSet::ProcessInput)
                            .before(EguiPreUpdateSet::BeginPass),
                    ),
                )
                    .chain(),
            );
    }
}

/// Stellt fest, wem die Maus in diesem Frame gehört.
pub fn track_pointer(
    cover: Res<UiCover>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Option<Single<(&Window, Option<&CursorOptions>), With<PrimaryWindow>>>,
    mut owner: ResMut<PointerOwner>,
) {
    let spot = window.map_or(PointerSpot::WORLD, |window| {
        let (window, options) = *window;
        // Ein gefangener Zeiger (Umsehen in der Schlacht) ist unsichtbar. Wo er steht oder
        // hinwandert, bedeutet nichts.
        if options.is_some_and(|options| options.grab_mode != CursorGrabMode::None) {
            return PointerSpot::GRABBED;
        }
        let Some(point) = window.cursor_position() else {
            return PointerSpot::WORLD;
        };
        let over_ui = cover.covers(point);
        // Ob ein Druck knapp neben der Oberfläche noch ihr gilt, verlangt einen Blick auf
        // jedes Bedienelement; er lohnt nur in dem Frame, in dem eine Taste niedergeht.
        let pressing = buttons.get_just_pressed().next().is_some();
        PointerSpot {
            over_ui,
            press_for_ui: over_ui || (pressing && cover.owns(point)),
            grabbed: false,
        }
    });
    owner.update(spot, &buttons);
}

/// Über der Oberfläche gehört das Mausrad ihr. Die Welt bekommt es dann gar nicht erst zu
/// sehen, auch die Freiflugsteuerung der Engine nicht, die das Rad selbst liest (sie läuft
/// nach diesem System). So muss niemand eine Wirkung nachträglich zurücknehmen.
fn keep_wheel_from_world(owner: Res<PointerOwner>, mut scroll: ResMut<AccumulatedMouseScroll>) {
    if owner.over_ui() && scroll.delta != Vec2::ZERO {
        scroll.delta = Vec2::ZERO;
    }
}

/// Solange der Zeiger gefangen ist, erfährt die Oberfläche nichts von ihm: egui weiß vom
/// Fangen nichts und ließe sonst jeden Klick und jede Drehung am Mausrad auf das wirken,
/// was gerade unter der unsichtbaren Stelle liegt.
fn keep_grabbed_pointer_from_ui(
    owner: Res<PointerOwner>,
    mut input: Option<Single<&mut EguiInput, With<PrimaryEguiContext>>>,
) {
    if owner.grabbed()
        && let Some(input) = input.as_mut()
    {
        withhold_pointer(&mut input.events);
    }
}

/// Streicht aus den Eingaben eines Frames alles, was einen Zeiger voraussetzt, und meldet
/// den Zeiger als fort (nichts wird mehr hervorgehoben, kein Hinweistext erscheint).
/// Losgelassene Tasten kommen durch: Was vor dem Fangen begonnen wurde, etwa ein Zug an
/// einem Rollbalken, endet so sauber. Einen Klick ergibt ein Loslassen allein nicht.
fn withhold_pointer(events: &mut Vec<egui::Event>) {
    events.retain(|event| match event {
        egui::Event::PointerButton { pressed, .. } => !pressed,
        egui::Event::PointerMoved(_)
        | egui::Event::MouseMoved(_)
        | egui::Event::MouseWheel { .. }
        | egui::Event::Zoom(_)
        | egui::Event::Rotate(_)
        | egui::Event::Touch { .. } => false,
        _ => true,
    });
    events.push(egui::Event::PointerGone);
}

#[cfg(test)]
pub mod testing {
    use super::*;

    /// Welt-Resourcen für Tests, die ein System mit [`WorldMouse`] direkt aufrufen: Die
    /// Maus gehört der Welt.
    pub fn free_mouse(world: &mut World) {
        world.init_resource::<ButtonInput<MouseButton>>();
        world.init_resource::<AccumulatedMouseScroll>();
        world.init_resource::<PointerOwner>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: egui::Vec2 = egui::vec2(1600.0, 900.0);

    /// Ein Durchlauf der Oberfläche, wie ihn die Engine fährt: Das Wurzel-Ui, das egui
    /// selbst mitbringt, bleibt ungenutzt; gezeichnet wird in eine eigene Wurzel.
    fn pass(ctx: &egui::Context, cover: &mut UiCover, draw: impl FnMut(&mut egui::Ui)) {
        pass_with(ctx, cover, Vec::new(), draw);
    }

    /// Wie [`pass`], mit den Eingaben dieses Durchlaufs.
    fn pass_with(
        ctx: &egui::Context,
        cover: &mut UiCover,
        events: Vec<egui::Event>,
        mut draw: impl FnMut(&mut egui::Ui),
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |_| {
            let mut root = UiCover::root(ctx, "wurzel");
            draw(&mut root);
            cover.record(&root);
        });
        output.textures_delta.clear();
    }

    /// Kopfleiste, Chronik unten, Tafeln links und rechts, wie in der Partie.
    fn panels(root: &mut egui::Ui) {
        egui::Panel::top("kopf").show(root, |ui| {
            ui.label("Kopfleiste");
        });
        egui::Panel::bottom("chronik")
            .resizable(true)
            .default_size(118.0)
            .size_range(60.0..=320.0)
            .show(root, |ui| {
                ui.label("Chronik");
            });
        egui::Panel::left("links")
            .resizable(true)
            .default_size(430.0)
            .size_range(300.0..=640.0)
            .show(root, |ui| {
                ui.label("Auswahl");
            });
        egui::Panel::right("rechts")
            .resizable(true)
            .default_size(290.0)
            .size_range(220.0..=420.0)
            .show(root, |ui| {
                ui.label("Gemeinschaften");
            });
    }

    #[test]
    fn nothing_is_covered_before_the_first_pass() {
        let cover = UiCover::default();
        assert!(!cover.covers(Vec2::new(10.0, 10.0)));
        assert!(!cover.owns(Vec2::new(10.0, 10.0)));
    }

    #[test]
    fn bars_and_side_panels_cover_their_part_of_the_window() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        for _ in 0..2 {
            pass(&ctx, &mut cover, panels);
        }
        for (point, covered, what) in [
            (Vec2::new(800.0, 8.0), true, "Kopfleiste"),
            (Vec2::new(1580.0, 3.0), true, "Kopfleiste, rechter Rand"),
            (Vec2::new(200.0, 400.0), true, "linke Tafel"),
            (Vec2::new(280.0, 800.0), true, "linke Tafel, untere Ecke"),
            (Vec2::new(1450.0, 400.0), true, "rechte Tafel"),
            (Vec2::new(800.0, 870.0), true, "Chronik"),
            (Vec2::new(800.0, 400.0), false, "freie Karte"),
            (
                Vec2::new(660.0, 40.0),
                false,
                "freie Karte, obere linke Ecke",
            ),
            (
                Vec2::new(1160.0, 800.0),
                false,
                "freie Karte, untere rechte Ecke",
            ),
        ] {
            assert_eq!(cover.covers(point), covered, "{what} bei {point}");
            assert_eq!(cover.owns(point), covered, "{what} bei {point}");
        }
    }

    #[test]
    fn windows_cover_and_passive_areas_do_not() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        // Der Hinweis nimmt keine Eingabe an: Klicks gehen durch ihn hindurch, auch durch
        // seinen Text (der sich sonst markieren ließe) und durch einen Knopf darin.
        let pressed = std::cell::Cell::new(false);
        let button = std::cell::Cell::new(egui::Rect::NOTHING);
        let draw = |root: &mut egui::Ui| {
            egui::Window::new("tafel")
                .title_bar(false)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -8.0])
                .show(root.ctx(), |ui| {
                    ui.set_width(300.0);
                    ui.set_height(200.0);
                    ui.label("Tafel");
                });
            egui::Area::new("hinweis".into())
                .anchor(egui::Align2::CENTER_TOP, [0.0, 44.0])
                .interactable(false)
                .show(root.ctx(), |ui| {
                    ui.label("Gespeichert: saves/schnell.ron");
                    let response = ui.button("Knopf im Hinweis");
                    pressed.set(pressed.get() || response.clicked());
                    button.set(response.rect);
                });
        };
        for _ in 0..3 {
            pass(&ctx, &mut cover, draw);
        }
        let button = button.get();
        assert!(cover.covers(Vec2::new(1500.0, 800.0)), "auf der Tafel");
        assert!(!cover.covers(Vec2::new(1100.0, 800.0)), "neben der Tafel");
        assert!(!cover.owns(Vec2::new(1100.0, 800.0)), "neben der Tafel");
        assert!(!cover.covers(Vec2::new(800.0, 52.0)), "auf dem Hinweis");
        assert!(!cover.owns(Vec2::new(800.0, 52.0)), "auf dem Hinweis");
        let on_button = Vec2::new(button.center().x, button.center().y);
        assert!(!cover.owns(on_button), "auf dem Knopf im Hinweis");
        // Ohne Leisten ist der Hintergrund ganz frei.
        assert!(!cover.covers(Vec2::new(800.0, 8.0)));

        // Die Oberfläche sieht es genauso: Ein Klick auf den Knopf im Hinweis drückt ihn
        // nicht. Der Klick gehört also der Welt und nur ihr.
        let press = |pressed: bool| egui::Event::PointerButton {
            pos: button.center(),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        for events in [
            vec![egui::Event::PointerMoved(button.center())],
            vec![press(true)],
            vec![press(false)],
            Vec::new(),
        ] {
            pass_with(&ctx, &mut cover, events, draw);
        }
        assert!(!pressed.get(), "der Hinweis hat einen Klick angenommen");
    }

    #[test]
    fn an_open_popup_takes_every_click() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        let mut open = false;
        for step in 0..4 {
            pass(&ctx, &mut cover, |root| {
                egui::Panel::top("kopf").show(root, |ui| {
                    let button = ui.button("Auswahl");
                    egui::Popup::menu(&button).open(open).show(|ui| {
                        ui.label("Eintrag");
                    });
                });
            });
            if step == 1 {
                assert!(!cover.covers(Vec2::new(800.0, 500.0)), "Menü zu");
                open = true;
            }
        }
        assert!(cover.covers(Vec2::new(800.0, 500.0)), "Menü offen");
    }

    #[test]
    fn two_roots_in_one_pass_add_up() {
        // In der Schlachtansicht der Partie zeichnen zwei Systeme in eigene Wurzeln.
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |_| {
            let mut first = UiCover::root(&ctx, "eins");
            egui::Panel::top("kopf").show(&mut first, |ui| {
                ui.label("Kopfleiste");
            });
            cover.record(&first);
            let second = UiCover::root(&ctx, "zwei");
            cover.record(&second);
        });
        output.textures_delta.clear();
        assert!(cover.covers(Vec2::new(800.0, 8.0)));
        assert!(!cover.covers(Vec2::new(800.0, 400.0)));
        // Der nächste Durchlauf beginnt neu: Ohne Leiste ist die Stelle wieder frei.
        pass(&ctx, &mut cover, |_| {});
        assert!(!cover.covers(Vec2::new(800.0, 8.0)));
    }

    /// Erste Stelle rechts von `from`, an der in Höhe `y` keine Oberfläche mehr liegt.
    fn first_free_x(cover: &UiCover, from: f32, y: f32) -> f32 {
        (0..1600)
            .map(|step| from + step as f32 + 0.5)
            .find(|x| !cover.covers(Vec2::new(*x, y)))
            .expect("freie Karte")
    }

    #[test]
    fn a_press_on_a_resize_handle_belongs_to_the_panel() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        // Der Zeiger war nie in der Nähe: Die Antwort hängt nicht davon ab, dass die
        // Oberfläche den Griff schon unter dem Zeiger gesehen hat.
        for _ in 0..2 {
            pass(&ctx, &mut cover, panels);
        }
        // Rechter Rand der linken Tafel: Dort sitzt der Griff zum Größerziehen. Er ragt ein
        // Stück über die Tafel hinaus.
        let edge = first_free_x(&cover, 0.0, 400.0);
        let beside = Vec2::new(edge + 1.5, 400.0);
        assert!(
            !cover.covers(beside),
            "neben der Tafel liegt keine Oberfläche"
        );
        assert!(cover.owns(beside), "der Griff gehört der Oberfläche");
        // Ebenso an der rechten Tafel und über der Chronik.
        let right = (0..1600)
            .map(|step| 1599.5 - step as f32)
            .find(|x| !cover.covers(Vec2::new(*x, 400.0)))
            .expect("freie Karte");
        assert!(cover.owns(Vec2::new(right - 1.5, 400.0)));
        let above_chronicle = (0..900)
            .map(|step| 899.5 - step as f32)
            .find(|y| !cover.covers(Vec2::new(800.0, *y)))
            .expect("freie Karte");
        assert!(cover.owns(Vec2::new(800.0, above_chronicle - 1.5)));
        // Die Kopfleiste hat keinen Griff: Dicht unter ihr gehört ein Druck der Karte.
        let below_bar = (0..900)
            .map(|step| step as f32 + 0.5)
            .find(|y| !cover.covers(Vec2::new(800.0, *y)))
            .expect("freie Karte");
        assert!(!cover.owns(Vec2::new(800.0, below_bar + 1.5)));
        // Weit weg von jeder Tafel ohnehin.
        assert!(!cover.owns(Vec2::new(800.0, 400.0)));
        assert!(!cover.owns(Vec2::new(edge + 30.0, 400.0)));
    }

    #[test]
    fn a_press_just_beside_a_button_belongs_to_the_button() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        // Ein Knopf, ein gesperrter Knopf, bloßer Text und Text, der sich markieren lässt
        // (so zeichnet die Oberfläche Text, wenn nichts anderes verlangt ist).
        let mut rects = [egui::Rect::NOTHING; 4];
        for _ in 0..2 {
            pass(&ctx, &mut cover, |root| {
                // Ohne Rand: Die Bedienelemente reichen bis dicht an die Unterkante.
                egui::Panel::top("kopf")
                    .frame(egui::Frame::NONE)
                    .show(root, |ui| {
                        ui.horizontal(|ui| {
                            rects[0] = ui.button("Menü").rect;
                            ui.add_space(200.0);
                            rects[1] = ui.add_enabled(false, egui::Button::new("gesperrt")).rect;
                            ui.add_space(200.0);
                            rects[2] = ui.add(egui::Label::new("nur Text").selectable(false)).rect;
                            ui.add_space(200.0);
                            rects[3] = ui.label("markierbarer Text").rect;
                        });
                    });
            });
        }
        let [button, locked, text, selectable] = rects;
        let reach = ctx.global_style().interaction.interact_radius;
        assert!(reach >= 3.0, "Fangradius {reach}");
        // Erste Stelle unter der Leiste, senkrecht unter der Mitte eines Bedienelements: Dort
        // liegt keine Oberfläche mehr, das Element aber noch in Reichweite.
        let below = |rect: egui::Rect| {
            let spot = (0..900)
                .map(|step| Vec2::new(rect.center().x, step as f32 + 0.5))
                .find(|spot| !cover.covers(*spot))
                .expect("freie Fläche unter der Leiste");
            let distance = rect.distance_to_pos(egui::pos2(spot.x, spot.y));
            assert!(
                distance > 0.0 && distance < reach,
                "{rect:?} liegt {distance} von {spot} entfernt"
            );
            spot
        };
        // egui schlägt den Druck dem Knopf zu, also gehört er der Oberfläche.
        assert!(cover.owns(below(button)));
        // Knapp außerhalb des Fangradius gehört er der Welt.
        let far = Vec2::new(button.center().x, button.bottom() + reach + 0.5);
        assert!(!cover.covers(far));
        assert!(!cover.owns(far));
        // Ein gesperrter Knopf und bloßer Text fangen nichts.
        assert!(!cover.owns(below(locked)), "unter dem gesperrten Knopf");
        assert!(!cover.owns(below(text)), "unter bloßem Text");
        // Text, der sich markieren lässt, nimmt den Druck an (dort beginnt das Markieren):
        // Auch dieser Druck hat genau einen Besitzer, die Oberfläche.
        assert!(cover.owns(below(selectable)), "unter markierbarem Text");
    }

    fn frame(owner: &mut PointerOwner, buttons: &mut ButtonInput<MouseButton>, spot: PointerSpot) {
        owner.update(spot, buttons);
    }

    #[test]
    fn a_click_belongs_to_whoever_lies_under_it() {
        let (ui, world) = (PointerSpot::UI, PointerSpot::WORLD);
        let mut owner = PointerOwner::default();
        let mut buttons = ButtonInput::<MouseButton>::default();
        // Über der Oberfläche gedrückt: Die Welt bekommt nichts davon mit.
        buttons.press(MouseButton::Left);
        frame(&mut owner, &mut buttons, ui);
        assert!(owner.taken_by_ui(MouseButton::Left));
        buttons.clear();
        // Der Zug verlässt die Oberfläche und gehört ihr weiter.
        frame(&mut owner, &mut buttons, world);
        assert!(owner.taken_by_ui(MouseButton::Left));
        buttons.release(MouseButton::Left);
        frame(&mut owner, &mut buttons, world);
        assert!(!owner.taken_by_ui(MouseButton::Left));
        buttons.clear();

        // Über der Welt gedrückt: Der Zug bleibt bei der Welt, auch über einer Tafel.
        buttons.press(MouseButton::Right);
        frame(&mut owner, &mut buttons, world);
        assert!(!owner.taken_by_ui(MouseButton::Right));
        buttons.clear();
        frame(&mut owner, &mut buttons, ui);
        assert!(!owner.taken_by_ui(MouseButton::Right));
        assert!(owner.over_ui());
        buttons.release(MouseButton::Right);
        buttons.clear();

        // Drücken und Loslassen im selben Frame über der Oberfläche.
        buttons.press(MouseButton::Left);
        buttons.release(MouseButton::Left);
        frame(&mut owner, &mut buttons, ui);
        assert!(owner.taken_by_ui(MouseButton::Left));
        buttons.clear();
        frame(&mut owner, &mut buttons, ui);
        assert!(!owner.taken_by_ui(MouseButton::Left));

        // Knapp neben der Oberfläche, wo ein Druck noch ihr gilt: Die Taste gehört ihr,
        // das Mausrad der Welt.
        let beside = PointerSpot {
            press_for_ui: true,
            ..world
        };
        buttons.press(MouseButton::Left);
        frame(&mut owner, &mut buttons, beside);
        assert!(owner.taken_by_ui(MouseButton::Left));
        assert!(!owner.over_ui());
        buttons.release(MouseButton::Left);
        buttons.clear();
        frame(&mut owner, &mut buttons, world);
    }

    #[test]
    fn a_grabbed_pointer_belongs_to_nobody_but_the_camera() {
        let mut owner = PointerOwner::default();
        let mut buttons = ButtonInput::<MouseButton>::default();
        // Gefangen gedrückt: Die Oberfläche nimmt die Taste nicht, wo auch immer der
        // unsichtbare Zeiger steht.
        buttons.press(MouseButton::Left);
        frame(&mut owner, &mut buttons, PointerSpot::GRABBED);
        assert!(owner.grabbed());
        assert!(!owner.taken_by_ui(MouseButton::Left));
        assert!(!owner.over_ui());
        // Auch dann nicht, wenn die Stelle als Oberfläche gemeldet würde.
        buttons.clear();
        buttons.press(MouseButton::Middle);
        let hidden_over_panel = PointerSpot {
            grabbed: true,
            ..PointerSpot::UI
        };
        frame(&mut owner, &mut buttons, hidden_over_panel);
        assert!(!owner.taken_by_ui(MouseButton::Middle));
        assert!(!owner.over_ui());
        // Was vor dem Fangen der Oberfläche gehörte, gehört ihr bis zum Loslassen.
        let mut owner = PointerOwner::default();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Left);
        frame(&mut owner, &mut buttons, PointerSpot::UI);
        buttons.clear();
        frame(&mut owner, &mut buttons, PointerSpot::GRABBED);
        assert!(owner.taken_by_ui(MouseButton::Left));
        buttons.release(MouseButton::Left);
        frame(&mut owner, &mut buttons, PointerSpot::GRABBED);
        assert!(!owner.taken_by_ui(MouseButton::Left));
    }

    #[test]
    fn the_world_mouse_only_reports_what_belongs_to_the_world() {
        use bevy::ecs::system::RunSystemOnce;

        let mut world = World::new();
        testing::free_mouse(&mut world);
        let read = |world: &mut World| {
            world
                .run_system_once(|mouse: WorldMouse| {
                    (
                        mouse.just_pressed(MouseButton::Left),
                        mouse.pressed(MouseButton::Left),
                        mouse.any_pressed([MouseButton::Right, MouseButton::Middle]),
                        mouse.wheel_lines(),
                    )
                })
                .unwrap()
        };
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, 2.0);
        assert_eq!(read(&mut world), (true, true, false, 2.0));
        // Dieselbe Eingabe über der Oberfläche.
        let buttons = world.resource::<ButtonInput<MouseButton>>().clone();
        world
            .resource_mut::<PointerOwner>()
            .update(PointerSpot::UI, &buttons);
        assert_eq!(read(&mut world), (false, false, false, 0.0));
        // Dieselbe Eingabe mit gefangenem Zeiger: kein Klick, der auf etwas zeigt, aber
        // das Mausrad gehört der Welt (der Kamera).
        *world.resource_mut::<PointerOwner>() = PointerOwner::default();
        world
            .resource_mut::<PointerOwner>()
            .update(PointerSpot::GRABBED, &buttons);
        assert_eq!(read(&mut world), (false, true, false, 2.0));
        // Ein Zug mit der mittleren Taste, über der Welt begonnen.
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Middle);
        world
            .resource_mut::<PointerOwner>()
            .update(PointerSpot::WORLD, &buttons);
        world.insert_resource(buttons);
        world.resource_mut::<AccumulatedMouseScroll>().unit = MouseScrollUnit::Pixel;
        world.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, -80.0);
        assert_eq!(read(&mut world), (false, false, true, -2.0));
    }

    #[test]
    fn the_wheel_over_the_interface_never_reaches_the_world() {
        use bevy::ecs::system::RunSystemOnce;

        let mut world = World::new();
        testing::free_mouse(&mut world);
        let buttons = ButtonInput::<MouseButton>::default();
        let turn = |world: &mut World, spot: PointerSpot| {
            world.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, 3.0);
            world.resource_mut::<PointerOwner>().update(spot, &buttons);
            world.run_system_once(keep_wheel_from_world).unwrap();
            world.resource::<AccumulatedMouseScroll>().delta.y
        };
        // Wer das Rad nach diesem System liest (die Freiflugsteuerung der Engine), sieht
        // über der Oberfläche keine Drehung.
        assert_eq!(turn(&mut world, PointerSpot::UI), 0.0);
        assert_eq!(turn(&mut world, PointerSpot::WORLD), 3.0);
        assert_eq!(turn(&mut world, PointerSpot::GRABBED), 3.0);
    }

    #[test]
    fn a_grabbed_pointer_is_withheld_from_the_interface() {
        let at = egui::pos2(300.0, 200.0);
        let button = |pressed: bool| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -3.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        };
        let key = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        };
        let mut events = vec![
            egui::Event::PointerMoved(at),
            button(true),
            wheel,
            key.clone(),
            button(false),
            egui::Event::Text("m".into()),
        ];
        withhold_pointer(&mut events);
        // Tasten und Text bleiben, ebenso das Loslassen; am Ende gilt der Zeiger als fort.
        assert_eq!(
            events,
            vec![
                key,
                button(false),
                egui::Event::Text("m".into()),
                egui::Event::PointerGone
            ]
        );

        // Was das für die Oberfläche heißt: Ein Knopf unter der Stelle wird weder gedrückt
        // noch hervorgehoben, auch nicht durch das Loslassen allein.
        let ctx = egui::Context::default();
        let (_, _, at) = button_pass(&ctx, Vec::new());
        let press = |pressed: bool| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let steps = || {
            [
                vec![egui::Event::PointerMoved(at)],
                vec![press(true)],
                vec![press(false)],
                vec![egui::Event::PointerMoved(at)],
            ]
        };
        // Ohne Fangen drückt der Ablauf den Knopf und hebt ihn hervor.
        let mut clicks = 0;
        let mut hovered = false;
        for events in steps() {
            let (clicked, hover, _) = button_pass(&ctx, events);
            clicks += clicked as u32;
            hovered = hover;
        }
        assert_eq!(clicks, 1);
        assert!(hovered);
        // Gefangen: derselbe Ablauf an derselben Stelle.
        for mut events in steps() {
            withhold_pointer(&mut events);
            let (clicked, hovered, _) = button_pass(&ctx, events);
            assert!(!clicked, "ein gefangener Zeiger hat den Knopf gedrückt");
            assert!(!hovered, "der Knopf unter dem unsichtbaren Zeiger leuchtet");
        }
    }

    /// Ein Durchlauf mit einem einzelnen Knopf: Wurde er gedrückt, ist er hervorgehoben,
    /// wo liegt seine Mitte?
    fn button_pass(ctx: &egui::Context, events: Vec<egui::Event>) -> (bool, bool, egui::Pos2) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            events,
            ..Default::default()
        };
        let mut seen = (false, false, egui::Pos2::ZERO);
        let mut output = ctx.run_ui(input, |ui| {
            let response = ui.button("Rückzug befehlen");
            seen = (
                response.clicked(),
                response.hovered(),
                response.rect.center(),
            );
        });
        output.textures_delta.clear();
        seen
    }
}
