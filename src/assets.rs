//! Block -> textures -> material. The table that turns block ids into surfaces.
//!
//! Textures are loaded once from the pack and shared by index, so blocks that use
//! the same image (every stone face, say) decode it only once.

use std::collections::HashMap;

use crate::blocks::{self, Block, COUNT};
use crate::material::{missing_color, Material};
use crate::math::{vec3, Vec3};
use crate::pack::Pack;
use crate::texture::Texture;
use crate::world::Face;

/// Which pack texture each face of a block uses.
#[derive(Clone, Copy)]
struct FaceTextures {
    top: usize,
    bottom: usize,
    side: usize,
}

pub struct Assets {
    textures: Vec<Texture>,
    faces: Vec<FaceTextures>,
    materials: Vec<Material>,
}

/// Foliage tint. `grass_block_top` and `oak_leaves` ship in grayscale precisely so
/// the game can tint them per biome; without this the island renders gray.
const GRASS_TINT: Vec3 = vec3(0.42, 0.75, 0.32);
const LEAF_TINT: Vec3 = vec3(0.33, 0.62, 0.24);

impl Assets {
    pub fn load(pack: &Pack) -> Result<Assets, String> {
        let mut textures: Vec<Texture> = Vec::new();
        let mut by_name: HashMap<String, usize> = HashMap::new();
        let mut faces = vec![
            FaceTextures {
                top: usize::MAX,
                bottom: usize::MAX,
                side: usize::MAX,
            };
            COUNT
        ];
        let mut materials = vec![Material::default(); COUNT];

        let load = |name: &str,
                        strength: f32,
                        textures: &mut Vec<Texture>,
                        by_name: &mut HashMap<String, usize>|
         -> Result<usize, String> {
            if let Some(&i) = by_name.get(name) {
                return Ok(i);
            }
            let tex = pack.block_texture(name, strength)?;
            textures.push(tex);
            by_name.insert(name.to_string(), textures.len() - 1);
            Ok(textures.len() - 1)
        };

        // (block, top, side, bottom, material)
        let table: Vec<(Block, &str, &str, &str, Material)> = vec![
            (
                blocks::GRASS,
                "grass_block_top",
                "grass_block_side",
                "dirt",
                Material::diffuse(2.5).with_tint(GRASS_TINT),
            ),
            (blocks::DIRT, "dirt", "dirt", "dirt", Material::diffuse(2.5)),
            (blocks::STONE, "stone", "stone", "stone", Material::diffuse(3.5)),
            (
                blocks::COBBLESTONE,
                "cobblestone",
                "cobblestone",
                "cobblestone",
                Material::diffuse(4.5),
            ),
            (blocks::SAND, "sand", "sand", "sand", Material::diffuse(2.0)),
            (
                blocks::WATER,
                "water_still",
                "water_still",
                "water_still",
                // Clear enough that the sandy pond floor shows through: the point
                // of refraction is seeing what is behind the surface.
                Material::refractive(0.88, 1.33, 0.30).with_tint(vec3(0.62, 0.95, 1.05)),
            ),
            (
                blocks::OAK_LOG,
                "oak_log_top",
                "oak_log",
                "oak_log_top",
                Material::diffuse(3.0),
            ),
            (
                blocks::OAK_LEAVES,
                "oak_leaves",
                "oak_leaves",
                "oak_leaves",
                Material::diffuse(1.5).with_tint(LEAF_TINT),
            ),
            (
                blocks::OAK_PLANKS,
                "oak_planks",
                "oak_planks",
                "oak_planks",
                Material::diffuse(2.5),
            ),
            (
                blocks::GOLD_ORE,
                "gold_ore",
                "gold_ore",
                "gold_ore",
                Material {
                    specular: 0.35,
                    shininess: 48.0,
                    reflectivity: 0.08,
                    normal_strength: 3.0,
                    ..Material::default()
                },
            ),
            (
                blocks::IRON_ORE,
                "iron_ore",
                "iron_ore",
                "iron_ore",
                Material {
                    specular: 0.25,
                    shininess: 32.0,
                    normal_strength: 3.0,
                    ..Material::default()
                },
            ),
            (
                blocks::DIAMOND_ORE,
                "diamond_ore",
                "diamond_ore",
                "diamond_ore",
                Material {
                    specular: 0.45,
                    shininess: 64.0,
                    reflectivity: 0.10,
                    normal_strength: 3.0,
                    ..Material::default()
                },
            ),
            (
                blocks::GOLD_BLOCK,
                "gold_block",
                "gold_block",
                "gold_block",
                Material::metal(vec3(1.0, 0.86, 0.52), 0.55),
            ),
            (
                blocks::IRON_BLOCK,
                "iron_block",
                "iron_block",
                "iron_block",
                Material::metal(vec3(0.95, 0.95, 0.98), 0.45),
            ),
            (
                blocks::DIAMOND_BLOCK,
                "diamond_block",
                "diamond_block",
                "diamond_block",
                Material::metal(vec3(0.72, 0.98, 1.0), 0.35),
            ),
            (
                blocks::QUARTZ,
                "quartz_block_top",
                "quartz_block_side",
                "quartz_block_bottom",
                Material {
                    specular: 0.25,
                    shininess: 40.0,
                    reflectivity: 0.06,
                    normal_strength: 2.0,
                    ..Material::default()
                },
            ),
            (
                blocks::QUARTZ_PILLAR,
                "quartz_pillar_top",
                "quartz_pillar",
                "quartz_pillar_top",
                Material {
                    specular: 0.25,
                    shininess: 40.0,
                    reflectivity: 0.06,
                    normal_strength: 2.5,
                    ..Material::default()
                },
            ),
            (
                blocks::GLOWSTONE,
                "glowstone",
                "glowstone",
                "glowstone",
                Material::diffuse(2.0).with_emission(3.2),
            ),
            (
                blocks::GLASS,
                "glass",
                "glass",
                "glass",
                Material::refractive(0.88, 1.52, 0.12),
            ),
            (
                // The portal reads as thick, smoky crystal rather than as water.
                blocks::PORTAL,
                "quartz_block_top",
                "quartz_block_top",
                "quartz_block_top",
                Material::refractive(0.72, 1.45, 0.20)
                    .with_tint(vec3(0.55, 0.75, 1.0))
                    .with_emission(0.9),
            ),
            (
                blocks::STONE_BRICKS,
                "stone_bricks",
                "stone_bricks",
                "stone_bricks",
                Material::diffuse(4.0),
            ),
            (
                blocks::MOSSY_STONE_BRICKS,
                "mossy_stone_bricks",
                "mossy_stone_bricks",
                "mossy_stone_bricks",
                Material::diffuse(4.0),
            ),
            (
                blocks::CRACKED_STONE_BRICKS,
                "cracked_stone_bricks",
                "cracked_stone_bricks",
                "cracked_stone_bricks",
                Material::diffuse(4.5),
            ),
            (
                blocks::CHISELED_QUARTZ,
                "chiseled_quartz_block_top",
                "chiseled_quartz_block",
                "chiseled_quartz_block_top",
                Material {
                    specular: 0.28,
                    shininess: 44.0,
                    reflectivity: 0.07,
                    normal_strength: 3.0,
                    ..Material::default()
                },
            ),
            (
                blocks::QUARTZ_BRICKS,
                "quartz_bricks",
                "quartz_bricks",
                "quartz_bricks",
                Material {
                    specular: 0.22,
                    shininess: 36.0,
                    reflectivity: 0.05,
                    normal_strength: 3.0,
                    ..Material::default()
                },
            ),
            (
                blocks::GRAVEL,
                "gravel",
                "gravel",
                "gravel",
                Material::diffuse(3.0),
            ),
            (
                blocks::MOSS,
                "moss_block",
                "moss_block",
                "moss_block",
                Material::diffuse(2.0).with_tint(vec3(0.62, 0.85, 0.55)),
            ),
            (
                // Obsidian is glassy volcanic rock: dark, and shiny enough that a
                // dragon built from it catches every lantern around the temple.
                blocks::OBSIDIAN,
                "obsidian",
                "obsidian",
                "obsidian",
                Material {
                    specular: 0.55,
                    shininess: 72.0,
                    reflectivity: 0.22,
                    normal_strength: 3.5,
                    ..Material::default()
                },
            ),
            (
                blocks::CRYING_OBSIDIAN,
                "crying_obsidian",
                "crying_obsidian",
                "crying_obsidian",
                Material {
                    specular: 0.55,
                    shininess: 72.0,
                    reflectivity: 0.22,
                    normal_strength: 3.5,
                    ..Material::default()
                }
                .with_emission(0.5),
            ),
            (
                blocks::EMERALD_BLOCK,
                "emerald_block",
                "emerald_block",
                "emerald_block",
                Material::metal(vec3(0.55, 1.0, 0.68), 0.30),
            ),
            (
                blocks::REDSTONE_BLOCK,
                "redstone_block",
                "redstone_block",
                "redstone_block",
                Material::diffuse(3.0).with_emission(0.25),
            ),
            (
                // The dragon's hide. Wool is matte and slightly fuzzy, which is
                // what keeps a white animal from turning into a white glare
                // against the quartz temple it is coiled around.
                blocks::WHITE_WOOL,
                "white_wool",
                "white_wool",
                "white_wool",
                Material {
                    specular: 0.10,
                    shininess: 12.0,
                    normal_strength: 2.5,
                    ..Material::default()
                },
            ),
            (
                blocks::RED_WOOL,
                "red_wool",
                "red_wool",
                "red_wool",
                Material {
                    specular: 0.10,
                    shininess: 12.0,
                    normal_strength: 2.5,
                    ..Material::default()
                },
            ),
            (
                // Concrete is flat and hard next to the wool: the crest and the
                // scales read as a different surface, not just a different color.
                blocks::RED_CONCRETE,
                "red_concrete",
                "red_concrete",
                "red_concrete",
                Material {
                    specular: 0.20,
                    shininess: 30.0,
                    reflectivity: 0.04,
                    normal_strength: 1.5,
                    ..Material::default()
                },
            ),
            (
                // Netherrack breaks up the red with its own grain, and glows
                // faintly inside the open jaw.
                blocks::NETHERRACK,
                "netherrack",
                "netherrack",
                "netherrack",
                Material {
                    specular: 0.12,
                    shininess: 16.0,
                    normal_strength: 4.0,
                    ..Material::default()
                }
                .with_emission(0.12),
            ),
            (
                blocks::BLACK_CONCRETE,
                "black_concrete",
                "black_concrete",
                "black_concrete",
                Material {
                    specular: 0.28,
                    shininess: 42.0,
                    reflectivity: 0.06,
                    normal_strength: 1.5,
                    ..Material::default()
                },
            ),
            (
                // Slabs and fences reuse the plank texture, as the game does: the
                // shape is in `blocks::shape`, not in the material.
                blocks::OAK_SLAB,
                "oak_planks",
                "oak_planks",
                "oak_planks",
                Material::diffuse(2.5),
            ),
            (
                blocks::OAK_FENCE,
                "oak_planks",
                "oak_planks",
                "oak_planks",
                Material::diffuse(2.5),
            ),
            (
                blocks::NETHER_BRICKS,
                "nether_bricks",
                "nether_bricks",
                "nether_bricks",
                Material::diffuse(3.0),
            ),
            (
                blocks::SOUL_SAND,
                "soul_sand",
                "soul_sand",
                "soul_sand",
                Material::diffuse(3.5),
            ),
            (
                // Magma glows from the cracks between its plates.
                blocks::MAGMA,
                "magma",
                "magma",
                "magma",
                Material::diffuse(2.5)
                    .with_tint(vec3(1.0, 0.72, 0.45))
                    .with_emission(1.4),
            ),
            (
                // Lava is the nether island's key light: bright, warm, animated,
                // and just glossy enough to catch the sky at a grazing angle.
                blocks::LAVA,
                "lava_still",
                "lava_still",
                "lava_still",
                Material {
                    specular: 0.18,
                    shininess: 24.0,
                    emission: 4.0,
                    tint: vec3(1.0, 0.68, 0.34),
                    ..Material::default()
                },
            ),
            (
                // The nether portal: transparent, refracting and lit from within,
                // the same trick as the crystal gate but in purple.
                blocks::NETHER_PORTAL,
                "nether_portal",
                "nether_portal",
                "nether_portal",
                Material::refractive(0.55, 1.25, 0.18)
                    .with_tint(vec3(0.86, 0.45, 1.0))
                    .with_emission(2.2),
            ),
            (
                blocks::HAY_BLOCK,
                "hay_block_top",
                "hay_block_side",
                "hay_block_top",
                Material::diffuse(2.5),
            ),
            (
                blocks::FARMLAND,
                "farmland_moist",
                "dirt",
                "dirt",
                Material::diffuse(3.0),
            ),
            (
                // Pumpkins are the farm's crop: a solid block reads at diorama
                // distance, and wheat — a texture of thin stalks with holes in it
                // — did not, besides paying for an alpha skip on every ray.
                blocks::PUMPKIN,
                "pumpkin_top",
                "pumpkin_side",
                "pumpkin_top",
                Material::diffuse(2.5),
            ),
            (
                blocks::BROWN_WOOL,
                "brown_wool",
                "brown_wool",
                "brown_wool",
                Material {
                    specular: 0.10,
                    shininess: 12.0,
                    normal_strength: 2.5,
                    ..Material::default()
                },
            ),
            (
                blocks::FLOWERING_LEAVES,
                "flowering_azalea_leaves",
                "flowering_azalea_leaves",
                "flowering_azalea_leaves",
                Material::diffuse(1.5),
            ),
        ];

        for (block, top, side, bottom, material) in table {
            let strength = material.normal_strength;
            let f = FaceTextures {
                top: load(top, strength, &mut textures, &mut by_name)?,
                side: load(side, strength, &mut textures, &mut by_name)?,
                bottom: load(bottom, strength, &mut textures, &mut by_name)?,
            };
            faces[block as usize] = f;
            materials[block as usize] = material;
        }

        Ok(Assets {
            textures,
            faces,
            materials,
        })
    }

