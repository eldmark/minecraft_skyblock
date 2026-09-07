//! Skyblock diorama — a CPU raytracer.
//!
//! Modes:
//!   skyblock                       live window
//!   skyblock --render out/ -n 240  write an orbit as PNGs, no window
//!   skyblock --bench 60            time N frames, no window, no I/O

mod assets;
mod blocks;
mod camera;
mod daylight;
mod hud;
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

use camera::{Camera, Mode as CameraMode};
use math::{vec3, Vec3};
use output::{FileOutput, Framebuffer, NullOutput, Output};
use render::Renderer;
use scene::Scene;
use window::WindowOutput;
use world::World;

enum Mode {
    Window,
    Render { dir: PathBuf },
    Bench,
    /// Time the idle path: accumulate frames with a still camera.
    BenchIdle,
    /// Time the path taken while the camera is being dragged.
    BenchMove,
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
    /// Where in the day/night cycle to start: 0 sunrise, 0.25 noon, 0.5 sunset,
    /// 0.75 midnight.
    time: f32,
    /// Offline: sweep a full day across the rendered frames.
    cycle: bool,
    /// Offline: draw the hotbar overlay on the rendered frames too.
    hud: bool,
    /// Offline: render from this free-flight eye instead of orbiting.
    eye: Option<Vec3>,
    /// Yaw and pitch, in radians, for `--eye`.
    look: (f32, f32),
}

