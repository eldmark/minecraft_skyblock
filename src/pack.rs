//! Access to the resource pack: finds the ZIP, decodes block textures on demand.

use std::path::{Path, PathBuf};

use crate::png;
use crate::texture::Texture;
use crate::zip::ZipArchive;

pub const BLOCK_DIR: &str = "assets/minecraft/textures/block/";

pub struct Pack {
    archive: ZipArchive,
    pub path: PathBuf,
}

impl Pack {
    /// Open the first `.zip` found in `texturepack/` (or an explicit path).
    pub fn open(explicit: Option<&Path>) -> Result<Pack, String> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => find_pack_zip(Path::new("texturepack"))?,
        };
        let archive = ZipArchive::open(&path)?;
        Ok(Pack { archive, path })
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        self.archive.read(name)
    }

    pub fn entry_count(&self) -> usize {
        self.archive.names().count()
    }

    pub fn decode_png(&self, name: &str) -> Result<png::Image, String> {
        png::decode(&self.read(name)?).map_err(|e| format!("{name}: {e}"))
    }

    /// Load `assets/minecraft/textures/block/<name>.png` as a texture.
    pub fn block_texture(&self, name: &str, normal_strength: f32) -> Result<Texture, String> {
        let image = self.decode_png(&format!("{BLOCK_DIR}{name}.png"))?;
        Ok(Texture::from_image(&image, normal_strength))
    }
}

fn find_pack_zip(dir: &Path) -> Result<PathBuf, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("pack: cannot read {}: {e}", dir.display()))?;
    let mut zips: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zip")))
        .collect();
    zips.sort();
    zips.into_iter()
        .next()
        .ok_or_else(|| format!("pack: no .zip found in {} (see README)", dir.display()))
}
