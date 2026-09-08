//! The title screen: a background, the credits, and two buttons.
//!
//! It is the same machinery as the in-game overlay — the pack's bitmap font and
//! the hand-written alpha blits — over a photograph decoded with the project's
//! own PNG decoder. The window is already open while this is on screen, so the
//! scene is only built once the player presses Jugar.

use std::path::Path;

use crate::hud::{blit, dim, fill_rect, Font};
use crate::output::Framebuffer;
use crate::pack::Pack;
use crate::png::{self, Image};

/// Where the background lives. It is a still of a Minecraft dragon build,
/// converted to PNG once so the program only ever needs its own decoder.
const BACKGROUND: &str = "title/splash.png";

const TITLE: &str = "Minecraft Diorama";
const AUTHOR: &str = "Marco Diaz  24229";
const COURSE: &str = "Graficas por Computadora";

/// What the title screen is waiting for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Play,
    Quit,
}

/// The controls, as they are shown on the rules screen. Kept to plain ASCII:
/// the pack's font sheet has no accented glyphs.
const RULES: &[(&str, &str)] = &[
    ("Mouse", "girar la camara (mouselook, siempre activo)"),
    ("Tab", "soltar o retomar el mouselook"),
    ("W A S D", "moverse; en orbita, acercar y girar"),
    ("Espacio / Shift", "subir y bajar en vuelo libre"),
    ("Ctrl", "moverse mas rapido"),
    ("F", "cambiar entre orbita y vuelo libre"),
    ("Click izq.", "quitar el bloque apuntado"),
    ("Click der.", "poner el bloque elegido"),
    ("E", "abrir el inventario de bloques"),
    ("1 - 8", "elegir ranura de la hotbar"),
    ("Enter", "usar el objeto de la hotbar"),
    ("Q", "correr el ciclo de dia y noche"),
    (", .", "mover la hora a mano"),
    ("R", "regenerar el terreno con otra semilla"),
    ("P", "captura de pantalla"),
    ("H", "esconder la interfaz"),
    ("Esc", "cerrar inventario, soltar mouse, salir"),
];

pub struct Splash {
    background: Image,
    font: Font,
    /// Whether the rules panel is up instead of the buttons.
    rules: bool,
}

/// A button on screen, in pixels.
struct Button {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: &'static str,
}

