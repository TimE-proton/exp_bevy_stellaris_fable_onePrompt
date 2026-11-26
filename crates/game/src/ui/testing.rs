//! Hilfen für Tests der Oberfläche: Durchläufe ohne Fenster und Grafikkarte, auch mit
//! Mausklicks, und ein Blick auf das, was dabei gezeichnet würde.

use super::panels;
use bevy_egui::egui::{self, epaint};

/// Was ein Durchlauf der Oberfläche gezeichnet hat.
#[derive(Default)]
pub struct Painted {
    /// Jeder gezeichnete Text mit dem Rechteck, in dem er zu sehen ist.
    pub texts: Vec<(String, egui::Rect)>,
    /// Gefüllte Rechtecke: sichtbarer Teil und Füllfarbe.
    pub rects: Vec<(egui::Rect, egui::Color32)>,
}

impl Painted {
    /// Wo der Text `needle` steht: zuerst ein Text, der genau so lautet, sonst einer, der
    /// ihn enthält.
    pub fn text(&self, needle: &str) -> Option<egui::Rect> {
        self.texts
            .iter()
            .find(|(text, _)| text == needle)
            .or_else(|| self.texts.iter().find(|(text, _)| text.contains(needle)))
            .map(|(_, rect)| *rect)
    }

    /// Alle Stellen, an denen genau der Text `needle` steht.
    pub fn all(&self, needle: &str) -> Vec<egui::Rect> {
        self.texts
            .iter()
            .filter(|(text, _)| text == needle)
            .map(|(_, rect)| *rect)
            .collect()
    }

    fn collect(&mut self, shape: &epaint::Shape, clip: egui::Rect) {
        match shape {
            epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    self.collect(shape, clip);
                }
            }
            epaint::Shape::Text(text) => {
                let visible = text.visual_bounding_rect().intersect(clip);
                if visible.is_positive() {
                    self.texts.push((text.galley.text().to_owned(), visible));
                }
            }
            epaint::Shape::Rect(rect) => {
                let visible = rect.rect.intersect(clip);
                // Weichgezeichnete Rechtecke sind Schatten, keine Flächen.
                if visible.is_positive() && rect.fill.a() > 0 && rect.blur_width == 0.0 {
                    self.rects.push((visible, rect.fill));
                }
            }
            _ => {}
        }
    }
}

/// Eine Oberfläche ohne Fenster. Der Kontext bleibt über die Durchläufe erhalten, wie im
/// Spiel: Fenster behalten ihren Platz, Klicks bestehen aus Drücken und Loslassen.
pub struct Screen {
    pub ctx: egui::Context,
    size: egui::Vec2,
}

impl Screen {
    pub fn new(width: f32, height: f32) -> Self {
        let ctx = egui::Context::default();
        panels::apply_style(&ctx);
        Self {
            ctx,
            size: egui::vec2(width, height),
        }
    }

    /// Ein Durchlauf mit den gegebenen Eingaben.
    pub fn run(
        &mut self,
        events: Vec<egui::Event>,
        mut draw: impl FnMut(&mut egui::Ui),
    ) -> Painted {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            events,
            ..Default::default()
        };
        let mut output = self.ctx.run_ui(input, |ui| draw(ui));
        // Ohne Renderer nimmt niemand die Schriftatlas-Textur ab; ausdrücklich verwerfen.
        output.textures_delta.clear();
        let mut painted = Painted::default();
        for clipped in &output.shapes {
            painted.collect(&clipped.shape, clipped.clip_rect);
        }
        painted
    }

    /// Ein Durchlauf ohne Eingabe.
    pub fn draw(&mut self, draw: impl FnMut(&mut egui::Ui)) -> Painted {
        self.run(Vec::new(), draw)
    }

    /// Mehrere Durchläufe ohne Eingabe, bis Fenster ihre Größe gefunden haben; liefert den
    /// letzten.
    pub fn settle(&mut self, mut draw: impl FnMut(&mut egui::Ui)) -> Painted {
        for _ in 0..3 {
            self.draw(&mut draw);
        }
        self.draw(&mut draw)
    }

    /// Bewegt den Mauszeiger an eine Stelle und lässt ihn dort einen Durchlauf lang ruhen.
    pub fn hover(&mut self, at: egui::Pos2, mut draw: impl FnMut(&mut egui::Ui)) -> Painted {
        self.run(vec![egui::Event::PointerMoved(at)], &mut draw);
        self.draw(&mut draw)
    }

    /// Ein Linksklick: Zeiger hinbewegen, drücken, loslassen, jeweils ein Durchlauf.
    /// `draw` läuft in jedem davon; was der Klick auslöst, fällt beim Loslassen an.
    pub fn click(&mut self, at: egui::Pos2, mut draw: impl FnMut(&mut egui::Ui)) -> Painted {
        let button = |pressed: bool| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        self.run(vec![egui::Event::PointerMoved(at)], &mut draw);
        self.run(vec![button(true)], &mut draw);
        self.run(vec![button(false)], &mut draw)
    }
}

/// Überschneiden sich zwei der Rechtecke? Liefert das erste Paar.
pub fn overlap(rects: &[(String, egui::Rect)]) -> Option<(String, String)> {
    for (index, (name, rect)) in rects.iter().enumerate() {
        for (other_name, other) in &rects[index + 1..] {
            if rect.shrink(0.5).intersects(other.shrink(0.5)) {
                return Some((name.clone(), other_name.clone()));
            }
        }
    }
    None
}
