//! The in-game overlay: a Minecraft hotbar that doubles as the menu.
//!
//! Every pixel of it comes out of the resource pack — the hotbar and its
//! selection frame from `gui/sprites/hud/`, the item icons from `textures/item/`,
//! the letters from `font/ascii.png` — decoded with the project's own PNG
//! decoder, and composited over the finished frame by hand. No text rendering
//! library, no widget toolkit, no GPU: the overlay is just alpha blending into
//! the same `u32` buffer the raytracer wrote.
//!
//! Each slot is an action. The clock and the compass are *held* rather than
//! pressed, so the hour sweeps while the key is down; everything else fires once.

use crate::output::Framebuffer;
use crate::pack::Pack;
use crate::png::Image;

const GUI_DIR: &str = "assets/minecraft/textures/gui/sprites/hud/";
const ITEM_DIR: &str = "assets/minecraft/textures/item/";
const FONT: &str = "assets/minecraft/textures/font/ascii.png";

/// How many animation frames are kept for the clock and the compass. The pack
/// ships 64 and 32; every fourth one is more than the eye can follow at this
/// size, and decoding all of them costs startup time for nothing.
const CLOCK_FRAMES: usize = 16;
const COMPASS_FRAMES: usize = 8;

/// The hotbar has nine slots; these are the ones with something in them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Diamond pickaxe: swap between orbiting and flying.
    ToggleCamera,
    /// Clock: hold to run the hour forward.
    TimeForward,
    /// Compass: hold to run it back.
    TimeBack,
    /// Painting: save a PNG of the frame.
    Screenshot,
    /// Wheat seeds: a new terrain seed, like typing one into a new world.
    Reseed,
    /// Spyglass: step the resolution scale, 1 to 4 and back to 1.
    Quality,
    /// Barrier: leave.
    Quit,
}

impl Action {
    /// Whether holding the key keeps the action firing. The two time items
    /// sweep; the rest would be nonsense repeated sixty times a second.
    pub fn repeats(self) -> bool {
        matches!(self, Action::TimeForward | Action::TimeBack)
    }
}

/// What a slot animates with, if anything.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Animation {
    Static,
    /// The dial follows the time of day, as it does in the game.
    Clock,
    /// The needle follows the camera's heading.
    Compass,
}

struct Slot {
    frames: Vec<Image>,
    animation: Animation,
    action: Action,
    /// Owned rather than static: the spyglass rewrites its own label with the
    /// resolution it is currently on.
    label: String,
}

pub struct Hud {
    hotbar: Image,
    selection: Image,
    slots: Vec<Slot>,
    font: Font,
    selected: usize,
    /// A line under the hotbar, and how many frames it still has to live.
    message: Option<(String, u32)>,
}

