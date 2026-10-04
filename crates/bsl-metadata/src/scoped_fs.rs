//! The file system as the metadata loaders may read it: the real one, less the
//! directories the user took out of the project.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bsl_conventions::{DirTree, EntryKind, RealFs, TreeEntry};
use stdx::path_exclusion::ExcludedPaths;

/// [`RealFs`] that never lists, probes or reports anything inside an excluded
/// directory. An excluded directory looks absent; its parent lists without it.
///
/// A directory is also asked about under its resolved spelling, once per listing, so a
/// symlink alias of an excluded directory is just as absent.
#[derive(Clone, Copy)]
pub struct ScopedFs<'a> {
    excluded: &'a ExcludedPaths,
}

impl<'a> ScopedFs<'a> {
    pub fn new(excluded: &'a ExcludedPaths) -> Self {
        Self { excluded }
    }

    /// Whether `path` is excluded under its own spelling or the one it resolves to.
    pub fn is_excluded(&self, path: &Path) -> bool {
        self.excluded.is_excluded_resolved(path)
    }

    /// `std::fs::read_dir` over what may be read: an excluded directory reads as absent,
    /// and excluded children are left out.
    pub fn read_dir(
        &self,
        dir: &Path,
    ) -> std::io::Result<impl Iterator<Item = std::io::Result<std::fs::DirEntry>> + 'a> {
        if self.is_excluded(dir) {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        let resolved = self.resolved_dir(dir);
        let excluded = self.excluded;
        Ok(std::fs::read_dir(dir)?.filter(move |entry| {
            entry.as_ref().map_or(true, |entry| {
                !child_excluded(excluded, &entry.path(), resolved.as_deref(), &entry.file_name())
            })
        }))
    }

    /// The resolved spelling of `dir` when it differs from the declared one — the second
    /// name its children can be excluded under.
    fn resolved_dir(&self, dir: &Path) -> Option<PathBuf> {
        if self.excluded.is_empty() {
            return None;
        }
        stdx::path_exclusion::resolve(dir).ok().filter(|resolved| resolved != dir)
    }
}

fn child_excluded(
    excluded: &ExcludedPaths,
    path: &Path,
    resolved_parent: Option<&Path>,
    name: &std::ffi::OsStr,
) -> bool {
    if excluded.is_empty() {
        return false;
    }
    excluded.is_excluded(path)
        || resolved_parent.is_some_and(|parent| excluded.is_excluded(&parent.join(name)))
        || (std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
            && excluded.is_excluded_resolved(path))
}

impl DirTree for ScopedFs<'_> {
    fn kind_of(&self, path: &Path) -> Option<EntryKind> {
        if self.is_excluded(path) {
            return None;
        }
        RealFs.kind_of(path)
    }

    fn entries(&self, dir: &Path) -> Vec<TreeEntry> {
        if self.is_excluded(dir) {
            return Vec::new();
        }
        let resolved = self.resolved_dir(dir);
        RealFs
            .entries(dir)
            .into_iter()
            .filter(|entry| {
                let name = entry.path.file_name().unwrap_or_default();
                !child_excluded(self.excluded, &entry.path, resolved.as_deref(), name)
            })
            .collect()
    }

    fn child_names(&self, dir: &Path) -> Vec<OsString> {
        if self.is_excluded(dir) {
            return Vec::new();
        }
        let resolved = self.resolved_dir(dir);
        RealFs
            .child_names(dir)
            .into_iter()
            .filter(|name| {
                !child_excluded(self.excluded, &dir.join(name), resolved.as_deref(), name)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_metadata_is_absent_from_every_filesystem_projection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let hidden = root.join("Catalogs/Hidden");
        let visible = root.join("Catalogs/Visible");
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::create_dir_all(&visible).unwrap();
        std::fs::write(hidden.join("Hidden.xml"), "secret").unwrap();
        std::fs::write(visible.join("Visible.xml"), "public").unwrap();
        let excluded = ExcludedPaths::new([hidden.clone()]);
        let fs = ScopedFs::new(&excluded);

        let names = fs.child_names(&root.join("Catalogs"));
        assert!(!names.iter().any(|name| name == "Hidden"));
        assert!(names.iter().any(|name| name == "Visible"));
        assert!(fs.entries(&hidden).is_empty());
        assert!(fs.kind_of(&hidden.join("Hidden.xml")).is_none());
        assert_eq!(
            fs.read_dir(&hidden).err().expect("excluded directory must be absent").kind(),
            std::io::ErrorKind::NotFound
        );

        let visible_files: Vec<_> =
            fs.read_dir(&visible).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(visible_files, [OsString::from("Visible.xml")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_metadata_symlink_cannot_alias_an_excluded_directory() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let hidden = real.join("Hidden");
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::write(hidden.join("Object.xml"), "secret").unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let excluded = ExcludedPaths::new([hidden]);
        let fs = ScopedFs::new(&excluded);

        assert!(fs.child_names(&alias).iter().all(|name| name != "Hidden"));
        assert!(fs.kind_of(&alias.join("Hidden/Object.xml")).is_none());
    }
}
