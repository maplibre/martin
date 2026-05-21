//! Recursively walk a directory tree for resource discovery.

use std::path::{Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

/// Recursively collects files under `dir` whose extension matches one of
/// `extensions` (case-sensitive, no leading dot)
///
/// Symlinks are followed so the per-file symlinks at the mount root resolve
/// to their real targets.
pub fn walk_files(dir: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>, walkdir::Error> {
    WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
    // k8s config maps use a layered symlink tree:
    // real files live in a `.2024_...` directory, `.data` is a symlink to that directory,
    // and each entry at the mount root is a symlink to `.data/<file>`.
    // Recursing into the bookkeeping dir would surface each file 3x
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e))
        .filter_map(|entry| match entry {
            Ok(e) if has_matching_extension(e.path(), extensions) && e.file_type().is_file() => {
                Some(Ok(e.into_path()))
            }
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .collect()
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.'))
}

fn has_matching_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.contains(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn k8s_configmap_symlinks_yield_each_file_once() {
        use std::fs::{create_dir, write};
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let real_dir = root.join("..2024_05_17_17_57_51.390489675");
        create_dir(&real_dir).unwrap();
        write(real_dir.join("foo.svg"), b"<svg/>").unwrap();
        write(real_dir.join("bar.svg"), b"<svg/>").unwrap();
        symlink("..2024_05_17_17_57_51.390489675", root.join("..data")).unwrap();
        symlink("..data/foo.svg", root.join("foo.svg")).unwrap();
        symlink("..data/bar.svg", root.join("bar.svg")).unwrap();

        let mut files = walk_files(root, &["svg"]).expect("walk_files");
        files.sort();
        assert_eq!(
            files,
            vec![root.join("bar.svg"), root.join("foo.svg")],
            "k8s ConfigMap symlinks must not produce duplicates or dotfile-prefixed paths"
        );
    }

    #[test]
    fn extension_filter_is_case_sensitive_and_exact() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.svg"), b"").unwrap();
        std::fs::write(root.join("b.SVG"), b"").unwrap();
        std::fs::write(root.join("c.svgx"), b"").unwrap();
        std::fs::write(root.join("d.txt"), b"").unwrap();

        let files = walk_files(root, &["svg"]).unwrap();
        assert_eq!(files, vec![root.join("a.svg")]);
    }

    #[test]
    fn recurses_into_visible_subdirectories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let nested = root.join("icons").join("logos");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("foo.svg"), b"").unwrap();

        let files = walk_files(root, &["svg"]).unwrap();
        assert_eq!(files, vec![nested.join("foo.svg")]);
    }
}