impl Hud {
    pub fn load(pack: &Pack) -> Result<Hud, String> {
        let item = |name: &str| pack.decode_png(&format!("{ITEM_DIR}{name}.png"));

        // The pickaxe is an animated strip in this pack; only the first frame is
        // the icon.
        let pickaxe = first_frame(item("diamond_pickaxe")?);

        let clock = frame_series(pack, "clock", 64, CLOCK_FRAMES)?;
        let compass = frame_series(pack, "compass", 32, COMPASS_FRAMES)?;

        let slots = vec![
            Slot {
                frames: vec![pickaxe],
                animation: Animation::Static,
                action: Action::ToggleCamera,
                label: "Camara: orbita / vuelo".into(),
            },
            Slot {
                frames: clock,
                animation: Animation::Clock,
                action: Action::TimeForward,
                label: "Reloj: manten para adelantar la hora".into(),
            },
            Slot {
                frames: compass,
                animation: Animation::Compass,
                action: Action::TimeBack,
                label: "Brujula: manten para regresar la hora".into(),
            },
            Slot {
                frames: vec![item("painting")?],
                animation: Animation::Static,
                action: Action::Screenshot,
                label: "Cuadro: guardar una captura PNG".into(),
            },
            Slot {
                frames: vec![item("wheat_seeds")?],
                animation: Animation::Static,
                action: Action::Reseed,
                label: "Semillas: generar otro terreno".into(),
            },
            Slot {
                frames: vec![item("spyglass")?],
                animation: Animation::Static,
                action: Action::Quality,
                label: quality_label(1),
            },
            // The barrier stays last: it is the one slot you do not want to land
            // on by accident while stepping along the bar.
            Slot {
                frames: vec![item("barrier")?],
                animation: Animation::Static,
                action: Action::Quit,
                label: "Barrera: salir".into(),
            },
        ];

        Ok(Hud {
            hotbar: pack.decode_png(&format!("{GUI_DIR}hotbar.png"))?,
            selection: pack.decode_png(&format!("{GUI_DIR}hotbar_selection.png"))?,
            slots,
            font: Font::load(pack)?,
            selected: 0,
            message: None,
        })
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    #[cfg(test)]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Out-of-range picks are ignored rather than clamped: pressing `9` on a
    /// six-slot bar should do nothing, not jump to the last item.
    pub fn select(&mut self, slot: usize) {
        if slot < self.slots.len() {
            self.selected = slot;
        }
    }

    pub fn action(&self) -> Action {
        self.slots[self.selected].action
    }

    pub fn label(&self) -> &str {
        &self.slots[self.selected].label
    }

    /// Keep the spyglass showing which resolution it is on. The label is the
    /// only readout: the scale is not visible in the picture until it changes.
    pub fn set_quality(&mut self, quality: usize) {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|s| s.action == Action::Quality)
        {
            slot.label = quality_label(quality);
        }
    }

    /// Show a line for about a second and a half.
    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), 90));
    }

    /// Composite the overlay onto a finished frame.
    ///
    /// `time_of_day` drives the clock dial and `yaw` the compass needle, so both
    /// items read like the real ones instead of being still pictures.
    pub fn draw(&mut self, frame: &mut Framebuffer, time_of_day: f32, yaw: f32) {
        if frame.width == 0 || frame.height == 0 || self.hotbar.width == 0 {
            return;
        }
        // The pack's hotbar is drawn at twice the vanilla 182x22, so measure the
        // slot pitch from the sprite instead of hard-coding it: a pack at a
        // different resolution still lands on its own slots.
        let unit = self.hotbar.width as f32 / 182.0;
        // Continuous, so the bar keeps the same share of the window instead of
        // jumping a whole step when it is resized.
        let scale = (frame.width as f32 / 600.0).clamp(1.0, 2.0);
        let px = |v: f32| (v * unit * scale).round() as i32;

        let bar_w = px(182.0);
        let bar_h = px(22.0);
        let bar_x = (frame.width as i32 - bar_w) / 2;
        let bar_y = frame.height as i32 - bar_h - px(4.0);
        blit(frame, &self.hotbar, bar_x, bar_y, bar_w, bar_h, 1.0);

        // Slot centres: one pixel of border, then nine twenty-pixel cells.
        let slot_centre = |i: usize| bar_x + px(1.0 + 20.0 * i as f32 + 10.0);
        let centre_y = bar_y + bar_h / 2;

        // The selection frame overhangs the bar by a pixel on every side, which
        // is why it is 24x23 against the bar's 20x22 cells. It goes on *before*
        // the icons: this pack draws it as a filled cell rather than an outline,
        // and over the icons it would hide whatever is selected.
        let sel_w = px(24.0);
        let sel_h = px(23.0);
        blit(
            frame,
            &self.selection,
            slot_centre(self.selected) - sel_w / 2,
            centre_y - sel_h / 2,
            sel_w,
            sel_h,
            1.0,
        );

        for (i, slot) in self.slots.iter().enumerate() {
            let icon = slot.frame(time_of_day, yaw);
            let size = px(16.0);
            blit(
                frame,
                icon,
                slot_centre(i) - size / 2,
                centre_y - size / 2,
                size,
                size,
                1.0,
            );
        }

        // The label of whatever is selected, above the bar, the way the game
        // names the item you just scrolled to.
        let text_scale = scale.max(1.0);
        let label = self.slots[self.selected].label.clone();
        let label_w = self.font.width(&label, text_scale);
        let label_y = bar_y - self.font.height(text_scale) - px(3.0);
        self.font.draw(
            frame,
            (frame.width as i32 - label_w) / 2,
            label_y,
            &label,
            text_scale,
        );

        if let Some((text, life)) = &mut self.message {
            let w = self.font.width(text, text_scale);
            let y = label_y - self.font.height(text_scale) - px(2.0);
            self.font
                .draw(frame, (frame.width as i32 - w) / 2, y, text, text_scale);
            *life -= 1;
            if *life == 0 {
                self.message = None;
            }
        }
    }
}

