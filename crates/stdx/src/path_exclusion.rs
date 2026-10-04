//! Directories a user has taken out of the project, and the one test every walker,
//! watcher and loader asks about them.
//!
//! The list is a hard exclusion: nothing declared inside it — a source root, an
//! extension, an opened document — brings any part of it back. That is what separates
//! it from the soft holes a cache punches in a tree, where a root declared inside the
//! hole wins for itself.
//!
//! Every exclusion is held under all the spellings a path inside it can arrive under,
//! because the callers do not agree on one: a walk yields paths spelled after the root
//! it started from, a watcher reports the path it armed, and the file system resolves
//! both to a third. Matching is by whole components, never by string prefix, so
//! `.tmp2` is not inside `.tmp`.

use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

/// A set of excluded directories.
///
/// Equality and hashing look only at the declared spellings: the canonical ones are
/// derived from whatever the file system says at construction time, and a directory
/// created later must not make an unchanged configuration look changed.
#[derive(Debug, Clone, Default)]
pub struct ExcludedPaths {
    entries: Vec<Excluded>,
}

#[derive(Debug, Clone)]
struct Excluded {
    declared: PathBuf,
    spellings: Vec<PathBuf>,
}

impl ExcludedPaths {
    /// Builds the set from absolute paths.
    ///
    /// Each path is normalised lexically, so `a/../.tmp` names `.tmp` and cannot be used
    /// to slip past the rule, and resolved through the file system on a best-effort
    /// basis: a directory that does not exist yet is excluded under its declared
    /// spelling and under its nearest existing ancestor's resolved spelling with the
    /// rest appended — so an exclusion declared through a symlink still matches the
    /// resolved paths walks and loaders produce before the directory is created.
    pub fn new<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut entries: Vec<Excluded> = Vec::new();
        for path in paths {
            let declared = normalize_lexically(path.as_ref());
            if entries.iter().any(|entry| entry.declared == declared) {
                continue;
            }
            let mut spellings = vec![declared.clone()];
            if let Some(resolved) = resolve_as_far_as_it_goes(&declared) {
                push_unique(&mut spellings, resolved);
            }
            entries.push(Excluded { declared, spellings });
        }
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The excluded directories as they were declared (after lexical normalisation).
    pub fn declared(&self) -> impl Iterator<Item = &Path> + '_ {
        self.entries.iter().map(|entry| entry.declared.as_path())
    }

    /// A copy that also recognises each exclusion under the spelling a walk started at
    /// one of `roots` produces.
    ///
    /// A root reached through a symlink or a `..` yields entries that match neither the
    /// declared nor the resolved spelling of an exclusion inside it. Re-spelling the
    /// exclusion once per root costs one resolution per root; resolving every walked
    /// entry instead would cost one per file.
    pub fn respelled_under(&self, roots: &[PathBuf]) -> Self {
        let roots: Vec<(PathBuf, PathBuf)> = roots
            .iter()
            .map(|root| {
                let canonical = root
                    .canonicalize()
                    .map(simplify_verbatim)
                    .unwrap_or_else(|_| normalize_lexically(root));
                (root.clone(), canonical)
            })
            .collect();
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                let mut spellings = entry.spellings.clone();
                for (declared_root, canonical_root) in &roots {
                    for spelling in &entry.spellings {
                        if let Ok(relative) = spelling.strip_prefix(canonical_root) {
                            push_unique(&mut spellings, declared_root.join(relative));
                        }
                    }
                }
                Excluded { declared: entry.declared.clone(), spellings }
            })
            .collect();
        Self { entries }
    }

    /// Whether `path` is an excluded directory or lies anywhere beneath one.
    pub fn is_excluded(&self, path: &Path) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        if self.any_spelling(|spelling| is_within(path, spelling)) {
            return true;
        }
        has_dot_components(path) && {
            let normalized = normalize_lexically(path);
            self.any_spelling(|spelling| is_within(&normalized, spelling))
        }
    }

    /// [`Self::is_excluded`] that also asks under the spelling the file system resolves
    /// `path` to. For a single decision about a path that may be, or pass through, a
    /// symlink into an excluded directory — not for the per-entry test of a walk, which
    /// would pay a resolution per entry. A path that does not exist yet — a document
    /// opened before it is first saved — is resolved through its nearest existing
    /// ancestor, so a symlink alias still leads into the exclusion.
    pub fn is_excluded_resolved(&self, path: &Path) -> bool {
        self.is_excluded(path)
            || (!self.entries.is_empty()
                && resolve_as_far_as_it_goes(path)
                    .is_some_and(|resolved| self.is_excluded(&resolved)))
    }

    /// Whether a walk must not enter the directory at `path` it has just reached, or
    /// take the symlink at `path`, whatever it points to.
    ///
    /// Asks about the walked spelling and about the resolved one, so that a symlink
    /// aliasing an excluded directory or file — or one of their ancestors — does not
    /// lead the walk back in under a name no exclusion carries. The resolved spelling is derived from
    /// the parent's, recorded in `resolved`, so only the walk's start and symlinks cost
    /// a resolution; a walk that follows links must pass every directory it enters.
    pub fn prunes_walked_dir(
        &self,
        path: &Path,
        is_symlink: bool,
        resolved: &mut ResolvedDirs,
    ) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        if self.is_excluded(path) {
            return true;
        }
        let from_parent = || {
            let parent = resolved.0.get(path.parent()?)?;
            Some(parent.join(path.file_name()?))
        };
        let canonical = match (is_symlink, from_parent()) {
            (false, Some(canonical)) => canonical,
            _ => path.canonicalize().map(simplify_verbatim).unwrap_or_else(|_| path.to_path_buf()),
        };
        let excluded = self.is_excluded(&canonical);
        resolved.0.insert(path.to_path_buf(), canonical);
        excluded
    }

    /// [`Self::is_excluded_resolved`] for many paths in a row: the resolved spelling of
    /// each parent directory is taken once and kept in `resolved`, and only a path that
    /// is itself a symlink is resolved on its own. For bulk decisions over files that
    /// may have been reached through a symlink alias of an excluded directory.
    pub fn is_excluded_resolving(&self, path: &Path, resolved: &mut ResolvedDirs) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        if self.is_excluded(path) {
            return true;
        }
        let canonical = if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
        {
            path.canonicalize().ok().map(simplify_verbatim)
        } else {
            match (path.parent(), path.file_name()) {
                (Some(parent), Some(name)) => {
                    let parent = resolved.0.entry(parent.to_path_buf()).or_insert_with(|| {
                        resolve_as_far_as_it_goes(parent).unwrap_or_else(|| parent.to_path_buf())
                    });
                    Some(parent.join(name))
                }
                _ => None,
            }
        };
        canonical.is_some_and(|canonical| self.is_excluded(&canonical))
    }

    /// Whether some exclusion lies strictly beneath `dir`.
    ///
    /// The question a watcher asks before arming `dir` recursively: a recursive watch on
    /// such a directory would cover the exclusion too, including one that does not
    /// exist yet and is only created later.
    pub fn has_exclusion_below(&self, dir: &Path) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let below = |dir: &Path| {
            self.any_spelling(|spelling| is_within(spelling, dir) && !same_path(spelling, dir))
        };
        below(dir) || (has_dot_components(dir) && below(&normalize_lexically(dir)))
    }

    /// [`Self::has_exclusion_below`] that also asks under the spelling the file system
    /// resolves `dir` to: a symlink to an ancestor of an exclusion has the exclusion
    /// below it just as much, though no lexical comparison can tell.
    pub fn has_exclusion_below_resolved(&self, dir: &Path) -> bool {
        self.has_exclusion_below(dir)
            || (!self.entries.is_empty()
                && resolve_as_far_as_it_goes(dir)
                    .is_some_and(|resolved| self.has_exclusion_below(&resolved)))
    }

    /// The heap this set owns: the entries and every spelling of each.
    pub fn heap_bytes(&self) -> usize {
        crate::heap::vec_bytes::<Excluded>(self.entries.capacity())
            + self
                .entries
                .iter()
                .map(|entry| {
                    entry.declared.capacity()
                        + crate::heap::vec_bytes::<PathBuf>(entry.spellings.capacity())
                        + entry.spellings.iter().map(PathBuf::capacity).sum::<usize>()
                })
                .sum::<usize>()
    }

    fn any_spelling(&self, mut test: impl FnMut(&Path) -> bool) -> bool {
        self.entries.iter().any(|entry| entry.spellings.iter().any(|spelling| test(spelling)))
    }
}

