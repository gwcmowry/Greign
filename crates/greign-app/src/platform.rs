//! Writable app locations and desktop integration, independent of the build host.
use std::path::{Path, PathBuf};

pub fn workspace_root() -> anyhow::Result<PathBuf> {
    let root = if let Some(path) = std::env::var_os("GREIGN_WORKSPACE") {
        PathBuf::from(path)
    } else {
        default_workspace()?
    };
    std::fs::create_dir_all(&root)?;
    Ok(root.canonicalize()?)
}

#[cfg(target_os = "macos")]
fn default_workspace() -> anyhow::Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/Greign"))
}

#[cfg(not(target_os = "macos"))]
fn default_workspace() -> anyhow::Result<PathBuf> {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if source.join("Cargo.toml").is_file() {
        return Ok(source);
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let data = std::env::var_os("XDG_DATA_HOME")
        .map_or_else(|| PathBuf::from(home).join(".local/share"), PathBuf::from);
    Ok(data.join("greign"))
}

pub fn export_directory(root: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    if std::env::var_os("GREIGN_WORKSPACE").is_none() {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Pictures/Greign");
        }
    }
    root.join("exports/film-rolls")
}

pub fn open_path(path: &Path) -> std::io::Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener).arg(path).spawn()?;
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn finder_launch_uses_writable_user_locations() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(
            default_workspace().unwrap(),
            home.join("Library/Application Support/Greign")
        );
        if std::env::var_os("GREIGN_WORKSPACE").is_none() {
            assert_eq!(
                export_directory(Path::new("/")),
                home.join("Pictures/Greign")
            );
        }
    }
}
