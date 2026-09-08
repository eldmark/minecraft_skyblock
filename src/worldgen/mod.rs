//! Everything that decides what the world contains, before a single ray is cast:
//! the noise it is built from, the procedural island, the hand-placed structures,
//! and the composition of the three islands into one grid.

pub mod neighbours;
pub mod noise;
pub mod structures;
pub mod terrain;
