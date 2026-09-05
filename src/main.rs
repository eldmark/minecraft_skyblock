//! Skyblock diorama — a CPU raytracer.
//!
//! Modes:
//!   skyblock                       live window
//!   skyblock --render out/ -n 240  write an orbit as PNGs, no window
//!   skyblock --bench 60            time N frames, no window, no I/O

mod assets;
mod blocks;
mod camera;
mod inflate;
mod noise;
mod material;
mod math;
mod output;
mod pack;
mod parallel;
mod png;
mod render;
mod scene;
mod skybox;
mod structures;
mod terrain;
mod texture;
mod window;
mod world;
mod zip;

use std::io;
use std::path::PathBuf;
use std::time::Instant;

use camera::Camera;
use math::vec3;
use output::{FileOutput, Framebuffer, NullOutput, Output};
use render::Renderer;
use scene::Scene;
use window::WindowOutput;
use world::World;

enum Mode {
    Window,
    Render { dir: PathBuf },
    Bench,
    /// Decode textures straight from the pack and report what was found.
    CheckPack { dump: Option<(String, PathBuf)> },
}

struct Args {
    mode: Mode,
    width: usize,
    height: usize,
    frames: usize,
    /// Override the worker count; used to measure scaling.
    threads: Option<usize>,
    seed: u32,
    /// Use the pack's panorama cubemap instead of the procedural dusk sky.
    panorama_sky: bool,
    /// Jittered samples per frame for offline rendering.
    samples: usize,
}

impl Args {
    fn parse() -> Args {
        let mut args = Args {
            mode: Mode::Window,
            width: 900,
            height: 600,
            frames: 240,
            threads: None,
            seed: 2024,
            panorama_sky: false,
            samples: 1,
        };
        let mut argv = std::env::args().skip(1);
        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--window" => args.mode = Mode::Window,
                "--render" => {
                    let dir = argv.next().unwrap_or_else(|| "out".into());
                    args.mode = Mode::Render { dir: dir.into() };
                }
                "--check-pack" => args.mode = Mode::CheckPack { dump: None },
                "--dump" => {
                    let name = argv.next().unwrap_or_default();
                    let out = argv.next().unwrap_or_else(|| "dump.rgba".into());
                    args.mode = Mode::CheckPack {
                        dump: Some((name, out.into())),
                    };
                }
                "--bench" => {
                    args.mode = Mode::Bench;
                    if let Some(n) = argv.next().and_then(|v| v.parse().ok()) {
                        args.frames = n;
                    }
                }
                "--frames" | "-n" => {
                    if let Some(n) = argv.next().and_then(|v| v.parse().ok()) {
                        args.frames = n;
                    }
                }
                "--seed" => {
                    if let Some(n) = argv.next().and_then(|v| v.parse().ok()) {
                        args.seed = n;
                    }
                }
                "--sky-panorama" => args.panorama_sky = true,
                "--samples" => {
                    if let Some(n) = argv.next().and_then(|v| v.parse().ok()) {
                        args.samples = n;
                    }
                }
                "--threads" => args.threads = argv.next().and_then(|v| v.parse().ok()),
                "--width" => {
                    if let Some(n) = argv.next().and_then(|v| v.parse().ok()) {
                        args.width = n;
                    }
                }
                "--height" => {
                    if let Some(n) = argv.next().and_then(|v| v.parse().ok()) {
                        args.height = n;
                    }
                }
                "--help" | "-h" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                other => {
                    eprintln!("unknown argument: {other}\n\n{USAGE}");
                    std::process::exit(2);
                }
            }
        }
        args
    }
}

const USAGE: &str = "\
usage: skyblock [--window | --render DIR | --bench N] [--frames N] [--width W] [--height H]

  --window         live window (default): drag to orbit, scroll or W/S to zoom,
                   R reseeds, P screenshots, 1-4 set resolution scale, Esc quits
  --render DIR     write an orbit as PNG frames, no window
  --bench N        render N frames and report ms/frame
  --threads N      force the worker count (default: all cores)
  --seed N         terrain seed (R reseeds in the window)
  --sky-panorama   use the pack's panorama cubemap instead of the dusk sky
  --samples N      jittered samples per offline frame (antialiasing)
  --check-pack     decode textures from the resource pack and report findings
  --dump NAME OUT  decode one pack entry and write its raw RGBA bytes";

