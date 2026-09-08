//! Access to the resource pack.
//!
//! Two backends behind one type: the `textures/` folder committed with the repo,
//! and the pack's own `.zip`. The folder holds exactly the files this program
//! asks for — 92 of the pack's 7861 entries — so the project can be cloned and
//! run with nothing to download; the ZIP is still read directly, with the
//! project's own inflate, when it is there and the folder is not.

use std::path::{Path, PathBuf};

use crate::codec::png;
use crate::assets::texture::Texture;
use crate::codec::zip::ZipArchive;

pub const BLOCK_DIR: &str = "assets/minecraft/textures/block/";

/// Where the texture bytes come from.
enum Source {
    /// The extracted `textures/` folder, laid out like the pack minus its
    /// `assets/minecraft/textures/` prefix.
    Dir(PathBuf),
    Zip(ZipArchive),
}

pub struct Pack {
    source: Source,
    pub path: PathBuf,
}

/// The part of a pack path that the extracted folder drops.
const TEXTURE_ROOT: &str = "assets/minecraft/textures/";

impl Pack {
    /// The extracted folder if it is there, otherwise the first `.zip` found in
    /// `texturepack/` — or exactly the pack given.
    pub fn open(explicit: Option<&Path>) -> Result<Pack, String> {
        if let Some(path) = explicit {
            let path = path.to_path_buf();
            if path.is_dir() {
                return Ok(Pack {
                    source: Source::Dir(path.clone()),
                    path,
                });
            }
            let archive = ZipArchive::open(&path)?;
            return Ok(Pack {
                source: Source::Zip(archive),
                path,
            });
        }

        let extracted = PathBuf::from("textures");
        if extracted.is_dir() {
            return Ok(Pack {
                source: Source::Dir(extracted.clone()),
                path: extracted,
            });
        }

        let path = find_pack_zip(Path::new("texturepack"))?;
        let archive = ZipArchive::open(&path)?;
        Ok(Pack {
            source: Source::Zip(archive),
            path,
        })
    }

    /// Read one entry. Names are always pack paths, whichever backend answers.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        match &self.source {
            Source::Zip(archive) => archive.read(name),
            Source::Dir(root) => {
                let relative = name.strip_prefix(TEXTURE_ROOT).unwrap_or(name);
                let path = root.join(relative);
                std::fs::read(&path).map_err(|e| format!("pack: {}: {e}", path.display()))
            }
        }
    }

    pub fn entry_count(&self) -> usize {
        match &self.source {
            Source::Zip(archive) => archive.names().count(),
            Source::Dir(root) => count_files(root),
        }
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

/// Every file under a directory, however deep.
fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                count_files(&path)
            } else {
                1
            }
        })
        .sum()
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
        .ok_or_else(|| {
            format!(
                "pack: no textures/ folder and no .zip in {} (see README)",
                dir.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extracted_folder_answers_the_same_names_as_the_zip() {
        let Ok(pack) = Pack::open(None) else { return };
        // Whatever backend answered, it does so under pack paths.
        let stone = pack.read(&format!("{BLOCK_DIR}stone.png"));
        assert!(stone.is_ok(), "{:?}", stone.err());
        assert!(pack.entry_count() > 50);

        // And a name that is in neither backend fails rather than panicking.
        assert!(pack.read(&format!("{BLOCK_DIR}not_a_texture.png")).is_err());
    }

    #[test]
    fn the_committed_folder_is_preferred_when_it_is_there() {
        if !Path::new("textures").is_dir() {
            return;
        }
        let pack = Pack::open(None).expect("textures/ should be enough on its own");
        assert!(matches!(pack.source, Source::Dir(_)));
        assert_eq!(pack.path, Path::new("textures"));
    }
}
