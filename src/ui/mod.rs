//! What the person sees and touches: the window (the only module that knows
//! minifb exists), the title screen, and the in-game overlay. All of it is
//! composited by hand onto the frame the raytracer already produced.

pub mod hud;
pub mod splash;
pub mod window;
