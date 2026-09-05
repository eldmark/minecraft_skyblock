//! Block ids. One byte per voxel; textures and material parameters are attached
//! in `assets.rs`, so the world stays a plain grid of numbers.

pub type Block = u8;

pub const AIR: Block = 0;
pub const GRASS: Block = 1;
pub const DIRT: Block = 2;
pub const STONE: Block = 3;
pub const COBBLESTONE: Block = 4;
pub const SAND: Block = 5;
pub const WATER: Block = 6;
pub const OAK_LOG: Block = 7;
pub const OAK_LEAVES: Block = 8;
pub const OAK_PLANKS: Block = 9;
pub const GOLD_ORE: Block = 10;
pub const IRON_ORE: Block = 11;
pub const DIAMOND_ORE: Block = 12;
pub const GOLD_BLOCK: Block = 13;
pub const IRON_BLOCK: Block = 14;
pub const DIAMOND_BLOCK: Block = 15;
pub const QUARTZ: Block = 16;
pub const QUARTZ_PILLAR: Block = 17;
pub const GLOWSTONE: Block = 18;
pub const GLASS: Block = 19;
pub const PORTAL: Block = 20;

pub const COUNT: usize = 21;

/// Light passes through these, so they need special handling in shadow rays and
/// in refraction. Kept here because both the renderer and the generator ask.
pub fn is_transparent(block: Block) -> bool {
    matches!(block, AIR | WATER | GLASS | PORTAL)
}

/// Blocks that emit light of their own.
pub fn is_emissive(block: Block) -> bool {
    matches!(block, GLOWSTONE | PORTAL)
}

/// Blocks a generator may replace when carving or planting.
pub fn is_replaceable(block: Block) -> bool {
    matches!(block, AIR | WATER | OAK_LEAVES)
}

pub fn name(block: Block) -> &'static str {
    match block {
        AIR => "air",
        GRASS => "grass_block",
        DIRT => "dirt",
        STONE => "stone",
        COBBLESTONE => "cobblestone",
        SAND => "sand",
        WATER => "water",
        OAK_LOG => "oak_log",
        OAK_LEAVES => "oak_leaves",
        OAK_PLANKS => "oak_planks",
        GOLD_ORE => "gold_ore",
        IRON_ORE => "iron_ore",
        DIAMOND_ORE => "diamond_ore",
        GOLD_BLOCK => "gold_block",
        IRON_BLOCK => "iron_block",
        DIAMOND_BLOCK => "diamond_block",
        QUARTZ => "quartz_block",
        QUARTZ_PILLAR => "quartz_pillar",
        GLOWSTONE => "glowstone",
        GLASS => "glass",
        PORTAL => "portal",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_below_count_has_a_name() {
        for block in 0..COUNT as Block {
            assert_ne!(name(block), "unknown", "block {block} has no name");
        }
    }

    #[test]
    fn transparency_and_emission_agree_with_intent() {
        assert!(is_transparent(WATER) && is_transparent(GLASS) && is_transparent(PORTAL));
        assert!(!is_transparent(STONE) && !is_transparent(GRASS));
        assert!(is_emissive(GLOWSTONE) && is_emissive(PORTAL));
        assert!(!is_emissive(GOLD_BLOCK));
    }
}
