//! Opt-in compatibility checks for private Canon R7 photographs.

use std::path::{Path, PathBuf};

use greign_raw::{PreviewEncoding, RawDecoder, RawlerDecoder};

fn r7_files() -> Vec<PathBuf> {
    let Some(root) = std::env::var_os("GREIGN_PRIVATE_RAW_DIR") else {
        eprintln!("skipping private R7 corpus: GREIGN_PRIVATE_RAW_DIR is not set");
        return Vec::new();
    };
    let mut files: Vec<_> = std::fs::read_dir(root)
        .expect("GREIGN_PRIVATE_RAW_DIR must be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("cr3"))
        })
        .collect();
    files.sort();
    files
}

fn first_r7() -> Option<PathBuf> {
    r7_files().into_iter().find(|path| {
        RawlerDecoder
            .metadata(path)
            .is_ok_and(|metadata| metadata.model == "Canon EOS R7")
    })
}

#[test]
fn r7_metadata_and_preview_fast_paths_are_complete() {
    let Some(path) = first_r7() else {
        return;
    };
    let metadata = RawlerDecoder
        .metadata(&path)
        .expect("R7 metadata should decode");
    assert_eq!((metadata.width, metadata.height), (7128, 4732));
    assert_eq!(metadata.cfa_pattern.as_deref(), Some("RGGB"));
    assert!(metadata.black_levels.iter().all(|value| value.is_finite()));
    assert!(metadata.white_levels.iter().all(|value| value.is_finite()));
    assert!(metadata.white_balance.iter().all(|value| value.is_finite()));
    assert!(
        metadata
            .xyz_to_camera_d65
            .iter()
            .flatten()
            .any(|value| *value != 0.0)
    );

    let preview = RawlerDecoder
        .embedded_preview(&path)
        .expect("R7 embedded preview should extract");
    assert_eq!(preview.encoding, PreviewEncoding::Jpeg);
    assert!(preview.width >= 1600 && preview.height >= 1000);
    assert!(preview.bytes.starts_with(&[0xff, 0xd8]));
}

#[test]
fn r7_craw_mosaic_is_complete() {
    let Some(path) = r7_files().into_iter().find(|path| is_mode(path, "craw")) else {
        eprintln!("skipping C-RAW decode: private corpus contains no Canon R7 C-RAW");
        return;
    };
    let mosaic = RawlerDecoder
        .mosaic(&path)
        .expect("R7 C-RAW mosaic should decode");
    mosaic.validate().expect("R7 sample count should be exact");
    assert_eq!(mosaic.metadata.mode, "craw");
}

#[test]
fn r7_full_raw_mosaic_is_complete() {
    let Some(path) = r7_files().into_iter().find(|path| is_mode(path, "raw")) else {
        eprintln!("skipping full RAW decode: private corpus contains no Canon R7 full RAW");
        return;
    };
    let mosaic = RawlerDecoder
        .mosaic(&path)
        .expect("R7 full RAW mosaic should decode");
    mosaic.validate().expect("R7 sample count should be exact");
    assert_eq!(mosaic.metadata.mode, "raw");
}

fn is_mode(path: &Path, expected: &str) -> bool {
    RawlerDecoder
        .metadata(path)
        .is_ok_and(|metadata| metadata.model == "Canon EOS R7" && metadata.mode == expected)
}
