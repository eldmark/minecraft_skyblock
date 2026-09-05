//! Skyblock diorama — a CPU raytracer.
//!
//! Modes:
//!   skyblock                       live window
//!   skyblock --render out/ -n 240  write an orbit as PNGs, no window
//!   skyblock --bench 60            time N frames, no window, no I/O

mod inflate;
mod math;
mod pack;
mod output;
mod png;
mod texture;
mod window;
mod zip;

use std::io;
use std::path::PathBuf;
use std::time::Instant;

use math::{to_srgb_u32, vec3};
use output::{FileOutput, Framebuffer, NullOutput, Output};
use window::WindowOutput;

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
}

impl Args {
    fn parse() -> Args {
        let mut args = Args {
            mode: Mode::Window,
            width: 900,
            height: 600,
            frames: 240,
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
                    println!("{}", USAGE);
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
  --check-pack     decode textures from the resource pack and report findings
  --dump NAME OUT  decode one pack entry and write its raw RGBA bytes";

/// Placeholder scene for phase 0: a sky gradient, so the whole pipeline
/// (render -> framebuffer -> window/PNG) can be verified before any tracing exists.
fn render_frame(frame: &mut Framebuffer, time: f32) {
    let horizon = vec3(0.94, 0.65, 0.45);
    let zenith = vec3(0.10, 0.16, 0.38);
    let (w, h) = (frame.width, frame.height);
    for y in 0..h {
        let t = y as f32 / h.max(1) as f32;
        let sky = zenith.lerp(horizon, t.powf(2.2));
        for x in 0..w {
            let sweep = ((x as f32 / w as f32) * 6.283 + time).sin() * 0.02;
            frame.pixels[y * w + x] = to_srgb_u32(sky + vec3(sweep, sweep * 0.5, 0.0));
        }
    }
}

fn run_headless(mut out: Box<dyn Output>, args: &Args, report: bool) -> io::Result<()> {
    let mut frame = Framebuffer::new(args.width, args.height);
    let start = Instant::now();
    let mut rendered = 0usize;
    loop {
        render_frame(&mut frame, rendered as f32 * 0.05);
        rendered += 1;
        if !out.present(&frame)? {
            break;
        }
    }
    if report {
        let ms = start.elapsed().as_secs_f64() * 1000.0 / rendered as f64;
        println!(
            "{rendered} frames at {}x{}: {ms:.2} ms/frame ({:.1} fps)",
            args.width,
            args.height,
            1000.0 / ms
        );
    }
    Ok(())
}

fn run_window(args: &Args) -> io::Result<()> {
    let mut win = WindowOutput::new("Skyblock Diorama", args.width, args.height)?;
    let mut frame = Framebuffer::new(args.width, args.height);
    let mut shots = 0usize;
    let mut time = 0.0f32;
    while win.is_open() {
        let frame_start = Instant::now();
        let input = win.poll_input();
        let (w, h) = win.size();
        frame.resize(w, h);

        time += 0.016;
        render_frame(&mut frame, time);

        if input.screenshot {
            let path = PathBuf::from(format!("screenshot_{shots:03}.png"));
            frame.save_png(&path)?;
            println!("wrote {}", path.display());
            shots += 1;
        }

        let ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        win.set_status(&format!("{ms:.1} ms  {:.0} fps  {w}x{h}", 1000.0 / ms.max(0.001)));
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
        Mode::CheckPack { dump } => check_pack(dump.as_ref()),
        Mode::Bench => run_headless(Box::new(NullOutput::new(args.frames)), &args, true),
    }
}
