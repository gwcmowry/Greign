//! Writable app locations and desktop integration, independent of the build host.
use std::path::{Path, PathBuf};

#[derive(serde::Deserialize)]
struct UprightGpuConfig {
    library: PathBuf,
    library_dirs: Vec<PathBuf>,
}

fn upright_gpu_config(root: &Path) -> Option<UprightGpuConfig> {
    if !cfg!(target_os = "linux") || std::env::var_os("GREIGN_CPU_UPRIGHT").is_some() {
        return None;
    }
    let bytes = std::fs::read(root.join(".greign/upright-gpu.json")).ok()?;
    let mut config: UprightGpuConfig = serde_json::from_slice(&bytes).ok()?;
    config.library = root.join(config.library);
    config.library_dirs = config
        .library_dirs
        .into_iter()
        .map(|p| root.join(p))
        .collect();
    config.library.is_file().then_some(config)
}

pub fn upright_gpu_library(root: &Path) -> Option<PathBuf> {
    upright_gpu_config(root).map(|config| config.library)
}

/// ELF resolves dependent CUDA libraries at process startup. Re-exec once,
/// before UI/worker threads, with only the installed local library paths added.
pub fn prepare_upright_gpu_loader(root: &Path) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let Some(config) = upright_gpu_config(root) else {
            return;
        };
        if std::env::var_os("GREIGN_UPRIGHT_GPU_LOADER_READY").is_some() {
            return;
        }
        let mut paths = config.library_dirs;
        if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let (Ok(paths), Ok(executable)) = (std::env::join_paths(paths), std::env::current_exe())
        {
            let error = std::process::Command::new(executable)
                .args(std::env::args_os().skip(1))
                .env("LD_LIBRARY_PATH", paths)
                .env("GREIGN_UPRIGHT_GPU_LOADER_READY", "1")
                .exec();
            eprintln!("GPU loader setup failed; CPU fallback remains available: {error}");
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = root;
}

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