/// Load the resource pack and build the scene, with a clear message when the pack
/// is missing: it is not committed to the repository.
fn load_scene(seed: u32, panorama_sky: bool) -> io::Result<Scene> {
    let pack = pack::Pack::open(None).map_err(io::Error::other)?;
    Scene::load(seed, &pack, panorama_sky).map_err(io::Error::other)
}

fn scene_camera(world: &World) -> Camera {
    // Aim a little above the middle of the terrain, where the island sits.
    let center = vec3(
        world.size[0] as f32 * 0.5,
        terrain::SURFACE_LEVEL - 4.0,
        world.size[2] as f32 * 0.5,
    );
    Camera::new(center, world.size[0] as f32 * 1.9)
}

fn run_headless(mut out: Box<dyn Output>, args: &Args, report: bool) -> io::Result<()> {
    let mut scene = load_scene(args.seed, args.panorama_sky)?;
    let mut camera = scene_camera(scene.world());
    let mut frame = Framebuffer::new(args.width, args.height);
    let mut renderer = Renderer::new();
    if let Some(threads) = args.threads {
        renderer.threads = threads;
    }

    let start = Instant::now();
    let mut rendered = 0usize;
    loop {
        // A full turn over the requested frame count, so --render yields a loop.
        camera.yaw = std::f32::consts::TAU * rendered as f32 / args.frames.max(1) as f32;
        if args.samples <= 1 {
            renderer.render(&mut frame, &scene, &camera);
        } else {
            // Offline frames get antialiasing: several jittered samples averaged.
            renderer.reset_accumulation();
            for _ in 0..args.samples {
                renderer.accumulate(&mut frame, &scene, &camera);
            }
        }
        scene.tick += 1;
        rendered += 1;
        if !out.present(&frame)? {
            break;
        }
    }
    if report {
        let ms = start.elapsed().as_secs_f64() * 1000.0 / rendered as f64;
        println!(
            "{rendered} frames at {}x{}, {} threads: {ms:.2} ms/frame ({:.1} fps)",
            args.width,
            args.height,
            renderer.threads,
            1000.0 / ms
        );
    }
    Ok(())
}

fn run_window(args: &Args) -> io::Result<()> {
    let mut scene = load_scene(args.seed, args.panorama_sky)?;
    let mut camera = scene_camera(scene.world());
    let mut win = WindowOutput::new("Skyblock Diorama", args.width, args.height)?;
    let mut frame = Framebuffer::new(args.width, args.height);
    let mut renderer = Renderer::new();
    if let Some(threads) = args.threads {
        renderer.threads = threads;
    }
    let mut shots = 0usize;
    let mut quality = 1usize;

    while win.is_open() {
        let frame_start = Instant::now();
        let input = win.poll_input();
        camera.apply(input.orbit, input.zoom);

        if input.reseed {
            let seed = scene
                .island
                .seed
                .wrapping_mul(1664525)
                .wrapping_add(1013904223);
            scene.reseed(seed);
            println!("regenerated terrain with seed {seed}");
        }
        if let Some(q) = input.quality {
            quality = q;
        }
        let (w, h) = win.size();
        let resized = w != frame.width || h != frame.height;
        frame.resize(w, h);

        // While the camera moves: drop resolution and re-render every frame.
        // Once it settles: full resolution, and keep folding in jittered samples
        // until the image converges, which is where the antialiasing comes from.
        let moving = !input.is_idle() || resized || input.reseed;
        if moving {
            renderer.scale = quality.max(2);
            renderer.render(&mut frame, &scene, &camera);
            renderer.reset_accumulation();
            scene.tick += 1;
        } else {
            renderer.scale = quality;
            if renderer.is_converged() {
                // Converged: only redraw to advance water and portal animation.
                renderer.render(&mut frame, &scene, &camera);
                renderer.reset_accumulation();
                scene.tick += 1;
            } else {
                renderer.accumulate(&mut frame, &scene, &camera);
            }
        }

        if input.screenshot {
            let path = PathBuf::from(format!("screenshot_{shots:03}.png"));
            frame.save_png(&path)?;
            println!("wrote {}", path.display());
            shots += 1;
        }

        let ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        win.set_status(&format!(
            "{ms:.1} ms  {:.0} fps  {w}x{h}/{}  {} threads  {} spp  dist {:.0}",
            1000.0 / ms.max(0.001),
            renderer.scale,
            renderer.threads,
            renderer.samples.max(1),
            camera.distance
        ));
        if !win.present(&frame)? {
            break;
        }
    }
    Ok(())
}