    pub fn material(&self, block: Block) -> &Material {
        &self.materials[block as usize]
    }

    #[cfg(test)]
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    fn texture_for(&self, block: Block, face: Face) -> Option<&Texture> {
        let f = *self.faces.get(block as usize)?;
        let index = match face {
            Face::PosY => f.top,
            Face::NegY => f.bottom,
            _ => f.side,
        };
        self.textures.get(index)
    }

    /// Sampled color (already tinted) and alpha for a face.
    pub fn sample(&self, block: Block, face: Face, u: f32, v: f32, frame: usize) -> (Vec3, f32) {
        let material = self.material(block);
        match self.texture_for(block, face) {
            Some(tex) => (
                tex.sample(u, v, frame).mul_elem(material.tint),
                tex.sample_alpha(u, v, frame),
            ),
            None => (missing_color(u, v), 1.0),
        }
    }

    /// Tangent-space normal from the block's own texture.
    pub fn sample_normal(&self, block: Block, face: Face, u: f32, v: f32, frame: usize) -> Vec3 {
        match self.texture_for(block, face) {
            Some(tex) => tex.sample_normal(u, v, frame),
            None => vec3(0.0, 0.0, 1.0),
        }
    }

    /// How many animation frames a block's side texture has (1 when static).
    pub fn frame_count(&self, block: Block, face: Face) -> usize {
        self.texture_for(block, face).map_or(1, |t| t.frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The pack is not committed, so these tests skip rather than fail when it is
    /// absent (see README for where to put it).
    fn pack() -> Option<Pack> {
        if !Path::new("texturepack").is_dir() {
            return None;
        }
        Pack::open(None).ok()
    }

    #[test]
    fn every_block_id_resolves_to_a_texture_and_material() {
        let Some(pack) = pack() else { return };
        let assets = Assets::load(&pack).expect("assets should load");
        for block in 1..COUNT as Block {
            let (color, alpha) = assets.sample(block, Face::PosY, 0.5, 0.5, 0);
            assert!(
                color.max_component() >= 0.0 && (0.0..=1.0).contains(&alpha),
                "block {} sampled badly",
                blocks::name(block)
            );
            assert_ne!(
                color,
                missing_color(0.5, 0.5),
                "block {} has no texture",
                blocks::name(block)
            );
        }
    }

    #[test]
    fn grass_is_tinted_green_not_gray() {
        let Some(pack) = pack() else { return };
        let assets = Assets::load(&pack).unwrap();
        let (color, _) = assets.sample(blocks::GRASS, Face::PosY, 0.5, 0.5, 0);
        assert!(
            color.y > color.x * 1.2 && color.y > color.z * 1.2,
            "grass should be green, got {color:?}"
        );
    }

    #[test]
    fn water_is_animated_and_refractive() {
        let Some(pack) = pack() else { return };
        let assets = Assets::load(&pack).unwrap();
        assert!(assets.frame_count(blocks::WATER, Face::PosY) > 1);
        let material = assets.material(blocks::WATER);
        assert!(material.is_transparent() && material.ior > 1.3);
    }

    #[test]
    fn shared_textures_are_decoded_once() {
        let Some(pack) = pack() else { return };
        let assets = Assets::load(&pack).unwrap();
        // Dirt is used by three blocks (dirt, grass bottom, grass sides differ),
        // so the atlas must hold fewer textures than blocks x faces.
        assert!(assets.texture_count() < COUNT * 3);
    }

    #[test]
    fn top_and_side_of_grass_differ() {
        let Some(pack) = pack() else { return };
        let assets = Assets::load(&pack).unwrap();
        let top = assets.sample(blocks::GRASS, Face::PosY, 0.5, 0.5, 0).0;
        let side = assets.sample(blocks::GRASS, Face::PosX, 0.5, 0.9, 0).0;
        assert_ne!(top, side, "grass top and side should use different textures");
    }
}