impl Button {
    fn contains(&self, x: f32, y: f32) -> bool {
        let (x, y) = (x.round() as i32, y.round() as i32);
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

impl Splash {
    pub fn load(pack: &Pack) -> Result<Splash, String> {
        let bytes = std::fs::read(Path::new(BACKGROUND))
            .map_err(|e| format!("{BACKGROUND}: {e}"))?;
        Ok(Splash {
            background: png::decode(&bytes).map_err(|e| format!("{BACKGROUND}: {e}"))?,
            font: Font::load(pack)?,
            rules: false,
        })
    }

    /// Scale of the interface for this window, so text and buttons keep their
    /// share of the frame instead of shrinking into a corner.
    fn scale(&self, frame: &Framebuffer) -> f32 {
        (frame.width as f32 / 700.0).clamp(1.0, 2.5)
    }

    fn buttons(&self, frame: &Framebuffer) -> [Button; 2] {
        let scale = self.scale(frame);
        let w = (200.0 * scale) as i32;
        let h = (34.0 * scale) as i32;
        let x = (frame.width as i32 - w) / 2;
        let gap = (h as f32 * 1.35) as i32;
        // With the rules up the buttons move to the bottom of the screen: over
        // the panel they sat on top of the very list they open.
        let first = if self.rules {
            frame.height as i32 - gap - h - (12.0 * scale) as i32
        } else {
            (frame.height as f32 * 0.60) as i32
        };
        [
            Button {
                x,
                y: first,
                w,
                h,
                label: "Jugar",
            },
            Button {
                x,
                y: first + gap,
                w,
                h,
                label: if self.rules { "Volver" } else { "Reglas" },
            },
        ]
    }

    /// A click on the title screen. `None` means it changed nothing that the
    /// caller has to act on.
    pub fn click(&mut self, frame: &Framebuffer, x: f32, y: f32) -> Option<Choice> {
        let buttons = self.buttons(frame);
        // Jugar works from the rules screen too: the button is right there and
        // doing nothing when it is pressed would just look broken.
        if buttons[0].contains(x, y) {
            return Some(Choice::Play);
        }
        if buttons[1].contains(x, y) {
            self.rules = !self.rules;
        }
        None
    }

    /// Escape backs out of the rules, and quits from the title screen itself.
    pub fn escape(&mut self) -> Option<Choice> {
        if self.rules {
            self.rules = false;
            return None;
        }
        Some(Choice::Quit)
    }

    pub fn draw(&self, frame: &mut Framebuffer) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        self.background(frame);
        let scale = self.scale(frame);

        // Title block, high enough to clear the dragon's head in the picture.
        let mut y = (frame.height as f32 * 0.08) as i32;
        self.centred(frame, TITLE, y, scale * 2.0);
        y += self.font.height(scale * 2.0) + (10.0 * scale) as i32;
        self.centred(frame, AUTHOR, y, scale);
        y += self.font.height(scale) + (4.0 * scale) as i32;
        self.centred(frame, COURSE, y, scale);

        if self.rules {
            self.draw_rules(frame);
        }


        for button in self.buttons(frame) {
            self.draw_button(frame, &button, scale);
        }
    }

    /// The background, scaled to cover the window and darkened a little so white
    /// text stays readable over the sky.
    fn background(&self, frame: &mut Framebuffer) {
        let (fw, fh) = (frame.width as f32, frame.height as f32);
        let (iw, ih) = (self.background.width as f32, self.background.height as f32);
        // Cover, not fit: the picture keeps its aspect and the frame is filled.
        let scale = (fw / iw).max(fh / ih);
        let (w, h) = ((iw * scale) as i32, (ih * scale) as i32);
        blit(
            frame,
            &self.background,
            (frame.width as i32 - w) / 2,
            (frame.height as i32 - h) / 2,
            w,
            h,
            1.0,
        );
        dim(frame, 0.25);
    }

    fn centred(&self, frame: &mut Framebuffer, text: &str, y: i32, scale: f32) {
        let width = self.font.width(text, scale);
        self.font
            .draw(frame, (frame.width as i32 - width) / 2, y, text, scale);
    }

    fn draw_button(&self, frame: &mut Framebuffer, button: &Button, scale: f32) {
        // The game's buttons are a light face over a darker edge; two rectangles
        // are enough to read as one at this size.
        fill_rect(frame, button.x, button.y, button.w, button.h, 0x0010_1013, 220);
        let inset = (2.0 * scale) as i32;
        fill_rect(
            frame,
            button.x + inset,
            button.y + inset,
            button.w - inset * 2,
            button.h - inset * 2,
            0x006A_6A72,
            230,
        );
        let text_w = self.font.width(button.label, scale);
        self.font.draw(
            frame,
            button.x + (button.w - text_w) / 2,
            button.y + (button.h - self.font.height(scale)) / 2,
            button.label,
            scale,
        );
    }

    fn draw_rules(&self, frame: &mut Framebuffer) {
        // A notch smaller than the rest: seventeen lines and two buttons have to
        // share the window.
        let scale = self.scale(frame) * 0.85;
        let column_gap = (26.0 * scale) as i32;
        let line = self.font.height(scale) + (4.0 * scale) as i32;
        let height = line * RULES.len() as i32;
        let pad = (12.0 * scale) as i32;
        // The widest key column decides where the description starts, so the two
        // columns line up whatever the window size.
        let key_w = RULES
            .iter()
            .map(|(key, _)| self.font.width(key, scale))
            .max()
            .unwrap_or(0);
        let text_w = RULES
            .iter()
            .map(|(_, what)| self.font.width(what, scale))
            .max()
            .unwrap_or(0);
        let width = key_w + text_w + column_gap;
        let x = (frame.width as i32 - width) / 2;
        let y = (frame.height as f32 * 0.12) as i32;

        dim(frame, 0.45);
        fill_rect(
            frame,
            x - pad,
            y - pad,
            width + pad * 2,
            height + pad * 2,
            0x0016_161A,
            225,
        );
        for (i, (key, what)) in RULES.iter().enumerate() {
            let row = y + i as i32 * line;
            self.font.draw(frame, x, row, key, scale);
            self.font.draw(frame, x + key_w + column_gap, row, what, scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn splash() -> Option<Splash> {
        if !Path::new(BACKGROUND).is_file() {
            return None;
        }
        Some(Splash::load(&Pack::open(None).ok()?).expect("the title screen should load"))
    }

    #[test]
    fn the_background_decodes_with_our_own_png_decoder() {
        let Some(splash) = splash() else { return };
        assert!(splash.background.width >= 640 && splash.background.height >= 320);
        assert_eq!(
            splash.background.rgba.len(),
            splash.background.width * splash.background.height * 4
        );
    }

    #[test]
    fn the_play_button_starts_the_game_and_the_other_one_toggles_the_rules() {
        let Some(mut splash) = splash() else { return };
        let frame = Framebuffer::new(900, 620);

        let centre = |b: &Button| ((b.x + b.w / 2) as f32, (b.y + b.h / 2) as f32);
        let buttons = splash.buttons(&frame);
        let (px, py) = centre(&buttons[0]);
        let (rx, ry) = centre(&buttons[1]);

        assert_eq!(splash.click(&frame, px, py), Some(Choice::Play));

        assert_eq!(splash.click(&frame, rx, ry), None);
        assert!(splash.rules, "the second button should open the rules");
        // The buttons move once the rules are up, so ask again where they are.
        let buttons = splash.buttons(&frame);
        let (px, py) = centre(&buttons[0]);
        let (rx, ry) = centre(&buttons[1]);
        // Jugar still starts the game from the rules screen.
        assert_eq!(splash.click(&frame, px, py), Some(Choice::Play));
        assert_eq!(splash.click(&frame, rx, ry), None);
        assert!(!splash.rules, "the same button should close them again");

        // Nowhere near a button.
        assert_eq!(splash.click(&frame, 5.0, 5.0), None);
    }

    #[test]
    fn escape_backs_out_of_the_rules_before_it_quits() {
        let Some(mut splash) = splash() else { return };
        splash.rules = true;
        assert_eq!(splash.escape(), None);
        assert!(!splash.rules);
        assert_eq!(splash.escape(), Some(Choice::Quit));
    }

    #[test]
    fn the_title_screen_covers_the_whole_frame() {
        let Some(splash) = splash() else { return };
        let mut frame = Framebuffer::new(640, 480);
        frame.pixels.fill(0x00FF00FF);
        splash.draw(&mut frame);
        assert!(
            frame.pixels.iter().all(|&p| p != 0x00FF00FF),
            "the background did not cover the frame"
        );
    }
}