impl Slot {
    fn frame(&self, time_of_day: f32, yaw: f32) -> &Image {
        let n = self.frames.len();
        let index = match self.animation {
            Animation::Static => 0,
            // The pack's frame 0 is noon, and our clock has noon at 0.25.
            Animation::Clock => ((time_of_day - 0.25).rem_euclid(1.0) * n as f32) as usize,
            Animation::Compass => {
                ((yaw / std::f32::consts::TAU).rem_euclid(1.0) * n as f32) as usize
            }
        };
        &self.frames[index.min(n - 1)]
    }
}

/// One step of the resolution cycle, in words: scale 1 traces every pixel, 2
/// traces one in four, and so on.
fn quality_label(quality: usize) -> String {
    match quality {
        1 => "Telescopio: resolucion completa".to_string(),
        q => format!("Telescopio: resolucion 1/{q}"),
    }
}

/// Load `name_00.png`, `name_01.png`, … keeping `wanted` of the `total` frames,
/// evenly spaced.
fn frame_series(pack: &Pack, name: &str, total: usize, wanted: usize) -> Result<Vec<Image>, String> {
    let step = (total / wanted).max(1);
    (0..wanted)
        .map(|i| pack.decode_png(&format!("{ITEM_DIR}{name}_{:02}.png", i * step)))
        .collect()
}

/// Some item textures are vertical animation strips. The icon is the top frame.
fn first_frame(image: Image) -> Image {
    if image.height <= image.width || image.height % image.width != 0 {
        return image;
    }
    let side = image.width;
    Image {
        width: side,
        height: side,
        rgba: image.rgba[..side * side * 4].to_vec(),
    }
}

/// Nearest-neighbour blit with alpha, straight into the 0RGB buffer. `tint`
/// scales the colour (not the alpha), which is all the drop shadow needs.
fn blit(frame: &mut Framebuffer, src: &Image, x: i32, y: i32, w: i32, h: i32, tint: f32) {
    blit_rect(frame, src, (0, 0, src.width, src.height), x, y, w, h, tint);
}

