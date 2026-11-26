//! Wem gehört die Maus: der Oberfläche oder der Welt dahinter (Karte, Schlacht)?
//!
//! egui meldet nur Fenster und Flächen, die es selbst verwaltet. Leisten und Seitentafeln
//! zeichnet das Spiel in ein eigenes Wurzel-Ui auf der Hintergrundebene, und von denen
//! weiß egui nichts. Außerdem stammt, was egui meldet, aus dem vorigen Frame und gilt für
//! die Stelle, an der der Zeiger damals stand. Deshalb führt das Spiel selbst Buch:
//!
//! - [`UiCover`] hält fest, welche Teile des Fensters die Oberfläche belegt, und
//!   beantwortet für jede Stelle: Liegt hier Oberfläche?
//! - [`WorldMouse`] gibt Klicks, Züge und das Mausrad nur dann an die Welt weiter, wenn
//!   sie über der Welt begonnen haben.
//!
//! Jedes System, das mit der Maus auf die Welt wirkt, fragt [`WorldMouse`] statt der rohen
//! Eingabe.

use bevy::ecs::system::SystemParam;
use bevy::input::InputSystems;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy_egui::{EguiOutput, PrimaryEguiContext, egui};

/// Was die Oberfläche vom Fenster belegt, Stand ihres letzten Durchlaufs.
#[derive(Resource, Default)]
pub struct UiCover {
    /// Kontext der Oberfläche: Er kennt Fenster, Aufklappmenüs und andere schwebende Flächen.
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

    /// Liegt Oberfläche in Griffweite der Stelle? Die Griffe, an denen man Fenster und
    /// Tafeln größer zieht, ragen ein Stück über deren Rand hinaus.
    fn within_reach(&self, point: Vec2) -> bool {
        let Some(ctx) = &self.ctx else {
            return false;
        };
        let reach = ctx.global_style().interaction.resize_grab_radius_corner;
        [
            (0.0, 0.0),
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (1.0, 1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
        ]
        .into_iter()
        .any(|(x, y)| self.covers(point + Vec2::new(x, y) * reach))
    }

    /// Gehört die Maus an dieser Stelle der Oberfläche? `cursor` ist das Zeigersymbol, das
    /// die Oberfläche zuletzt verlangt hat: Über einem Griff zum Größerziehen zeigt es
    /// einen Doppelpfeil, auch knapp neben dem Rand.
    pub fn owns(&self, point: Vec2, cursor: egui::CursorIcon) -> bool {
        self.covers(point) || (is_resize(cursor) && self.within_reach(point))
    }
}

fn is_resize(cursor: egui::CursorIcon) -> bool {
    use egui::CursorIcon::*;
    matches!(
        cursor,
        ResizeHorizontal
            | ResizeVertical
            | ResizeNeSw
            | ResizeNwSe
            | ResizeEast
            | ResizeSouthEast
            | ResizeSouth
            | ResizeSouthWest
            | ResizeWest
            | ResizeNorthWest
            | ResizeNorth
            | ResizeNorthEast
            | ResizeColumn
            | ResizeRow
    )
}

/// Wem die Maus in diesem Frame gehört.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct PointerOwner {
    /// Der Zeiger steht über der Oberfläche.
    over_ui: bool,
    /// Tasten, die über der Oberfläche gedrückt wurden und seither gehalten werden. Ein
    /// solcher Zug gehört bis zum Loslassen der Oberfläche, auch wenn er sie verlässt.
    taken: Vec<MouseButton>,
}

impl PointerOwner {
    /// Führt Buch über ein Frame. `over_ui`: Steht der Zeiger jetzt über der Oberfläche?
    pub fn update(&mut self, over_ui: bool, buttons: &ButtonInput<MouseButton>) {
        self.over_ui = over_ui;
        self.taken.retain(|button| buttons.pressed(*button));
        if over_ui {
            for button in buttons.get_just_pressed() {
                if !self.taken.contains(button) {
                    self.taken.push(*button);
                }
            }
        }
    }

