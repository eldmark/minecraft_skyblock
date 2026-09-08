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

use crate::blocks::{self, Block};
use crate::output::Framebuffer;
use crate::pack::{Pack, BLOCK_DIR};
use crate::png::Image;

const GUI_DIR: &str = "assets/minecraft/textures/gui/sprites/hud/";
const ITEM_DIR: &str = "assets/minecraft/textures/item/";
const FONT: &str = "assets/minecraft/textures/font/ascii.png";

/// What the inventory offers, and the texture that stands for each block. A few
/// of them are animation strips in this pack, so only the first frame is used.
const INVENTORY: &[(Block, &str)] = &[
    (blocks::GRASS, "grass_block_side"),
    (blocks::DIRT, "dirt"),
    (blocks::STONE, "stone"),
    (blocks::COBBLESTONE, "cobblestone"),
    (blocks::SAND, "sand"),
    (blocks::GRAVEL, "gravel"),
    (blocks::OAK_LOG, "oak_log"),
    (blocks::OAK_PLANKS, "oak_planks"),
    (blocks::OAK_LEAVES, "oak_leaves"),
    (blocks::OAK_SLAB, "oak_planks"),
    (blocks::OAK_FENCE, "oak_planks"),
    (blocks::GLASS, "glass"),
    (blocks::GLOWSTONE, "glowstone"),
    (blocks::QUARTZ, "quartz_block_side"),
    (blocks::STONE_BRICKS, "stone_bricks"),
    (blocks::GOLD_BLOCK, "gold_block"),
    (blocks::IRON_BLOCK, "iron_block"),
    (blocks::DIAMOND_BLOCK, "diamond_block"),
    (blocks::EMERALD_BLOCK, "emerald_block"),
    (blocks::OBSIDIAN, "obsidian"),
    (blocks::NETHERRACK, "netherrack"),
    (blocks::NETHER_BRICKS, "nether_bricks"),
    (blocks::MAGMA, "magma"),
    (blocks::SOUL_SAND, "soul_sand"),
    (blocks::HAY_BLOCK, "hay_block_side"),
    (blocks::PUMPKIN, "pumpkin_side"),
    (blocks::WHITE_WOOL, "white_wool"),
    (blocks::RED_WOOL, "red_wool"),
    (blocks::RED_CONCRETE, "red_concrete"),
    (blocks::BLACK_CONCRETE, "black_concrete"),
    (blocks::WATER, "water_still"),
    (blocks::LAVA, "lava_still"),
];

