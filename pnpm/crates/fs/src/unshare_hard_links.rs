//! Give a directory's files their own inodes before something rewrites them.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Replace every regular file under `dir` that shares its inode with
/// another path (a hard link into the content-addressable store) by a
/// private copy, so writes to it stay inside `dir`.
///
/// Files that are already private (one link) are left alone, so a tree
/// imported by clone or copy costs one directory walk. Symlinks are not
/// followed. Each copy is written next to its original and renamed over
/// it, so a crash leaves either the shared file or the private copy,
/// never a truncated one.
///
/// Returns how many files were copied.
///
/// # Errors
///
/// Fails on the first directory that can't be read or file that can't be
/// copied or renamed.
#[cfg(unix)]
pub fn unshare_hard_links(dir: &Path) -> io::Result<usize> {
    use std::os::unix::fs::MetadataExt;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut copied = 0;
    let mut pending: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if !file_type.is_file() || entry.metadata()?.nlink() <= 1 {
                continue;
            }
            let path = entry.path();
            let temp = current.join(format!(
                ".{}.pnpm-unshare-{}-{}",
                entry.file_name().to_string_lossy(),
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed),
            ));
            if let Err(error) = fs::copy(&path, &temp).and_then(|_| fs::rename(&temp, &path)) {
                let _ = fs::remove_file(&temp);
                return Err(error);
            }
            copied += 1;
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::unshare_hard_links;
    use std::fs;

    #[cfg(unix)]
    #[test]
    fn copies_shared_files_and_leaves_the_link_source_untouched() {
        let root = tempfile::tempdir().expect("create temp dir");
        let store_blob = root.path().join("blob");
        fs::write(&store_blob, "original").expect("write blob");
        let package = root.path().join("pkg");
        fs::create_dir_all(package.join("lib")).expect("create package dirs");
        fs::hard_link(&store_blob, package.join("lib/shared.js")).expect("hard link");
        fs::write(package.join("private.js"), "private").expect("write private file");

        assert_eq!(unshare_hard_links(&package).expect("unshare"), 1);

        fs::write(package.join("lib/shared.js"), "rewritten by a build script")
            .expect("rewrite unshared file");
        assert_eq!(fs::read_to_string(&store_blob).expect("read blob"), "original");
        assert_eq!(
            fs::read_dir(package.join("lib"))
                .expect("read lib")
                .count(),
            1
        );
    }
}