#[allow(clippy::too_many_arguments)]
fn blit_rect(
    frame: &mut Framebuffer,
    src: &Image,
    rect: (usize, usize, usize, usize),
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    tint: f32,
) {
    if w <= 0 || h <= 0 || rect.2 == 0 || rect.3 == 0 {
        return;
    }
    let (fw, fh) = (frame.width as i32, frame.height as i32);
    for dy in y.max(0)..(y + h).min(fh) {
        let sy = rect.1 + (((dy - y) as usize * rect.3) / h as usize).min(rect.3 - 1);
        for dx in x.max(0)..(x + w).min(fw) {
            let sx = rect.0 + (((dx - x) as usize * rect.2) / w as usize).min(rect.2 - 1);
            let i = (sy * src.width + sx) * 4;
            let alpha = src.rgba[i + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let dst = &mut frame.pixels[(dy * fw + dx) as usize];
            *dst = blend(
                *dst,
                [
                    (src.rgba[i] as f32 * tint) as u8,
                    (src.rgba[i + 1] as f32 * tint) as u8,
                    (src.rgba[i + 2] as f32 * tint) as u8,
                ],
                alpha,
            );
        }
    }
}

fn blend(dst: u32, src: [u8; 3], alpha: u32) -> u32 {
    if alpha >= 255 {
        return ((src[0] as u32) << 16) | ((src[1] as u32) << 8) | src[2] as u32;
    }
    let mix = |s: u8, d: u32| (s as u32 * alpha + d * (255 - alpha)) / 255;
    let r = mix(src[0], (dst >> 16) & 0xff);
    let g = mix(src[1], (dst >> 8) & 0xff);
    let b = mix(src[2], dst & 0xff);
    (r << 16) | (g << 8) | b
}

/// The game's bitmap font: a 16x16 grid of glyphs indexed by code point, with
/// the drawn width of each one measured from its own pixels — the sheet is
/// monospaced but the font is not.
pub struct Font {
    sheet: Image,
    cell: usize,
    widths: [u8; 256],
}

impl Font {
    fn load(pack: &Pack) -> Result<Font, String> {
        let sheet = pack.decode_png(FONT)?;
        let cell = sheet.width / 16;
        let mut widths = [0u8; 256];
        for (code, width) in widths.iter_mut().enumerate() {
            *width = glyph_width(&sheet, cell, code as u8) as u8;
        }
        // A space draws nothing, so it has to be given a width of its own.
        widths[b' ' as usize] = (cell / 4).max(1) as u8;
        Ok(Font {
            sheet,
            cell,
            widths,
        })
    }

    /// One vanilla pixel, scaled. The sheet here is twice vanilla size, so a
    /// glyph cell is 16 sheet pixels for an 8-pixel character.
    fn pixel(&self, scale: f32) -> i32 {
        ((self.cell as f32 / 8.0) * scale).round().max(1.0) as i32
    }

    pub fn height(&self, scale: f32) -> i32 {
        self.pixel(scale) * 8
    }

    pub fn width(&self, text: &str, scale: f32) -> i32 {
        let unit = self.pixel(scale) as f32 / (self.cell as f32 / 8.0);
        let sheet_width: usize = text
            .bytes()
            .map(|b| self.widths[b as usize] as usize + self.cell / 16)
            .sum();
        (sheet_width as f32 * unit).round() as i32
    }

    /// Draw with the game's one-pixel drop shadow, which is what keeps white
    /// text readable over a bright sky.
    pub fn draw(&self, frame: &mut Framebuffer, x: i32, y: i32, text: &str, scale: f32) {
        let shadow = self.pixel(scale);
        self.draw_plain(frame, x + shadow, y + shadow, text, scale, 0.25);
        self.draw_plain(frame, x, y, text, scale, 1.0);
    }

    fn draw_plain(&self, frame: &mut Framebuffer, x: i32, y: i32, text: &str, scale: f32, tint: f32) {
        let unit = self.pixel(scale);
        let cell = self.cell;
        let mut pen = x;
        for byte in text.bytes() {
            let glyph = self.widths[byte as usize] as usize;
            let w = ((glyph * unit as usize * 8) / cell) as i32;
            if glyph > 0 && byte != b' ' {
                let (col, row) = ((byte as usize % 16) * cell, (byte as usize / 16) * cell);
                blit_rect(
                    frame,
                    &self.sheet,
                    (col, row, glyph, cell),
                    pen,
                    y,
                    w.max(1),
                    unit * 8,
                    tint,
                );
            }
            pen += w + ((unit as usize * 8) / 8) as i32;
        }
    }
}

/// The drawn width of a glyph: the last column that has any ink in it.
fn glyph_width(sheet: &Image, cell: usize, code: u8) -> usize {
    let (col, row) = ((code as usize % 16) * cell, (code as usize / 16) * cell);
    let mut width = 0;
    for y in 0..cell {
        for x in 0..cell {
            let i = ((row + y) * sheet.width + col + x) * 4;
            if sheet.rgba[i + 3] > 0 {
                width = width.max(x + 1);
            }
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The pack is not committed; these tests skip when it is missing.
    fn hud() -> Option<Hud> {
        if !Path::new("texturepack").is_dir() {
            return None;
        }
        Some(Hud::load(&Pack::open(None).ok()?).expect("the hud should load from the pack"))
    }

    #[test]
    fn every_slot_has_an_icon_and_an_action() {
        let Some(hud) = hud() else { return };
        assert_eq!(hud.slot_count(), 7);
        let mut actions = Vec::new();
        for (i, slot) in hud.slots.iter().enumerate() {
            assert!(!slot.frames.is_empty(), "slot {i} has no icon");
            assert!(!slot.label.is_empty());
            actions.push(slot.action);
        }
        assert_eq!(
            actions,
            vec![
                Action::ToggleCamera,
                Action::TimeForward,
                Action::TimeBack,
                Action::Screenshot,
                Action::Reseed,
                Action::Quality,
                Action::Quit,
            ]
        );
        // Only the two time items keep firing while the key is held.
        assert!(Action::TimeForward.repeats() && Action::TimeBack.repeats());
        assert!(!Action::Quit.repeats() && !Action::Screenshot.repeats());
    }

    #[test]
    fn the_spyglass_reports_the_resolution_it_is_on() {
        let Some(mut hud) = hud() else { return };
        hud.select(5);
        assert!(hud.label().contains("completa"), "{}", hud.label());
        hud.set_quality(3);
        assert!(hud.label().contains("1/3"), "{}", hud.label());
        // The barrier is last, so stepping along the bar never lands on it first.
        assert_eq!(hud.slots.last().map(|s| s.action), Some(Action::Quit));
    }

    #[test]
    fn selecting_out_of_range_keeps_the_current_slot() {
        let Some(mut hud) = hud() else { return };
        hud.select(3);
        assert_eq!(hud.selected(), 3);
        hud.select(50);
        assert_eq!(hud.selected(), 3, "an empty slot should not be selectable");
    }

    #[test]
    fn the_clock_and_the_compass_follow_the_time_and_the_heading() {
        let Some(hud) = hud() else { return };
        let clock = &hud.slots[1];
        let noon = clock.frame(0.25, 0.0) as *const Image;
        let midnight = clock.frame(0.75, 0.0) as *const Image;
        assert_ne!(noon, midnight, "the clock dial should follow the hour");
        assert_eq!(noon, clock.frame(0.25, 3.0) as *const Image, "yaw is not the clock's business");

        let compass = &hud.slots[2];
        let north = compass.frame(0.5, 0.0) as *const Image;
        let south = compass.frame(0.5, std::f32::consts::PI) as *const Image;
        assert_ne!(north, south, "the needle should follow the camera");
    }

    #[test]
    fn the_overlay_only_touches_the_bottom_of_the_frame() {
        let Some(mut hud) = hud() else { return };
        let mut frame = Framebuffer::new(640, 480);
        frame.pixels.fill(0x00336699);
        hud.draw(&mut frame, 0.3, 0.0);

        let untouched = frame.pixels[..640 * 300].iter().all(|&p| p == 0x00336699);
        assert!(untouched, "the hud reached into the top of the frame");
        let changed = frame.pixels[640 * 400..].iter().any(|&p| p != 0x00336699);
        assert!(changed, "the hud drew nothing");
    }

    #[test]
    fn a_message_fades_by_itself() {
        let Some(mut hud) = hud() else { return };
        let mut frame = Framebuffer::new(400, 300);
        hud.say("semilla 42");
        assert!(hud.message.is_some());
        for _ in 0..90 {
            hud.draw(&mut frame, 0.3, 0.0);
        }
        assert!(hud.message.is_none(), "the message should expire on its own");
    }

    #[test]
    fn the_font_measures_each_glyph_and_not_the_cell() {
        let Some(hud) = hud() else { return };
        let font = &hud.font;
        assert!(font.widths[b'i' as usize] < font.widths[b'W' as usize]);
        assert!(font.widths[b' ' as usize] > 0, "a space needs a width");
        assert!(font.width("Salir", 1.0) > font.width("S", 1.0));
        assert_eq!(font.width("", 1.0), 0);
    }

    #[test]
    fn drawing_text_leaves_ink() {
        let Some(hud) = hud() else { return };
        let mut frame = Framebuffer::new(320, 64);
        hud.font.draw(&mut frame, 4, 4, "Salir", 1.0);
        assert!(frame.pixels.iter().any(|&p| p != 0), "no text was drawn");
    }

    #[test]
    fn an_animation_strip_yields_a_square_icon() {
        let strip = Image {
            width: 4,
            height: 16,
            rgba: vec![7; 4 * 16 * 4],
        };
        let icon = first_frame(strip);
        assert_eq!((icon.width, icon.height), (4, 4));
        assert_eq!(icon.rgba.len(), 4 * 4 * 4);
    }
}


