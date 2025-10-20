use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

pub fn gather_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();

    if inputs.is_empty() {
        for dir in ["src", "tests"] {
            let path = Path::new(dir);

            if path.exists() {
                files.extend(rust_files_in(path));
            }
        }
    } else {
        for entry in inputs {
            if entry.is_dir() {
                files.extend(rust_files_in(entry));
            } else if is_rust_file(entry) {
                files.insert(entry.clone());
            }
        }
    }

    Ok(files.into_iter().collect())
}

fn is_rust_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("rs")
}

fn rust_files_in(dir: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_rust_file(path))
}
