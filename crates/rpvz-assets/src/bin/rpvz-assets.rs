//! Command-line front end for the RPvZ asset installer.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use rpvz_assets::{
    ApkFileSource, AssetSource, DataDirs, DemoSource, Installer, Manifest, PcGotySource, Progress,
    Status, SteamSource,
};

#[derive(Parser)]
#[command(
    name = "rpvz-assets",
    version,
    about = "Install Plants vs. Zombies game data for RPvZ"
)]
struct Cli {
    /// Data directory (default: platform data dir, or $RPVZ_DATA_DIR).
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what is installed.
    Status,
    /// List sources found on this machine.
    Detect,
    /// Install or reinstall game data.
    Install {
        /// Where to get the data from.
        #[arg(long, value_enum, default_value_t = From::Auto)]
        from: From,
        /// Demo installer you already have (PlantsVsZombiesSetup-en_2009.exe).
        #[arg(long, value_name = "PATH")]
        demo_file: Option<PathBuf>,
        /// Android .apk/.xapk you already have.
        #[arg(long, value_name = "PATH")]
        apk_file: Option<PathBuf>,
        /// Don't ask before downloading.
        #[arg(short, long)]
        yes: bool,
        /// Reinstall even if a current install exists.
        #[arg(long)]
        force: bool,
    },
    /// Re-hash installed files against the manifest.
    Verify,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum From {
    /// Steam if found, otherwise the PC demo plus GOTY content from Android.
    Auto,
    /// An installed Steam copy (PC GOTY).
    Steam,
    /// PC demo (Internet Archive) plus GOTY content from the Android APK.
    Demo,
    /// Android data only (mobile layout).
    Android,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dirs = match cli.data_dir {
        Some(dir) => DataDirs::at(dir),
        None => DataDirs::from_env()?,
    };
    let installer = Installer::new(dirs);

    match cli.command {
        Command::Status => status(&installer),
        Command::Detect => {
            let found = rpvz_assets::detect();
            match &found.steam {
                Some(s) => println!("Steam:        {}", s.dir.display()),
                None => println!("Steam:        not found"),
            }
            println!(
                "Demo + GOTY:  available (download ~{} MB)",
                PcGotySource::default().download_size() / 1_000_000
            );
            Ok(())
        }
        Command::Install {
            from,
            demo_file,
            apk_file,
            yes,
            force,
        } => install(&installer, from, demo_file, apk_file, yes, force),
        Command::Verify => {
            let report = installer
                .verify(&mut printer())
                .context("nothing to verify; run `rpvz-assets install` first")?;
            finish_line();
            if report.is_ok() {
                println!("All {} files OK.", report.ok);
                Ok(())
            } else {
                for f in &report.missing {
                    println!("missing: {f}");
                }
                for f in &report.corrupt {
                    println!("changed: {f}");
                }
                bail!(
                    "{} missing, {} changed; run `rpvz-assets install --force` to repair",
                    report.missing.len(),
                    report.corrupt.len()
                )
            }
        }
    }
}

fn status(installer: &Installer) -> Result<()> {
    println!("Data dir: {}", installer.dirs().assets().display());
    match installer.status() {
        Status::NotInstalled => println!("Not installed."),
        Status::Broken(e) => println!("Broken install: {e}"),
        Status::Outdated(m) => {
            print_manifest(&m);
            println!("Layout is outdated; run `rpvz-assets install --force`.");
        }
        Status::Installed(m) => print_manifest(&m),
    }
    Ok(())
}

fn print_manifest(m: &Manifest) {
    println!("Source:   {}", m.source);
    println!("Layout:   {:?} (v{})", m.layout, m.layout_version);
    println!(
        "Files:    {} ({:.1} MB)",
        m.files.len(),
        m.total_size() as f64 / 1_000_000.0
    );
    println!("Digest:   {}", m.content_digest);
}

fn install(
    installer: &Installer,
    from: From,
    demo_file: Option<PathBuf>,
    apk_file: Option<PathBuf>,
    yes: bool,
    force: bool,
) -> Result<()> {
    if !force && let Status::Installed(m) = installer.status() {
        println!(
            "Already installed from {}. Use --force to reinstall.",
            m.source
        );
        return Ok(());
    }

    let steam = match from {
        From::Auto | From::Steam => rpvz_assets::detect().steam,
        _ => None,
    };
    let source: Box<dyn AssetSource> = match (from, steam) {
        (From::Auto | From::Steam, Some(install)) => Box::new(SteamSource { install }),
        (From::Steam, None) => bail!("no Steam install of Plants vs. Zombies found"),
        (From::Android, _) => {
            let path = apk_file.context("--from android needs --apk-file")?;
            Box::new(ApkFileSource::new(path))
        }
        (From::Auto | From::Demo, _) => {
            let src = PcGotySource {
                demo: DemoSource {
                    file: demo_file,
                    ..DemoSource::default()
                },
                apk_file,
                ..PcGotySource::default()
            };
            let size = src.download_size();
            if size > 0 && !yes && !confirm_download(size)? {
                bail!("cancelled");
            }
            Box::new(src)
        }
    };

    println!("Installing from {}", source.label());
    let manifest = installer.install(source.as_ref(), &mut printer());
    finish_line();
    let manifest = manifest?;
    println!(
        "Installed {} files ({:.1} MB) to {}",
        manifest.files.len(),
        manifest.total_size() as f64 / 1_000_000.0,
        installer.dirs().assets().display()
    );
    Ok(())
}

fn confirm_download(bytes: u64) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("no Steam copy found; pass --yes to download the PC demo and the Android data");
    }
    print!(
        "No Steam copy found. Download the free PC demo (Internet Archive) and the free Android \
         version (APKPure) for GOTY content, ~{} MB? [y/N] ",
        bytes / 1_000_000
    );
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

/// Progress printer that redraws one line at most ~10 times per second.
fn printer() -> impl FnMut(Progress) {
    let mut stage = "";
    let mut last = Instant::now() - Duration::from_secs(1);
    let tty = std::io::stderr().is_terminal();
    move |p| {
        let line = match p {
            Progress::Stage(s) => {
                stage = s.label();
                if !tty {
                    eprintln!("{stage}...");
                }
                last = Instant::now() - Duration::from_secs(1);
                format!("{stage}...")
            }
            Progress::Bytes { done, total } => match total {
                Some(t) => format!(
                    "{stage}: {:.1}/{:.1} MB ({:.0}%)",
                    done as f64 / 1e6,
                    t as f64 / 1e6,
                    p.fraction().unwrap_or(0.0) * 100.0
                ),
                None => format!("{stage}: {:.1} MB", done as f64 / 1e6),
            },
            Progress::Files { done, total } => format!("{stage}: {done}/{total} files"),
        };
        if tty && last.elapsed() >= Duration::from_millis(100) {
            last = Instant::now();
            eprint!("\r\x1b[2K{line}");
        }
    }
}

fn finish_line() {
    if std::io::stderr().is_terminal() {
        eprintln!();
    }
}
