//! Command-line options.

use std::path::PathBuf;

use bevy::prelude::*;

#[derive(Resource, Debug, Clone, Default)]
pub struct Config {
    /// Installed assets dir (holds `manifest.json`, `pc/`, `goty/`).
    pub data: Option<PathBuf>,
    /// Use a placeholder pack (as written by `--make-devpack`) instead.
    pub devpack: Option<PathBuf>,
    /// Draw every reanim track as an outlined, labelled box instead of its
    /// image. Geometry is real, art is not loaded.
    pub boxes: bool,
    /// Show track names on top of the scene.
    pub labels: bool,
    pub automation: Automation,
}

/// Scripted runs for screenshots and tests. When any of these is set, time
/// advances by a fixed 1/60 s per frame so results don't depend on speed.
#[derive(Debug, Clone, Default)]
pub struct Automation {
    pub screenshot: Option<PathBuf>,
    /// Simulated seconds after which the screenshot is taken.
    pub at: f32,
    /// Fake cursor position in game coordinates (800x600, y down).
    pub cursor: Option<Vec2>,
    /// Simulated second at which a left click happens.
    pub click_at: Option<f32>,
    /// Keys typed at a simulated second; `{enter}`, `{esc}` and `{bs}` are
    /// special keys, everything else is text.
    pub keys: Vec<(f32, String)>,
}

impl Automation {
    pub fn active(&self) -> bool {
        self.screenshot.is_some()
            || self.cursor.is_some()
            || self.click_at.is_some()
            || !self.keys.is_empty()
    }
}

pub enum Command {
    Run(Config),
    MakeDevpack(PathBuf),
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut cfg = Config::default();
    let mut it = args.into_iter().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--data" => cfg.data = Some(value("--data")?.into()),
            "--devpack" => cfg.devpack = Some(value("--devpack")?.into()),
            "--make-devpack" => return Ok(Command::MakeDevpack(value("--make-devpack")?.into())),
            "--boxes" => cfg.boxes = true,
            "--labels" => cfg.labels = true,
            "--screenshot" => cfg.automation.screenshot = Some(value("--screenshot")?.into()),
            "--at" => cfg.automation.at = parse_f32(&value("--at")?)?,
            "--click-at" => cfg.automation.click_at = Some(parse_f32(&value("--click-at")?)?),
            "--type-at" => {
                let at = parse_f32(&value("--type-at")?)?;
                let text = value("--type-at")?;
                cfg.automation.keys.push((at, text));
            }
            "--cursor" => {
                let v = value("--cursor")?;
                let (x, y) = v.split_once(',').ok_or("--cursor takes X,Y")?;
                cfg.automation.cursor = Some(Vec2::new(parse_f32(x)?, parse_f32(y)?));
            }
            "-h" | "--help" => return Err(HELP.into()),
            other => return Err(format!("unknown option {other}\n\n{HELP}")),
        }
    }
    Ok(Command::Run(cfg))
}

fn parse_f32(s: &str) -> Result<f32, String> {
    s.trim().parse().map_err(|_| format!("not a number: {s}"))
}

const HELP: &str = "rpvz [options]
  --data DIR           installed assets (default: rpvz-assets location)
  --devpack DIR        use a placeholder pack instead of game data
  --make-devpack DIR   write a placeholder pack and exit
  --boxes              draw tracks as labelled boxes, no art
  --labels             show track names
  --screenshot FILE    save a screenshot and exit
  --at SECONDS         simulated time of the screenshot (default 0)
  --cursor X,Y         fake cursor position (game coordinates)
  --click-at SECONDS   simulated left click
  --type-at SECONDS T  type T at that time ({enter}, {esc}, {bs} are keys)";