    pub fn over_ui(&self) -> bool {
        self.over_ui
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
    /// In diesem Frame über der Welt gedrückt.
    pub fn just_pressed(&self, button: MouseButton) -> bool {
        self.buttons.just_pressed(button) && !self.owner.taken_by_ui(button)
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
            // Nach der Eingabe dieses Frames und vor allem, was sie auswertet.
            .add_systems(PreUpdate, track_pointer.after(InputSystems));
    }
}

/// Stellt fest, wem die Maus in diesem Frame gehört.
pub fn track_pointer(
    cover: Res<UiCover>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Option<Single<(&Window, Option<&CursorOptions>), With<PrimaryWindow>>>,
    output: Option<Single<&EguiOutput, With<PrimaryEguiContext>>>,
    mut owner: ResMut<PointerOwner>,
) {
    let over_ui = window.is_some_and(|window| {
        let (window, options) = *window;
        // Ein gefangener Zeiger (Umsehen in der Schlacht) steht still, wo er gefangen
        // wurde, und zeigt auf nichts.
        let grabbed = options.is_some_and(|options| options.grab_mode != CursorGrabMode::None);
        let cursor = output.as_ref().map_or(egui::CursorIcon::Default, |output| {
            output.platform_output.cursor_icon
        });
        !grabbed
            && window
                .cursor_position()
                .is_some_and(|point| cover.owns(point, cursor))
    });
    owner.update(over_ui, &buttons);
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
    fn pass(
        ctx: &egui::Context,
        cover: &mut UiCover,
        pointer: Option<egui::Pos2>,
        mut draw: impl FnMut(&mut egui::Ui),
    ) -> egui::CursorIcon {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            ..Default::default()
        };
        if let Some(pointer) = pointer {
            input.events.push(egui::Event::PointerMoved(pointer));
        }
        let mut output = ctx.run_ui(input, |_| {
            let mut root = UiCover::root(ctx, "wurzel");
            draw(&mut root);
            cover.record(&root);
        });
        output.textures_delta.clear();
        output.platform_output.cursor_icon
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
        assert!(!cover.owns(Vec2::new(10.0, 10.0), egui::CursorIcon::ResizeHorizontal));
    }