/// The resolved spelling of each directory one walk has entered, for
/// [`ExcludedPaths::prunes_walked_dir`]. One per walk: entries are keyed by the walk's
/// own spelling of each directory.
#[derive(Debug, Default)]
pub struct ResolvedDirs(std::collections::HashMap<PathBuf, PathBuf>);

impl PartialEq for ExcludedPaths {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self.entries.iter().zip(&other.entries).all(|(a, b)| a.declared == b.declared)
    }
}

impl Eq for ExcludedPaths {}

impl Hash for ExcludedPaths {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.entries.len().hash(state);
        for entry in &self.entries {
            entry.declared.hash(state);
        }
    }
}

/// Resolves `.` and `..` without touching the file system. A `..` that would climb
/// above the root of an absolute path is dropped, the way the file system treats it.
pub fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let popped =
                    matches!(out.components().next_back(), Some(Component::Normal(_))) && out.pop();
                if !popped && !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path` with every link resolved, in the spelling walks and events use (no Windows
/// verbatim prefix).
pub fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize().map(simplify_verbatim)
}

/// [`resolve`] of the nearest existing ancestor of `path`, with the part that does not
/// exist yet appended; `None` when not even the root resolves.
///
/// The appended part is normalised lexically, `..` included: nothing in it exists, so
/// nothing in it can be a symlink that `..` would have to step back out of.
fn resolve_as_far_as_it_goes(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        if let Ok(resolved) = resolve(existing) {
            let mut out = resolved;
            for component in missing.iter().rev() {
                out.push(component);
            }
            return Some(normalize_lexically(&out));
        }
        missing.push(existing.components().next_back()?.as_os_str().to_os_string());
        existing = existing.parent()?;
    }
}

fn has_dot_components(path: &Path) -> bool {
    path.components().any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

fn push_unique(spellings: &mut Vec<PathBuf>, spelling: PathBuf) {
    if !spellings.contains(&spelling) {
        spellings.push(spelling);
    }
}

/// `canonicalize` on Windows answers with a verbatim `\\?\C:\...` path, which no
/// watcher event or walk entry is spelled with.
#[cfg(windows)]
fn simplify_verbatim(path: PathBuf) -> PathBuf {
    use std::path::Prefix;
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(letter) => {
                let mut out = PathBuf::from(format!("{}:\\", letter as char));
                for component in components {
                    if !matches!(component, Component::RootDir) {
                        out.push(component.as_os_str());
                    }
                }
                out
            }
            _ => path,
        },
        _ => path,
    }
}

#[cfg(not(windows))]
fn simplify_verbatim(path: PathBuf) -> PathBuf {
    path
}

/// Whether `path` is `prefix` or lies beneath it, compared by whole components. On
/// Windows the file system ignores case, so the comparison does too.
fn is_within(path: &Path, prefix: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut path = path.components();
        prefix.components().all(|expected| {
            path.next().is_some_and(|actual| {
                actual.as_os_str().to_string_lossy().to_lowercase()
                    == expected.as_os_str().to_string_lossy().to_lowercase()
            })
        })
    }
    #[cfg(not(windows))]
    {
        path.starts_with(prefix)
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    is_within(left, right) && left.components().count() == right.components().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute(name: &str) -> PathBuf {
        std::path::absolute(Path::new("target").join("path-exclusion-tests").join(name)).unwrap()
    }

    #[test]
    fn exclusions_are_component_scoped_and_normalized() {
        let root = absolute("component");
        let excluded = ExcludedPaths::new([root.join("a/../.tmp")]);

        assert!(excluded.is_excluded(&root.join(".tmp")));
        assert!(excluded.is_excluded(&root.join("./.tmp/child/Module.bsl")));
        assert!(!excluded.is_excluded(&root.join(".tmp2/Module.bsl")));
        assert!(!excluded.is_excluded(&root.join("allowed/Module.bsl")));
    }

    #[test]
    fn a_future_exclusion_already_narrows_its_existing_parent() {
        let root = absolute("future");
        let future = root.join("missing/deep/.tmp");
        let excluded = ExcludedPaths::new([future.clone()]);

        assert!(excluded.has_exclusion_below(&root));
        assert!(excluded.has_exclusion_below(&root.join("missing")));
        assert!(!excluded.has_exclusion_below(&future));
        assert!(excluded.is_excluded(&future.join("Module.bsl")));
        assert!(!excluded.is_excluded(&root.join("missing/deep/.tmp2/Module.bsl")));
    }

    #[test]
    fn declared_identity_does_not_change_when_a_path_can_be_resolved() {
        let root = absolute("identity");
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join(".tmp");
        let before = ExcludedPaths::new([path.clone()]);
        std::fs::create_dir_all(&path).unwrap();
        let after = ExcludedPaths::new([path.clone()]);

        assert_eq!(before, after);
        assert!(after.is_excluded_resolved(&path.join("Module.bsl")));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_alias_cannot_reenter_an_excluded_directory() {
        let root = absolute("symlink");
        let _ = std::fs::remove_dir_all(&root);
        let real = root.join("real");
        let excluded_dir = real.join(".tmp");
        std::fs::create_dir_all(&excluded_dir).unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let excluded = ExcludedPaths::new([excluded_dir.clone()]);

        assert!(excluded.is_excluded_resolved(&alias.join(".tmp/Module.bsl")));
        assert!(excluded.is_excluded_resolved(&alias.join(".tmp/missing/../StillHidden.bsl")));
        assert!(!excluded.is_excluded_resolved(&alias.join(".tmp2/Module.bsl")));
        assert!(!excluded.is_excluded_resolved(&alias.join("allowed/missing/../StillAllowed.bsl")));

        let declared_through_alias = ExcludedPaths::new([alias.join(".tmp")]);
        assert!(declared_through_alias.is_excluded_resolved(&real.join(".tmp/Module.bsl")));
        assert!(!declared_through_alias.is_excluded_resolved(&real.join(".tmp2/Module.bsl")));

        std::fs::remove_dir_all(&root).unwrap();
    }
}