/// Exercises the hand-written ZIP reader, inflate and PNG decoder against the
/// real pack, and can dump raw pixels so they can be diffed against a reference.
fn check_pack(dump: Option<&(String, PathBuf)>) -> io::Result<()> {
    let pack = pack::Pack::open(None).map_err(io::Error::other)?;
    println!("pack: {} ({} entries)", pack.path.display(), pack.entry_count());

    if let Some((name, out_path)) = dump {
        let image = pack.decode_png(name).map_err(io::Error::other)?;
        std::fs::write(out_path, &image.rgba)?;
        println!(
            "{name}: {}x{} -> {} ({} bytes RGBA)",
            image.width,
            image.height,
            out_path.display(),
            image.rgba.len()
        );
        return Ok(());
    }

    for name in [
        "stone",
        "grass_block_top",
        "dirt",
        "water_still",
        "glass",
        "gold_block",
        "glowstone",
        "quartz_block_side",
        "oak_leaves",
        "diamond_ore",
    ] {
        match pack.block_texture(name, 3.0) {
            Ok(tex) => println!(
                "  {name:<18} {}x{} x{} frames  avg {:?}",
                tex.width,
                tex.height,
                tex.frames,
                tex.average()
            ),
            Err(e) => println!("  {name:<18} FAILED: {e}"),
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    match &args.mode {
        Mode::Window => run_window(&args),
        Mode::Render { dir } => {
            println!("rendering {} frames to {}", args.frames, dir.display());
            run_headless(Box::new(FileOutput::new(dir.clone(), args.frames)), &args, true)
        }
        Mode::Bench => run_headless(Box::new(NullOutput::new(args.frames)), &args, true),
        Mode::CheckPack { dump } => check_pack(dump.as_ref()),
    }
}

#[cfg(test)]
mod scene_tests {
    use super::*;

    #[test]
    fn the_placeholder_island_is_visible_from_the_default_camera() {
        let world = terrain::generate(2024).world;
        let camera = scene_camera(&world);
        let (w, h) = (64usize, 48usize);
        let mut hits = 0;
        for y in 0..h {
            for x in 0..w {
                let ray = camera.ray(x, y, w, h, (0.5, 0.5));
                if world.trace(&ray, 1000.0, |_| true).is_some() {
                    hits += 1;
                }
            }
        }
        assert!(hits > w * h / 20, "island should cover part of the frame, hits={hits}");
    }

    #[test]
    fn the_island_stays_in_frame_through_a_full_orbit() {
        let world = terrain::generate(2024).world;
        let mut camera = scene_camera(&world);
        let (w, h) = (64usize, 48usize);
        for step in 0..8 {
            camera.yaw = std::f32::consts::TAU * step as f32 / 8.0;
            let mut hits = 0;
            for y in 0..h {
                for x in 0..w {
                    let ray = camera.ray(x, y, w, h, (0.5, 0.5));
                    if world.trace(&ray, 1000.0, |_| true).is_some() {
                        hits += 1;
                    }
                }
            }
            assert!(hits > w * h / 20, "island vanished at yaw step {step}");
        }
    }
}
