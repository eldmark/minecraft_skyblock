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
pub const STONE_BRICKS: Block = 21;
pub const MOSSY_STONE_BRICKS: Block = 22;
pub const CRACKED_STONE_BRICKS: Block = 23;
pub const CHISELED_QUARTZ: Block = 24;
pub const QUARTZ_BRICKS: Block = 25;
pub const GRAVEL: Block = 26;
pub const MOSS: Block = 27;
pub const FLOWERING_LEAVES: Block = 28;
pub const OBSIDIAN: Block = 29;
pub const CRYING_OBSIDIAN: Block = 30;
pub const EMERALD_BLOCK: Block = 31;
pub const REDSTONE_BLOCK: Block = 32;
pub const WHITE_WOOL: Block = 33;
pub const RED_WOOL: Block = 34;
pub const RED_CONCRETE: Block = 35;
pub const NETHERRACK: Block = 36;
pub const BLACK_CONCRETE: Block = 37;
pub const OAK_SLAB: Block = 38;
pub const OAK_FENCE: Block = 39;
pub const NETHER_BRICKS: Block = 40;
pub const SOUL_SAND: Block = 41;
pub const MAGMA: Block = 42;
pub const LAVA: Block = 43;
pub const NETHER_PORTAL: Block = 44;
pub const HAY_BLOCK: Block = 45;
pub const FARMLAND: Block = 46;
pub const PUMPKIN: Block = 47;
pub const BROWN_WOOL: Block = 48;

pub const COUNT: usize = 49;

/// Blocks that emit light of their own.
pub fn is_emissive(block: Block) -> bool {
    // Magma is deliberately absent: it covers the whole underside of the nether
    // island, and as a light source it added forty clusters that every shaded
    // point had to walk past. Its material still glows, which is all it is for.
    matches!(block, GLOWSTONE | PORTAL | LAVA | NETHER_PORTAL)
}

/// How much of its cell a block fills. Everything is a full cube unless it is
/// one of the few shapes the bridge and the farm need.
pub fn shape(block: Block) -> crate::scene::world::Shape {
    use crate::scene::world::Shape;
    match block {
        OAK_SLAB => Shape::Slab,
        OAK_FENCE => Shape::Fence,
        _ => Shape::Full,
    }
}

/// Used in test failures and error messages.
#[allow(dead_code)]
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
        STONE_BRICKS => "stone_bricks",
        MOSSY_STONE_BRICKS => "mossy_stone_bricks",
        CRACKED_STONE_BRICKS => "cracked_stone_bricks",
        CHISELED_QUARTZ => "chiseled_quartz_block",
        QUARTZ_BRICKS => "quartz_bricks",
        GRAVEL => "gravel",
        MOSS => "moss_block",
        FLOWERING_LEAVES => "flowering_azalea_leaves",
        OBSIDIAN => "obsidian",
        CRYING_OBSIDIAN => "crying_obsidian",
        EMERALD_BLOCK => "emerald_block",
        REDSTONE_BLOCK => "redstone_block",
        WHITE_WOOL => "white_wool",
        RED_WOOL => "red_wool",
        RED_CONCRETE => "red_concrete",
        NETHERRACK => "netherrack",
        BLACK_CONCRETE => "black_concrete",
        OAK_SLAB => "oak_slab",
        OAK_FENCE => "oak_fence",
        NETHER_BRICKS => "nether_bricks",
        SOUL_SAND => "soul_sand",
        MAGMA => "magma",
        LAVA => "lava",
        NETHER_PORTAL => "nether_portal",
        HAY_BLOCK => "hay_block",
        FARMLAND => "farmland",
        PUMPKIN => "pumpkin",
        BROWN_WOOL => "brown_wool",
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
    fn emission_agrees_with_intent() {
        assert!(is_emissive(GLOWSTONE) && is_emissive(PORTAL));
        assert!(!is_emissive(GOLD_BLOCK) && !is_emissive(STONE));
    }
}