impl Args {
    fn parse() -> Args {
        let mut args = Args {
            mode: Mode::Window,
            hud: false,
            width: 900,
            height: 600,
            frames: 240,
            threads: None,
            seed: 2024,
            panorama_sky: false,
            samples: 1,
            time: 0.16,
            cycle: false,
            eye: None,
            look: (0.9, 0.2),
        };
        let mut argv = std::env::args().skip(1);
        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--window" => args.mode = Mode::Window,
                "--hud" => args.hud = true,
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
                "--bench-move" => {
                    args.mode = Mode::BenchMove;
                    if let Some(n) = argv.next().and_then(|v| v.parse().ok()) {
                        args.frames = n;
                    }
                }
                "--bench-idle" => {
                    args.mode = Mode::BenchIdle;
                    if let Some(n) = argv.next().and_then(|v| v.parse().ok()) {
                        args.frames = n;
                    }
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
                "--time" => {
                    if let Some(v) = argv.next().and_then(|v| v.parse::<f32>().ok()) {
                        args.time = v.rem_euclid(1.0);
                    }
                }
                "--cycle" => args.cycle = true,
                "--eye" => {
                    if let Some(v) = argv.next() {
                        let parts: Vec<f32> =
                            v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
                        if let [x, y, z] = parts[..] {
                            args.eye = Some(vec3(x, y, z));
                        }
                    }
                }
                "--look" => {
                    if let Some(v) = argv.next() {
                        let parts: Vec<f32> =
                            v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
                        if let [yaw, pitch] = parts[..] {
                            args.look = (yaw, pitch);
                        }
                    }
                }
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

  --window         live window (default): arrows or drag to turn, WASD to move,
                   F toggles free flight (Q/E up-down, Shift sprints),
                   Space runs the day/night cycle, R reseeds, P screenshots, Esc quits
  --render DIR     write an orbit as PNG frames, no window
  --bench N        render N frames and report ms/frame
  --bench-idle N   time N refinement frames with a still camera
  --bench-move N   time N frames of a camera being dragged (writes bench_move.png)
  --threads N      force the worker count (default: all cores)
  --seed N         terrain seed (R reseeds in the window)
  --sky-panorama   use the pack's panorama cubemap instead of the dusk sky
  --samples N      jittered samples per offline frame (antialiasing)
  --time T         time of day in [0,1): 0 sunrise, .25 noon, .5 sunset, .75 night
  --cycle          offline: sweep a full day/night cycle across the frames
  --hud            offline: composite the hotbar overlay onto the frames
  --eye X,Y,Z      offline: render from this point instead of orbiting
  --look YAW,PITCH radians, for --eye (default 0.9,0.2)
  --check-pack     decode textures from the resource pack and report findings
  --dump NAME OUT  decode one pack entry and write its raw RGBA bytes";

/// Load the resource pack and build the scene, with a clear message when the pack
/// is missing: it is not committed to the repository.
fn load_scene(seed: u32, panorama_sky: bool, time: f32) -> io::Result<Scene> {
    let pack = pack::Pack::open(None).map_err(io::Error::other)?;
    Scene::load(seed, &pack, panorama_sky, time).map_err(io::Error::other)
}

/// The camera an offline render uses: orbiting by default, or planted at a fixed
/// point when `--eye` is given, which is the same free-flight camera the window
/// drives with F.
fn offline_camera(world: &World, args: &Args) -> Camera {
    let mut camera = scene_camera(world);
    if let Some(eye) = args.eye {
        camera.set_mode(CameraMode::Free);
        camera.position = eye;
        camera.yaw = args.look.0;
        camera.pitch = args.look.1;
    }
    camera
}

fn scene_camera(world: &World) -> Camera {
    // Aim just below the shrine's floor so the island fills the frame with the
    // structure near the upper third, the way the reference diorama is framed.
    let center = vec3(
        world.size[0] as f32 * 0.5,
        terrain::SURFACE_LEVEL + 4.0,
        world.size[2] as f32 * 0.5,
    );
    Camera::new(center, world.size[0] as f32 * 1.9)
}

fn run_headless(mut out: Box<dyn Output>, args: &Args, report: bool) -> io::Result<()> {
    let mut scene = load_scene(args.seed, args.panorama_sky, args.time)?;
    let mut camera = offline_camera(scene.world(), args);
    let mut frame = Framebuffer::new(args.width, args.height);
    let mut renderer = Renderer::new();
    if let Some(threads) = args.threads {
        renderer.threads = threads;
    }
    // Offline the overlay can go straight onto the frame: nothing reads it back.
    let mut overlay = match args.hud {
        true => Some(hud::Hud::load(&pack::Pack::open(None).map_err(io::Error::other)?)
            .map_err(io::Error::other)?),
        false => None,
    };

    let start = Instant::now();
    let mut rendered = 0usize;
    loop {
        // A full turn over the requested frame count, so --render yields a loop.
        // With --eye the camera is planted instead, and only the clock may move.
        if args.eye.is_none() {
            camera.yaw = std::f32::consts::TAU * rendered as f32 / args.frames.max(1) as f32;
        }
        if args.cycle {
            // One full day across the sequence, so the clip shows both states.
            scene.set_time(args.time + rendered as f32 / args.frames.max(1) as f32);
        }
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
        if let Some(hud) = overlay.as_mut() {
            hud.draw(&mut frame, scene.time_of_day, camera.yaw);
        }
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

/// Time the path the window takes when nobody is touching anything: a still
/// camera folding one jittered sample per frame into the running average.
fn bench_idle(args: &Args) -> io::Result<()> {
    let mut scene = load_scene(args.seed, args.panorama_sky, args.time)?;
    let camera = scene_camera(scene.world());
    let mut frame = Framebuffer::new(args.width, args.height);
    let mut renderer = Renderer::new();
    if let Some(threads) = args.threads {
        renderer.threads = threads;
    }

    let pixels = args.width * args.height;
    let start = Instant::now();
    let mut traced_total = 0usize;
    for _ in 0..args.frames.max(1) {
        renderer.accumulate(&mut frame, &scene, &camera);
        scene.tick += 1;
        traced_total += renderer.last_traced;
    }
    let frames = args.frames.max(1);
    let ms = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!(
        "{frames} idle frames at {}x{}, {} threads: {ms:.2} ms/frame ({:.1} fps), \
tracing {:.0}% of the pixels",
        args.width,
        args.height,
        renderer.threads,
        1000.0 / ms,
        100.0 * traced_total as f64 / (frames * pixels) as f64
    );
    Ok(())
}

/// Time the path the window takes while the camera is being dragged, and leave the
/// last frame on disk so the result can be looked at, not just measured.
fn bench_move(args: &Args) -> io::Result<()> {
    let mut scene = load_scene(args.seed, args.panorama_sky, args.time)?;
    let mut camera = scene_camera(scene.world());
    let mut frame = Framebuffer::new(args.width, args.height);
    let mut renderer = Renderer::new();
    if let Some(threads) = args.threads {
        renderer.threads = threads;
    }

    let frames = args.frames.max(1);
    let pixels = args.width * args.height;
    // Prime the buffer, as a real drag does, then time the frames after it.
    renderer.render_moving(&mut frame, &scene, &camera);
    let start = Instant::now();
    let mut traced_total = 0usize;
    for i in 0..frames {
        camera.apply((6.0, 0.0), 0.0);
        renderer.render_moving(&mut frame, &scene, &camera);
        scene.tick += 1;
        traced_total += renderer.last_traced;
        let _ = i;
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!(
        "{frames} moving frames at {}x{}, {} threads: {ms:.2} ms/frame ({:.1} fps), \
tracing {:.0}% of the pixels",
        args.width,
        args.height,
        renderer.threads,
        1000.0 / ms,
        100.0 * traced_total as f64 / (frames * pixels) as f64
    );
    frame.save_png(std::path::Path::new("bench_move.png"))?;
    Ok(())
}

fn run_window(args: &Args) -> io::Result<()> {
    let mut scene = load_scene(args.seed, args.panorama_sky, args.time)?;
    // The overlay reads its own sprites out of the same pack the scene uses.
    let mut hud = hud::Hud::load(&pack::Pack::open(None).map_err(io::Error::other)?)
        .map_err(io::Error::other)?;
    let mut hud_visible = true;
    // The overlay is composited into a copy: the renderer keeps re-using the
    // frame it wrote, and painting a hotbar into it would poison the running
    // average and the pixels the refinement pass skips.
    let mut presented = Framebuffer::new(args.width, args.height);
    let mut camera = scene_camera(scene.world());
    let mut win = WindowOutput::new("Skyblock Diorama", args.width, args.height)?;
    let mut frame = Framebuffer::new(args.width, args.height);
    let mut renderer = Renderer::new();
    if let Some(threads) = args.threads {
        renderer.threads = threads;
    }
    let mut shots = 0usize;
    let mut quality = 1usize;
    let mut last_frame_seconds = 1.0 / 60.0;

    while win.is_open() {
        let frame_start = Instant::now();
        let input = win.poll_input();

        // The hotbar is the menu: a slot is chosen with the number row and used
        // with Enter, and the two time items keep working while it is held.
        if let Some(slot) = input.select_slot {
            if slot < hud.slot_count() {
                hud.select(slot);
                println!("hotbar {}: {}", slot + 1, hud.label());
            }
        }
        if input.toggle_hud {
            hud_visible = !hud_visible;
        }
        let mut fired = None;
        if input.use_item || (input.use_held && hud.action().repeats()) {
            fired = Some(hud.action());
        }

        let mut toggle_camera = input.toggle_free;
        let mut want_screenshot = input.screenshot;
        let mut want_reseed = input.reseed;
        let mut item_nudge = 0.0;
        match fired {
            Some(hud::Action::ToggleCamera) => toggle_camera = true,
            Some(hud::Action::TimeForward) => item_nudge = 0.004,
            Some(hud::Action::TimeBack) => item_nudge = -0.004,
            Some(hud::Action::Screenshot) => want_screenshot = true,
            Some(hud::Action::Reseed) => want_reseed = true,
            Some(hud::Action::Quit) => break,
            None => {}
        }

        if toggle_camera {
            let next = match camera.mode {
                CameraMode::Orbit => CameraMode::Free,
                CameraMode::Free => CameraMode::Orbit,
            };
            camera.set_mode(next);
            println!(
                "camera: {}",
                match next {
                    CameraMode::Orbit => "orbiting the island",
                    CameraMode::Free => "free flight (WASD, Q/E up-down, Shift to sprint)",
                }
            );
        }
        match camera.mode {
            // While orbiting, A and D swing the camera around the island the way
            // the arrows do, and W and S dolly in and out.
            CameraMode::Orbit => camera.apply(
                (input.orbit.0 + input.move_axes.1 * 6.0, input.orbit.1),
                input.zoom,
            ),
            CameraMode::Free => {
                camera.look(input.orbit);
                camera.fly(input.move_axes, last_frame_seconds, input.boost);
            }
        }

        if input.toggle_cycle {
            scene.toggle_cycle();
            println!(
                "day/night cycle {}",
                if scene.cycle_running { "running" } else { "paused" }
            );
        }
        let mut time_changed = false;
        let nudge = input.time_nudge + item_nudge;
        if nudge != 0.0 {
            time_changed |= scene.set_time(scene.time_of_day + nudge);
        }
        time_changed |= scene.advance_cycle(last_frame_seconds);

        if want_reseed {
            let seed = scene
                .island
                .seed
                .wrapping_mul(1664525)
                .wrapping_add(1013904223);
            scene.reseed(seed);
            hud.say(format!("semilla {seed}"));
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
        // A change of light is not a change of geometry: the picture stays, every
        // pixel is simply re-shaded at full resolution and folded into the average.
        if time_changed {
            renderer.mark_all_active();
        }
        let moving = !input.is_idle() || resized || want_reseed;
        if moving {
            // Full resolution, a quarter of the pixels per frame, the rest kept
            // from the frame before. Sharper than stretching a half-size image.
            if resized || want_reseed {
                renderer.invalidate_moving();
            }
            renderer.scale = quality;
            renderer.render_moving(&mut frame, &scene, &camera);
            renderer.reset_accumulation();
            scene.tick += 1;
        } else {
            // Still camera: keep folding jittered samples into the running average.
            // The animation keeps running, and the average tracks it instead of
            // being thrown away, which is what used to make the image blink.
            renderer.scale = quality;
            renderer.accumulate(&mut frame, &scene, &camera);
            scene.tick += 1;
        }

        // The screenshot is of the render, without the overlay on top of it.
        if want_screenshot {
            let path = PathBuf::from(format!("screenshot_{shots:03}.png"));
            frame.save_png(&path)?;
            hud.say(format!("{}", path.display()));
            println!("wrote {}", path.display());
            shots += 1;
        }

        let ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        last_frame_seconds = (ms / 1000.0) as f32;
        win.set_status(&format!(
            "{ms:.1} ms  {:.0} fps  {w}x{h}  {} threads  {} spp  {}  {} {}  [{}]",
            1000.0 / ms.max(0.001),
            renderer.threads,
            renderer.samples.max(1),
            match camera.mode {
                CameraMode::Orbit => "orbit (F: fly)",
                CameraMode::Free => "free flight (F: orbit)",
            },
            scene.clock(),
            if scene.cycle_running {
                "(space: running)"
            } else {
                "(space: paused)"
            },
            hud.label()
        ));
        let shown = if hud_visible {
            presented.resize(frame.width, frame.height);
            presented.pixels.copy_from_slice(&frame.pixels);
            hud.draw(&mut presented, scene.time_of_day, camera.yaw);
            &presented
        } else {
            &frame
        };
        if !win.present(shown)? {
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
        Mode::BenchIdle => bench_idle(&args),
        Mode::BenchMove => bench_move(&args),
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
