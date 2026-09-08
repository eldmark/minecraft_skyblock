//! The title screen: a background, the credits, and two buttons.
//!
//! It is the same machinery as the in-game overlay — the pack's bitmap font and
//! the hand-written alpha blits — over a photograph decoded with the project's
//! own PNG decoder. The window is already open while this is on screen, so the
//! scene is only built once the player presses Jugar.

use std::path::Path;

use crate::ui::hud::{blit, dim, fill_rect, Font};
use crate::render::output::Framebuffer;
use crate::assets::pack::Pack;
use crate::codec::png::{self, Image};

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
    /// Which button the pointer is over, if any.
    hover: Option<usize>,
    /// A button being pressed, and how many frames the press still shows for.
    press: Option<(usize, u32)>,
    /// What the press will do once its animation has played.
    pending: Option<Choice>,
}

/// How long the press animation lasts, in frames. Long enough to see, short
/// enough that it never feels like lag.
const PRESS_FRAMES: u32 = 7;

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
            hover: None,
            press: None,
            pending: None,
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
        // Always in the same place. They used to sit higher until the rules
        // opened and then jump to the bottom to get out of the panel's way, and
        // a button that moves out from under the pointer is a button that gets
        // missed.
        let first = frame.height as i32 - gap - h - (12.0 * scale) as i32;
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

    /// Track the pointer so the button under it can light up.
    pub fn hover(&mut self, frame: &Framebuffer, x: f32, y: f32) {
        self.hover = self
            .buttons(frame)
            .iter()
            .position(|button| button.contains(x, y));
    }

    /// A click on the title screen. The action does not happen here: the button
    /// is pressed, and `tick` hands the choice back once the press has been seen.
    pub fn click(&mut self, frame: &Framebuffer, x: f32, y: f32) {
        let buttons = self.buttons(frame);
        // Jugar works from the rules screen too: the button is right there and
        // doing nothing when it is pressed would just look broken.
        if buttons[0].contains(x, y) {
            self.press = Some((0, PRESS_FRAMES));
            self.pending = Some(Choice::Play);
        } else if buttons[1].contains(x, y) {
            self.press = Some((1, PRESS_FRAMES));
            self.pending = None;
            self.rules = !self.rules;
        }
    }

    /// Advance the animations by one frame, and hand back a choice whose press
    /// has finished playing.
    pub fn tick(&mut self) -> Option<Choice> {
        match &mut self.press {
            Some((_, frames)) if *frames > 1 => {
                *frames -= 1;
                None
            }
            Some(_) => {
                self.press = None;
                self.pending.take()
            }
            None => self.pending.take(),
        }
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


        for (index, button) in self.buttons(frame).iter().enumerate() {
            self.draw_button(frame, index, button, scale);
        }
    }

    /// The screen shown while the scene is being built, on the same background:
    /// a line of text and a ring of squares turning under it.
    ///
    /// Building the world takes about a second — terrain, structures, the two
    /// neighbours, and 92 textures decoded with our own PNG decoder — and a
    /// window frozen for a second reads as a window that has crashed.
    pub fn draw_loading(&self, frame: &mut Framebuffer, tick: usize) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        self.background(frame);
        dim(frame, 0.35);

        let scale = self.scale(frame);
        let text = "Cargando la escena...";
        let y = (frame.height as f32 * 0.52) as i32;
        self.centred(frame, text, y, scale);
        self.spinner(
            frame,
            (frame.width / 2) as i32,
            y + (self.font.height(scale) as f32 * 3.2) as i32,
            scale,
            tick,
        );
    }

    /// Eight squares on a circle, each fading behind the one in front: the
    /// classic spinner, drawn with the same rectangles as everything else.
    fn spinner(&self, frame: &mut Framebuffer, cx: i32, cy: i32, scale: f32, tick: usize) {
        const DOTS: usize = 8;
        let radius = 22.0 * scale;
        let size = (7.0 * scale).max(4.0) as i32;
        // One step every three frames: fast enough to read as motion, slow enough
        // not to strobe.
        let head = (tick / 3) % DOTS;
        for i in 0..DOTS {
            let angle = i as f32 / DOTS as f32 * std::f32::consts::TAU;
            let x = cx + (angle.sin() * radius) as i32 - size / 2;
            let y = cy - (angle.cos() * radius) as i32 - size / 2;
            // Distance behind the head, so the tail trails off.
            let behind = (i + DOTS - head) % DOTS;
            // The head is solid and the tail fades, but never to nothing: a dot
            // that disappears entirely reads as a gap, not as motion.
            let alpha = 245 - behind as u32 * 22;
            fill_rect(frame, x, y, size, size, 0x00E8_E8F0, alpha);
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

    fn draw_button(&self, frame: &mut Framebuffer, index: usize, button: &Button, scale: f32) {
        let hovered = self.hover == Some(index);
        let pressed = matches!(self.press, Some((i, _)) if i == index);

        // Pressed: the face sinks in by a pixel and darkens, so the click is felt
        // and not only heard by the code. Hovered: it lifts and brightens.
        let unit = scale.max(1.0) as i32;
        let sink = if pressed { unit } else { 0 };
        let (face, border) = match (pressed, hovered) {
            (true, _) => (0x0053_5359, 0x000A_0A0C),
            (_, true) => (0x008B_8B95, 0x0014_1418),
            _ => (0x006A_6A72, 0x0010_1013),
        };

        // The game's buttons are a light face over a darker edge; two rectangles
        // are enough to read as one at this size.
        fill_rect(frame, button.x, button.y, button.w, button.h, border, 220);
        let inset = (2.0 * scale) as i32;
        fill_rect(
            frame,
            button.x + inset,
            button.y + inset + sink,
            button.w - inset * 2,
            button.h - inset * 2 - sink,
            face,
            235,
        );
        let text_w = self.font.width(button.label, scale);
        self.font.draw(
            frame,
            button.x + (button.w - text_w) / 2,
            button.y + (button.h - self.font.height(scale)) / 2 + sink,
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

        splash.click(&frame, px, py);
        assert_eq!(splash.pending, Some(Choice::Play));

        let mut splash = splash;
        splash.pending = None;
        splash.click(&frame, rx, ry);
        assert!(splash.rules, "the second button should open the rules");
        assert_eq!(splash.pending, None);
        // Jugar still starts the game from the rules screen, from the same place.
        splash.click(&frame, px, py);
        assert_eq!(splash.pending, Some(Choice::Play));
        splash.pending = None;
        splash.click(&frame, rx, ry);
        assert!(!splash.rules, "the same button should close them again");

        // Nowhere near a button.
        splash.click(&frame, 5.0, 5.0);
        assert_eq!(splash.pending, None);
    }

    #[test]
    fn the_buttons_do_not_move_when_the_rules_open() {
        let Some(mut splash) = splash() else { return };
        let frame = Framebuffer::new(900, 620);
        let before: Vec<(i32, i32)> = splash.buttons(&frame).iter().map(|b| (b.x, b.y)).collect();
        splash.rules = true;
        let after: Vec<(i32, i32)> = splash.buttons(&frame).iter().map(|b| (b.x, b.y)).collect();
        assert_eq!(before, after, "a button moved out from under the pointer");
    }

    #[test]
    fn a_press_is_shown_before_it_is_acted_on() {
        let Some(mut splash) = splash() else { return };
        let frame = Framebuffer::new(900, 620);
        let button = &splash.buttons(&frame)[0];
        let (x, y) = ((button.x + button.w / 2) as f32, (button.y + button.h / 2) as f32);

        splash.click(&frame, x, y);
        assert!(splash.press.is_some(), "the button should look pressed");
        // The choice is held back until the animation has played.
        for _ in 0..PRESS_FRAMES - 1 {
            assert_eq!(splash.tick(), None);
        }
        assert_eq!(splash.tick(), Some(Choice::Play));
        assert!(splash.press.is_none());
        assert_eq!(splash.tick(), None, "a choice is only handed over once");
    }

    #[test]
    fn hovering_lights_up_the_button_under_the_pointer() {
        let Some(mut splash) = splash() else { return };
        let frame = Framebuffer::new(900, 620);
        let button = &splash.buttons(&frame)[1];
        splash.hover(&frame, (button.x + 2) as f32, (button.y + 2) as f32);
        assert_eq!(splash.hover, Some(1));
        splash.hover(&frame, 4.0, 4.0);
        assert_eq!(splash.hover, None);
    }

    #[test]
    fn the_loading_screen_draws_a_spinner_that_moves() {
        let Some(splash) = splash() else { return };
        let mut first = Framebuffer::new(400, 300);
        let mut later = Framebuffer::new(400, 300);
        splash.draw_loading(&mut first, 0);
        splash.draw_loading(&mut later, 9);
        assert_ne!(first.pixels, later.pixels, "the spinner is not turning");
        assert!(first.pixels.iter().any(|&p| p != 0), "nothing was drawn");
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


