//! Where a rendered frame goes. The renderer never knows.
//!
//! Two backends: a live window (minifb, which only blits pixels — no GPU context),
//! and a PNG writer for offline orbits. Keeping this behind a trait means the
//! project can be delivered as a video if the live window is ever ruled out.

use std::io;
use std::path::{Path, PathBuf};

use crate::codec::png;

/// A linear-indexed 0RGB framebuffer, the format minifb wants.
pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Framebuffer {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    /// Resize if the window changed, and only then clear.
    ///
    /// Clearing unconditionally used to be harmless because every frame rewrote
    /// every pixel. It stopped being harmless once the refinement pass began
    /// skipping pixels that had not changed: the skipped ones were cleared to
    /// black and never written back, leaving the image as a few scattered dots.
    pub fn resize(&mut self, width: usize, height: usize) {
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            self.pixels.clear();
            self.pixels.resize(width * height, 0);
        }
    }

    pub fn to_rgb_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 3);
        for &p in &self.pixels {
            out.push((p >> 16) as u8);
            out.push((p >> 8) as u8);
            out.push(p as u8);
        }
        out
    }

    pub fn save_png(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        std::fs::write(path, png::encode_rgb(self.width, self.height, &self.to_rgb_bytes()))
    }
}

pub trait Output {
    /// Show or store one frame. Returns false when the run should stop.
    fn present(&mut self, frame: &Framebuffer) -> io::Result<bool>;

    /// Free-form status line (fps, frame counter).
    fn set_status(&mut self, _status: &str) {}
}

/// Writes each frame as `dir/frame_00000.png`.
pub struct FileOutput {
    dir: PathBuf,
    index: usize,
    limit: usize,
}

impl FileOutput {
    pub fn new(dir: impl Into<PathBuf>, limit: usize) -> Self {
        FileOutput {
            dir: dir.into(),
            index: 0,
            limit,
        }
    }
}

impl Output for FileOutput {
    fn present(&mut self, frame: &Framebuffer) -> io::Result<bool> {
        let path = self.dir.join(format!("frame_{:05}.png", self.index));
        frame.save_png(&path)?;
        self.index += 1;
        Ok(self.index < self.limit)
    }
}

/// Discards frames. Used by `--bench`, so timings measure the renderer alone.
pub struct NullOutput {
    remaining: usize,
}

impl NullOutput {
    pub fn new(frames: usize) -> Self {
        NullOutput { remaining: frames }
    }
}

impl Output for NullOutput {
    fn present(&mut self, _frame: &Framebuffer) -> io::Result<bool> {
        self.remaining = self.remaining.saturating_sub(1);
        Ok(self.remaining > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_to_the_same_size_keeps_the_image() {
        let mut frame = Framebuffer::new(8, 4);
        frame.pixels.fill(0x00FF00);
        frame.resize(8, 4);
        assert!(
            frame.pixels.iter().all(|&p| p == 0x00FF00),
            "a same-size resize must not clear the framebuffer"
        );
    }

    #[test]
    fn resizing_to_a_new_size_starts_clean() {
        let mut frame = Framebuffer::new(8, 4);
        frame.pixels.fill(0x00FF00);
        frame.resize(9, 4);
        assert_eq!(frame.pixels.len(), 36);
        assert!(frame.pixels.iter().all(|&p| p == 0));
    }
}
