//! Locating a Steam install of Plants vs. Zombies GOTY.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::vdf;

/// Steam app id of *Plants vs. Zombies: Game of the Year*.
pub const PVZ_APP_ID: u32 = 3590;
/// Folder name under `steamapps/common` when the app manifest is unreadable.
pub const PVZ_DEFAULT_DIR: &str = "Plants vs. Zombies";
pub const PAK_NAME: &str = "main.pak";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamInstall {
    /// Game directory, e.g. `.../steamapps/common/Plants vs. Zombies`.
    pub dir: PathBuf,
    pub pak: PathBuf,
}

/// Steam root directories to probe on this machine, most likely first.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.extend(unix_roots(
            &home,
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        ));
    }
    #[cfg(windows)]
    roots.extend(windows_roots());
    roots
}

fn unix_roots(home: &Path, xdg_data_home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(xdg) = xdg_data_home {
        roots.push(xdg.join("Steam"));
    }
    roots.extend([
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".steam/root"),
        home.join(".steam/debian-installation"),
        // Flatpak
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
        // Snap
        home.join("snap/steam/common/.local/share/Steam"),
        // macOS
        home.join("Library/Application Support/Steam"),
    ]);
    roots
}

#[cfg(windows)]
fn windows_roots() -> Vec<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let mut roots = Vec::new();
    let lookups = [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ];
    for (hive, key, value) in lookups {
        if let Ok(path) = RegKey::predef(hive)
            .open_subkey(key)
            .and_then(|k| k.get_value::<String, _>(value))
        {
            roots.push(PathBuf::from(path));
        }
    }
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(dir) = std::env::var_os(var) {
            roots.push(PathBuf::from(dir).join("Steam"));
        }
    }
    roots
}

/// All library folders known to a Steam root, including the root itself.
pub fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_path_buf()];
    for candidate in [
        root.join("steamapps/libraryfolders.vdf"),
        root.join("config/libraryfolders.vdf"),
    ] {
        let Ok(text) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        let Ok(doc) = vdf::parse(&text) else {
            continue;
        };
        let Some(folders) = doc.get("libraryfolders") else {
            continue;
        };
        for (_, folder) in folders.entries() {
            // Current format: { "path" "..." }. Very old format: "1" "D:\\Lib".
            let path = folder
                .get("path")
                .and_then(vdf::Value::as_str)
                .or_else(|| folder.as_str());
            if let Some(path) = path {
                libs.push(PathBuf::from(path));
            }
        }
    }
    libs
}

/// Game directory of `app_id` inside one library, if installed there.
pub fn app_dir(library: &Path, app_id: u32) -> Option<PathBuf> {
    let steamapps = library.join("steamapps");
    let manifest = steamapps.join(format!("appmanifest_{app_id}.acf"));
    let installdir = std::fs::read_to_string(manifest).ok().and_then(|text| {
        vdf::parse(&text)
            .ok()?
            .path(&["AppState", "installdir"])?
            .as_str()
            .map(str::to_owned)
    });
    let dir = steamapps
        .join("common")
        .join(installdir.as_deref().unwrap_or(PVZ_DEFAULT_DIR));
    dir.is_dir().then_some(dir)
}

/// Searches the given Steam roots (and their libraries) for PvZ with a `main.pak`.
pub fn find_pvz(roots: &[PathBuf]) -> Option<SteamInstall> {
    let mut seen = HashSet::new();
    for root in roots.iter().filter(|r| r.is_dir()) {
        for lib in library_folders(root) {
            // ~/.steam/steam is usually a symlink to ~/.local/share/Steam.
            let key = lib.canonicalize().unwrap_or_else(|_| lib.clone());
            if !seen.insert(key) {
                continue;
            }
            if let Some(dir) = app_dir(&lib, PVZ_APP_ID) {
                let pak = dir.join(PAK_NAME);
                if pak.is_file() {
                    return Some(SteamInstall { dir, pak });
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fake_game(lib: &Path, installdir: &str, with_manifest: bool) -> PathBuf {
        let steamapps = lib.join("steamapps");
        let dir = steamapps.join("common").join(installdir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(PAK_NAME), b"pak").unwrap();
        if with_manifest {
            fs::write(
                steamapps.join(format!("appmanifest_{PVZ_APP_ID}.acf")),
                format!("\"AppState\"\n{{\n\t\"appid\"\t\"3590\"\n\t\"installdir\"\t\"{installdir}\"\n}}\n"),
            )
            .unwrap();
        }
        dir
    }

    #[test]
    fn finds_game_in_secondary_library() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Steam");
        let lib2 = tmp.path().join("Games/SteamLibrary");
        fs::create_dir_all(root.join("steamapps")).unwrap();
        let game = fake_game(&lib2, "PvZ Custom", true);
        fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\"0\" {{ \"path\" \"{}\" }}\n\"1\" {{ \"path\" \"{}\" }}\n}}",
                root.display(),
                lib2.display()
            ),
        )
        .unwrap();

        let found = find_pvz(&[tmp.path().join("missing"), root]).unwrap();
        assert_eq!(found.dir, game);
        assert_eq!(found.pak, game.join(PAK_NAME));
    }

    #[test]
    fn falls_back_to_default_dir_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let game = fake_game(tmp.path(), PVZ_DEFAULT_DIR, false);
        assert_eq!(find_pvz(&[tmp.path().to_path_buf()]).unwrap().dir, game);
    }

    #[test]
    fn ignores_dir_without_pak() {
        let tmp = tempfile::tempdir().unwrap();
        let game = fake_game(tmp.path(), PVZ_DEFAULT_DIR, true);
        fs::remove_file(game.join(PAK_NAME)).unwrap();
        assert_eq!(find_pvz(&[tmp.path().to_path_buf()]), None);
    }

    #[test]
    fn unix_roots_include_flatpak_and_snap() {
        let roots = unix_roots(Path::new("/home/u"), None);
        assert!(roots.contains(&PathBuf::from(
            "/home/u/.var/app/com.valvesoftware.Steam/.local/share/Steam"
        )));
        assert!(roots.contains(&PathBuf::from(
            "/home/u/snap/steam/common/.local/share/Steam"
        )));
    }
}
