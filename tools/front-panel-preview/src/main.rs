use std::{
    convert::Infallible,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use image::{Rgb, RgbImage};

#[path = "../../../firmware/src/front_panel_scene.rs"]
mod front_panel_scene;

use front_panel_scene::{UiFocus, UiModel, UiPainter, UiVariant, UI_H, UI_W};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1))?;

    if !args.out_dir.is_absolute() {
        return Err("--out-dir must be an absolute path".into());
    }

    let frame_dir = args
        .out_dir
        .join(format!("variant-{}", args.variant.as_tag()))
        .join(format!("focus-{}", args.focus.as_tag()));
    fs::create_dir_all(&frame_dir).map_err(|e| format!("create output dir failed: {e}"))?;

    let mut framebuffer = FrameBuffer::new(UI_W as usize, UI_H as usize);
    let model = UiModel {
        focus: args.focus.into_scene(),
        touch_irq: args.focus.into_scene() == UiFocus::Touch,
        frame_no: args.frame_no,
    };

    front_panel_scene::render_frame(&mut framebuffer, &model, args.variant.into_scene())
        .map_err(|_| "render failed unexpectedly".to_string())?;

    let bin_path = frame_dir.join("framebuffer.bin");
    framebuffer
        .write_raw_le(&bin_path)
        .map_err(|e| format!("write framebuffer failed: {e}"))?;

    let png_path = frame_dir.join("preview.png");
    framebuffer
        .write_png(&png_path)
        .map_err(|e| format!("write preview png failed: {e}"))?;

    println!("framebuffer: {}", bin_path.display());
    println!("preview: {}", png_path.display());

    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum VariantArg {
    A,
    B,
    C,
    D,
}

impl VariantArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "a" => Ok(Self::A),
            "b" => Ok(Self::B),
            "c" => Ok(Self::C),
            "d" => Ok(Self::D),
            _ => Err(format!(
                "unsupported --variant value: {raw} (expected A|B|C|D)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            VariantArg::A => "A",
            VariantArg::B => "B",
            VariantArg::C => "C",
            VariantArg::D => "D",
        }
    }

    fn into_scene(self) -> UiVariant {
        match self {
            VariantArg::A => UiVariant::InstrumentA,
            VariantArg::B => UiVariant::InstrumentB,
            VariantArg::C => UiVariant::RetroC,
            VariantArg::D => UiVariant::InstrumentD,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum FocusArg {
    Idle,
    Up,
    Down,
    Left,
    Right,
    Center,
    Touch,
}

impl FocusArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "idle" => Ok(Self::Idle),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "center" => Ok(Self::Center),
            "touch" => Ok(Self::Touch),
            _ => Err(format!(
                "unsupported --focus value: {raw} (expected idle|up|down|left|right|center|touch)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            FocusArg::Idle => "idle",
            FocusArg::Up => "up",
            FocusArg::Down => "down",
            FocusArg::Left => "left",
            FocusArg::Right => "right",
            FocusArg::Center => "center",
            FocusArg::Touch => "touch",
        }
    }

    fn into_scene(self) -> UiFocus {
        match self {
            FocusArg::Idle => UiFocus::Idle,
            FocusArg::Up => UiFocus::Up,
            FocusArg::Down => UiFocus::Down,
            FocusArg::Left => UiFocus::Left,
            FocusArg::Right => UiFocus::Right,
            FocusArg::Center => UiFocus::Center,
            FocusArg::Touch => UiFocus::Touch,
        }
    }
}

#[derive(Debug)]
struct Args {
    variant: VariantArg,
    focus: FocusArg,
    out_dir: PathBuf,
    frame_no: u32,
}

impl Args {
    fn parse<I>(mut iter: I) -> Result<Self, String>
    where
        I: Iterator<Item = String>,
    {
        let mut variant: Option<VariantArg> = None;
        let mut focus: Option<FocusArg> = None;
        let mut out_dir: Option<PathBuf> = None;
        let mut frame_no: u32 = 0;

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--variant" => {
                    let value = iter.next().ok_or("missing value for --variant")?;
                    variant = Some(VariantArg::parse(&value)?);
                }
                "--focus" => {
                    let value = iter.next().ok_or("missing value for --focus")?;
                    focus = Some(FocusArg::parse(&value)?);
                }
                "--out-dir" => {
                    let value = iter.next().ok_or("missing value for --out-dir")?;
                    out_dir = Some(PathBuf::from(value));
                }
                "--frame-no" => {
                    let value = iter.next().ok_or("missing value for --frame-no")?;
                    frame_no = value
                        .parse::<u32>()
                        .map_err(|_| format!("invalid --frame-no value: {value}"))?;
                }
                "--help" | "-h" => {
                    return Err(help_text());
                }
                unknown => {
                    return Err(format!("unknown argument: {unknown}\n\n{}", help_text()));
                }
            }
        }

        let variant = variant.ok_or_else(|| format!("missing --variant\n\n{}", help_text()))?;
        let focus = focus.ok_or_else(|| format!("missing --focus\n\n{}", help_text()))?;
        let out_dir = out_dir.ok_or_else(|| format!("missing --out-dir\n\n{}", help_text()))?;

        Ok(Self {
            variant,
            focus,
            out_dir,
            frame_no,
        })
    }
}

fn help_text() -> String {
    [
        "Usage:",
        "  front-panel-preview --variant {A|B|C|D} --focus {idle|up|down|left|right|center|touch} --out-dir <ABS_PATH> [--frame-no <n>]",
        "",
        "Example:",
        "  cargo run --manifest-path tools/front-panel-preview/Cargo.toml -- --variant A --focus idle --out-dir /tmp/front-panel-preview",
    ]
    .join("\n")
}

struct FrameBuffer {
    width: usize,
    height: usize,
    pixels: Vec<u16>,
}

impl FrameBuffer {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    fn write_raw_le(&self, path: &Path) -> io::Result<()> {
        let mut file = fs::File::create(path)?;
        for pixel in &self.pixels {
            file.write_all(&pixel.to_le_bytes())?;
        }
        Ok(())
    }

    fn write_png(&self, path: &Path) -> io::Result<()> {
        let mut image = RgbImage::new(self.width as u32, self.height as u32);

        for (index, pixel) in self.pixels.iter().enumerate() {
            let x = (index % self.width) as u32;
            let y = (index / self.width) as u32;
            image.put_pixel(x, y, Rgb(rgb565_to_rgb888(*pixel)));
        }

        image.save(path).map_err(io::Error::other)
    }
}

impl UiPainter for FrameBuffer {
    type Error = Infallible;

    fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgb565: u16,
    ) -> Result<(), Self::Error> {
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = x0.saturating_add(w as usize).min(self.width);
        let y1 = y0.saturating_add(h as usize).min(self.height);

        for yy in y0..y1 {
            let row = yy * self.width;
            for xx in x0..x1 {
                self.pixels[row + xx] = rgb565;
            }
        }

        Ok(())
    }
}

fn rgb565_to_rgb888(raw: u16) -> [u8; 3] {
    let r = ((raw >> 11) & 0x1f) as u8;
    let g = ((raw >> 5) & 0x3f) as u8;
    let b = (raw & 0x1f) as u8;

    [
        (r as u16 * 255 / 31) as u8,
        (g as u16 * 255 / 63) as u8,
        (b as u16 * 255 / 31) as u8,
    ]
}