/// Columns in the inventory grid.
const INVENTORY_COLS: usize = 8;

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
    /// The block slot: left click breaks, right click places.
    Place,
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
    crosshair: Image,
    slots: Vec<Slot>,
    /// Every block the inventory can hand out, with its icon.
    inventory: Vec<(Block, Image)>,
    /// Whether the inventory screen is up.
    pub open: bool,
    /// The block in the hotbar's block slot, placed by a right click.
    held: Option<Block>,
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
            // The barrier stays last of the tools: it is the one slot you do not
            // want to land on by accident while stepping along the bar.
            Slot {
                frames: vec![item("barrier")?],
                animation: Animation::Static,
                action: Action::Quit,
                label: "Barrera: salir".into(),
            },
            // The block slot, in what used to be the first empty cell. It starts
            // out empty and the inventory fills it.
            Slot {
                frames: Vec::new(),
                animation: Animation::Static,
                action: Action::Place,
                label: "Bloque: E para elegir uno".into(),
            },
        ];

        let mut inventory = Vec::with_capacity(INVENTORY.len());
        for (block, texture) in INVENTORY {
            let image = pack.decode_png(&format!("{BLOCK_DIR}{texture}.png"))?;
            inventory.push((*block, first_frame(image)));
        }

        Ok(Hud {
            hotbar: pack.decode_png(&format!("{GUI_DIR}hotbar.png"))?,
            selection: pack.decode_png(&format!("{GUI_DIR}hotbar_selection.png"))?,
            crosshair: pack.decode_png(&format!("{GUI_DIR}crosshair.png"))?,
            slots,
            inventory,
            open: false,
            held: None,
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

    /// The block the block slot is holding, if the inventory has filled it.
    pub fn held(&self) -> Option<Block> {
        self.held
    }

    /// Put a block in the block slot, and select that slot: picking a block from
    /// the inventory is also saying "this is what I am about to place".
    pub fn set_held(&mut self, block: Block, icon: Image) {
        self.held = Some(block);
        if let Some((index, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.action == Action::Place)
        {
            slot.frames = vec![icon];
            slot.label = format!("{}: click izq. quita, der. pone", blocks::name(block));
            self.selected = index;
        }
    }

    /// Open and close the inventory screen.
    pub fn toggle_inventory(&mut self) {
        self.open = !self.open;
    }

    /// Which block the inventory cell under a pixel holds, if any. Returns the
    /// icon with it so the caller can hand both straight back to `set_held`.
    pub fn inventory_pick(&self, frame: &Framebuffer, mx: f32, my: f32) -> Option<(Block, Image)> {
        if !self.open {
            return None;
        }
        let l = self.layout(frame);
        let (x, y) = (mx.round() as i32, my.round() as i32);
        let (grid_x, grid_y, cell) = self.inventory_origin(frame, &l);
        let rows = self.inventory.len().div_ceil(INVENTORY_COLS);
        if x < grid_x || y < grid_y {
            return None;
        }
        let (col, row) = (((x - grid_x) / cell) as usize, ((y - grid_y) / cell) as usize);
        if col >= INVENTORY_COLS || row >= rows {
            return None;
        }
        let index = row * INVENTORY_COLS + col;
        self.inventory
            .get(index)
            .map(|(block, icon)| (*block, icon.clone()))
    }

    /// Geometry of the hotbar, measured from the sprite rather than hard-coded so
    /// a pack at another resolution still lands on its own slots.
    fn layout(&self, frame: &Framebuffer) -> Layout {
        let unit = self.hotbar.width as f32 / 182.0;
        // Continuous, so the bar keeps the same share of the window instead of
        // jumping a whole step when it is resized.
        let scale = (frame.width as f32 / 600.0).clamp(1.0, 2.0);
        let px = |v: f32| (v * unit * scale).round() as i32;
        let bar_w = px(182.0);
        let bar_h = px(22.0);
        Layout {
            unit,
            scale,
            bar_w,
            bar_h,
            bar_x: (frame.width as i32 - bar_w) / 2,
            bar_y: frame.height as i32 - bar_h - px(4.0),
        }
    }

    /// Top-left corner of the inventory grid and the size of one cell.
    fn inventory_origin(&self, frame: &Framebuffer, l: &Layout) -> (i32, i32, i32) {
        let cell = l.px(22.0);
        let rows = self.inventory.len().div_ceil(INVENTORY_COLS) as i32;
        let width = cell * INVENTORY_COLS as i32;
        let height = cell * rows;
        (
            (frame.width as i32 - width) / 2,
            (frame.height as i32 - height) / 2 - l.px(10.0),
            cell,
        )
    }

    /// Composite the overlay onto a finished frame.
    ///
    /// `time_of_day` drives the clock dial and `yaw` the compass needle, so both
    /// items read like the real ones instead of being still pictures.
    pub fn draw(&mut self, frame: &mut Framebuffer, time_of_day: f32, yaw: f32) {
        if frame.width == 0 || frame.height == 0 || self.hotbar.width == 0 {
            return;
        }
        let l = self.layout(frame);
        let px = |v: f32| l.px(v);

        // The crosshair marks where a click lands when the mouse is not the one
        // aiming, and it is the only thing on screen while the inventory is up
        // that would get in the way, so it goes away then.
        if !self.open {
            let size = px(9.0);
            blit(
                frame,
                &self.crosshair,
                (frame.width as i32 - size) / 2,
                (frame.height as i32 - size) / 2,
                size,
                size,
                1.0,
            );
        }

        blit(frame, &self.hotbar, l.bar_x, l.bar_y, l.bar_w, l.bar_h, 1.0);

        // Slot centres: one pixel of border, then nine twenty-pixel cells.
        let slot_centre = |i: usize| l.bar_x + px(1.0 + 20.0 * i as f32 + 10.0);
        let centre_y = l.bar_y + l.bar_h / 2;

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
            let Some(icon) = slot.frame(time_of_day, yaw) else {
                continue;
            };
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
        let text_scale = l.scale.max(1.0);
        let label = self.slots[self.selected].label.clone();
        let label_w = self.font.width(&label, text_scale);
        let label_y = l.bar_y - self.font.height(text_scale) - px(3.0);
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

        if self.open {
            self.draw_inventory(frame, &l);
        }
    }

    /// The inventory screen: a dimmed frame and a grid of block faces.
    fn draw_inventory(&self, frame: &mut Framebuffer, l: &Layout) {
        let (grid_x, grid_y, cell) = self.inventory_origin(frame, l);
        let rows = self.inventory.len().div_ceil(INVENTORY_COLS) as i32;
        let pad = l.px(6.0);

        // Dim the whole picture, then draw the panel over it, so the grid reads
        // as a screen on top of the world rather than as blocks floating in it.
        dim(frame, 0.45);
        fill_rect(
            frame,
            grid_x - pad,
            grid_y - pad,
            cell * INVENTORY_COLS as i32 + pad * 2,
            cell * rows + pad * 2,
            0x0021_2126,
            220,
        );

        for (i, (_, icon)) in self.inventory.iter().enumerate() {
            let (col, row) = (i % INVENTORY_COLS, i / INVENTORY_COLS);
            let x = grid_x + col as i32 * cell;
            let y = grid_y + row as i32 * cell;
            fill_rect(frame, x + 1, y + 1, cell - 2, cell - 2, 0x0046_464C, 255);
            let inset = l.px(3.0);
            blit(
                frame,
                icon,
                x + inset,
                y + inset,
                cell - inset * 2,
                cell - inset * 2,
                1.0,
            );
        }

        let text = "Inventario: click para elegir un bloque, E para cerrar";
        let width = self.font.width(text, l.scale);
        self.font.draw(
            frame,
            (frame.width as i32 - width) / 2,
            grid_y - pad - self.font.height(l.scale) - l.px(3.0),
            text,
            l.scale,
        );
    }
}

/// Where the hotbar is and how big a hotbar pixel is on this frame.
struct Layout {
    unit: f32,
    scale: f32,
    bar_x: i32,
    bar_y: i32,
    bar_w: i32,
    bar_h: i32,
}

impl Layout {
    fn px(&self, v: f32) -> i32 {
        (v * self.unit * self.scale).round() as i32
    }
}

impl Slot {
    /// The icon to draw, or nothing at all: the block slot is empty until the
    /// inventory fills it.
    fn frame(&self, time_of_day: f32, yaw: f32) -> Option<&Image> {
        let n = self.frames.len();
        if n == 0 {
            return None;
        }
        let index = match self.animation {
            Animation::Static => 0,
            // The pack's frame 0 is noon, and our clock has noon at 0.25.
            Animation::Clock => ((time_of_day - 0.25).rem_euclid(1.0) * n as f32) as usize,
            Animation::Compass => {
                ((yaw / std::f32::consts::TAU).rem_euclid(1.0) * n as f32) as usize
            }
        };
        Some(&self.frames[index.min(n - 1)])
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
pub(crate) fn blit(frame: &mut Framebuffer, src: &Image, x: i32, y: i32, w: i32, h: i32, tint: f32) {
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

/// A flat rectangle, blended in. Used for the inventory panel and its cells.
pub(crate) fn fill_rect(frame: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, color: u32, alpha: u32) {
    let (fw, fh) = (frame.width as i32, frame.height as i32);
    let src = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
    for dy in y.max(0)..(y + h).min(fh) {
        for dx in x.max(0)..(x + w).min(fw) {
            let i = (dy * fw + dx) as usize;
            frame.pixels[i] = blend(frame.pixels[i], src, alpha);
        }
    }
}

/// Darken the whole frame, so an overlay on top of it reads as a screen.
pub(crate) fn dim(frame: &mut Framebuffer, amount: f32) {
    let keep = ((1.0 - amount) * 256.0) as u32;
    for p in frame.pixels.iter_mut() {
        let r = ((*p >> 16) & 0xff) * keep >> 8;
        let g = ((*p >> 8) & 0xff) * keep >> 8;
        let b = (*p & 0xff) * keep >> 8;
        *p = (r << 16) | (g << 8) | b;
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
    pub(crate) fn load(pack: &Pack) -> Result<Font, String> {
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
        Some(Hud::load(&Pack::open(None).ok()?).expect("the hud should load from the pack"))
    }

    #[test]
    fn every_slot_has_an_icon_and_an_action() {
        let Some(hud) = hud() else { return };
        assert_eq!(hud.slot_count(), 8);
        let mut actions = Vec::new();
        for (i, slot) in hud.slots.iter().enumerate() {
            // Every slot but the block one, which is empty until the inventory
            // hands it something.
            assert!(
                !slot.frames.is_empty() || slot.action == Action::Place,
                "slot {i} has no icon"
            );
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
                Action::Place,
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
        // The barrier is the last of the tools: stepping along the bar never
        // lands on it before anything else, and only the block slot follows it.
        let quit = hud.slots.iter().position(|s| s.action == Action::Quit).unwrap();
        assert_eq!(quit, hud.slots.len() - 2);
        assert_eq!(hud.slots.last().map(|s| s.action), Some(Action::Place));
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
        let noon = clock.frame(0.25, 0.0).unwrap() as *const Image;
        let midnight = clock.frame(0.75, 0.0).unwrap() as *const Image;
        assert_ne!(noon, midnight, "the clock dial should follow the hour");
        assert_eq!(noon, clock.frame(0.25, 3.0).unwrap() as *const Image, "yaw is not the clock's business");

        let compass = &hud.slots[2];
        let north = compass.frame(0.5, 0.0).unwrap() as *const Image;
        let south = compass.frame(0.5, std::f32::consts::PI).unwrap() as *const Image;
        assert_ne!(north, south, "the needle should follow the camera");
    }

    #[test]
    fn the_overlay_stays_out_of_the_picture() {
        let Some(mut hud) = hud() else { return };
        let mut frame = Framebuffer::new(640, 480);
        frame.pixels.fill(0x00336699);
        hud.draw(&mut frame, 0.3, 0.0);

        // The top quarter is the sky: the hotbar and its labels live at the
        // bottom, and the only thing in the middle is the crosshair.
        let untouched = frame.pixels[..640 * 120].iter().all(|&p| p == 0x00336699);
        assert!(untouched, "the hud reached into the top of the frame");
        let changed = frame.pixels[640 * 400..].iter().any(|&p| p != 0x00336699);
        assert!(changed, "the hud drew nothing");
        // Somewhere in the middle there is a crosshair; which exact pixel of it
        // is ink depends on the pack's sprite.
        let crosshair = (232..248)
            .flat_map(|y| (312..328).map(move |x| y * 640 + x))
            .any(|i| frame.pixels[i] != 0x00336699);
        assert!(crosshair, "no crosshair in the middle");
    }

    #[test]
    fn the_inventory_hands_a_block_to_the_hotbar() {
        let Some(mut hud) = hud() else { return };
        let frame = Framebuffer::new(900, 620);

        // Closed, a click on the middle of the screen picks nothing.
        assert!(hud.inventory_pick(&frame, 450.0, 300.0).is_none());

        hud.toggle_inventory();
        assert!(hud.open);
        // The first cell of the grid is the first entry of the table.
        let (grid_x, grid_y, cell) = hud.inventory_origin(&frame, &hud.layout(&frame));
        let pick = hud.inventory_pick(
            &frame,
            (grid_x + cell / 2) as f32,
            (grid_y + cell / 2) as f32,
        );
        let (block, icon) = pick.expect("the first cell should hold a block");
        assert_eq!(block, INVENTORY[0].0);

        // Choosing it fills the block slot and selects it, so the very next
        // right click places it.
        hud.set_held(block, icon);
        assert_eq!(hud.held(), Some(block));
        assert_eq!(hud.action(), Action::Place);
        assert!(hud.label().contains(blocks::name(block)));

        // Well outside the grid picks nothing.
        assert!(hud.inventory_pick(&frame, 2.0, 2.0).is_none());
    }

    #[test]
    fn the_inventory_dims_the_picture_behind_it() {
        let Some(mut hud) = hud() else { return };
        let mut frame = Framebuffer::new(640, 480);
        frame.pixels.fill(0x00808080);
        hud.toggle_inventory();
        hud.draw(&mut frame, 0.3, 0.0);
        // A corner well away from the panel: dimmed, not untouched, and not black.
        let corner = frame.pixels[10 * 640 + 10];
        assert!(corner < 0x00808080 && corner > 0x0010_1010, "corner is {corner:06x}");
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