    #[test]
    fn bars_and_side_panels_cover_their_part_of_the_window() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        for _ in 0..2 {
            pass(&ctx, &mut cover, None, panels);
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
        }
    }

    #[test]
    fn windows_cover_and_passive_areas_do_not() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
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
            // Der Hinweis nimmt keine Eingabe an: Klicks gehen durch ihn hindurch.
            egui::Area::new("hinweis".into())
                .anchor(egui::Align2::CENTER_TOP, [0.0, 44.0])
                .interactable(false)
                .show(root.ctx(), |ui| {
                    ui.label("Gespeichert: saves/schnell.ron");
                });
        };
        for _ in 0..3 {
            pass(&ctx, &mut cover, None, draw);
        }
        assert!(cover.covers(Vec2::new(1500.0, 800.0)), "auf der Tafel");
        assert!(!cover.covers(Vec2::new(1100.0, 800.0)), "neben der Tafel");
        assert!(!cover.covers(Vec2::new(800.0, 52.0)), "auf dem Hinweis");
        // Ohne Leisten ist der Hintergrund ganz frei.
        assert!(!cover.covers(Vec2::new(800.0, 8.0)));
    }

    #[test]
    fn an_open_popup_takes_every_click() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        let mut open = false;
        for step in 0..4 {
            pass(&ctx, &mut cover, None, |root| {
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
        pass(&ctx, &mut cover, None, |_| {});
        assert!(!cover.covers(Vec2::new(800.0, 8.0)));
    }

    #[test]
    fn resize_handles_reach_past_the_edge() {
        let ctx = egui::Context::default();
        let mut cover = UiCover::default();
        for _ in 0..2 {
            pass(&ctx, &mut cover, None, panels);
        }
        // Rechter Rand der linken Tafel: Dort sitzt der Griff zum Größerziehen.
        let edge = (0..1600)
            .map(|x| x as f32 + 0.5)
            .find(|x| !cover.covers(Vec2::new(*x, 400.0)))
            .expect("freie Karte");
        let beside = Vec2::new(edge + 1.5, 400.0);
        assert!(!cover.covers(beside));
        let mut cursor = egui::CursorIcon::Default;
        for _ in 0..3 {
            cursor = pass(
                &ctx,
                &mut cover,
                Some(egui::pos2(beside.x, beside.y)),
                panels,
            );
        }
        assert!(is_resize(cursor), "Zeigersymbol {cursor:?}");
        assert!(
            cover.owns(beside, cursor),
            "der Griff gehört der Oberfläche"
        );
        // Dasselbe Symbol weit weg von jeder Tafel (der Zeiger ist inzwischen woanders)
        // zählt nicht.
        assert!(!cover.owns(Vec2::new(800.0, 400.0), cursor));
        // Mitten auf der Karte verlangt die Oberfläche kein besonderes Symbol.
        let cursor = pass(&ctx, &mut cover, Some(egui::pos2(800.0, 400.0)), panels);
        assert!(!is_resize(cursor));
        assert!(!cover.owns(Vec2::new(800.0, 400.0), cursor));
    }

    fn frame(owner: &mut PointerOwner, buttons: &mut ButtonInput<MouseButton>, over_ui: bool) {
        owner.update(over_ui, buttons);
    }

    #[test]
    fn a_click_belongs_to_whoever_lies_under_it() {
        let mut owner = PointerOwner::default();
        let mut buttons = ButtonInput::<MouseButton>::default();
        // Über der Oberfläche gedrückt: Die Welt bekommt nichts davon mit.
        buttons.press(MouseButton::Left);
        frame(&mut owner, &mut buttons, true);
        assert!(owner.taken_by_ui(MouseButton::Left));
        buttons.clear();
        // Der Zug verlässt die Oberfläche und gehört ihr weiter.
        frame(&mut owner, &mut buttons, false);
        assert!(owner.taken_by_ui(MouseButton::Left));
        buttons.release(MouseButton::Left);
        frame(&mut owner, &mut buttons, false);
        assert!(!owner.taken_by_ui(MouseButton::Left));
        buttons.clear();

        // Über der Welt gedrückt: Der Zug bleibt bei der Welt, auch über einer Tafel.
        buttons.press(MouseButton::Right);
        frame(&mut owner, &mut buttons, false);
        assert!(!owner.taken_by_ui(MouseButton::Right));
        buttons.clear();
        frame(&mut owner, &mut buttons, true);
        assert!(!owner.taken_by_ui(MouseButton::Right));
        assert!(owner.over_ui());
        buttons.release(MouseButton::Right);
        buttons.clear();

        // Drücken und Loslassen im selben Frame über der Oberfläche.
        buttons.press(MouseButton::Left);
        buttons.release(MouseButton::Left);
        frame(&mut owner, &mut buttons, true);
        assert!(owner.taken_by_ui(MouseButton::Left));
        buttons.clear();
        frame(&mut owner, &mut buttons, true);
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
        world.resource_mut::<PointerOwner>().update(true, &buttons);
        assert_eq!(read(&mut world), (false, false, false, 0.0));
        // Ein Zug mit der mittleren Taste, über der Welt begonnen.
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Middle);
        world.resource_mut::<PointerOwner>().update(false, &buttons);
        world.insert_resource(buttons);
        world.resource_mut::<AccumulatedMouseScroll>().unit = MouseScrollUnit::Pixel;
        world.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, -80.0);
        assert_eq!(read(&mut world), (false, false, true, -2.0));
    }
}
