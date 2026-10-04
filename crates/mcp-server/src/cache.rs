//! The single owner of the per-workspace derived-cache directory.
//!
//! Every SQLite index the server builds from a workspace (the call graph and the
//! code-search index) lives under one root: `<workspace>/.build` by default, or the
//! directory `mcp serve --cache-dir` names. Centralising the path here keeps the
//! directory layout in one place instead of being reconstructed ad-hoc at each call
//! site. The directory is a rebuildable cache: it is safe to delete and is re-created
//! on demand.
//!
//! Workspace-independent caches (the platform reference-search database) live in the
//! user's OS cache directory, not here — see `state::reference_search_db_path`.

use std::path::{Path, PathBuf};

/// The lock serializing lease reads and writes. Defined here, not next to the lease code, so
/// the directory's file names have one definition and a rename cannot leave the layout behind.
pub(crate) const LEASE_LOCK_FILE: &str = "writer.lease.lock";

/// The one-shot artifact a wedged build leaves behind when daemon file logging is off.
pub(crate) const STALL_REPORT_FILE: &str = "bsl-graph-stall-report.txt";

/// Resolved locations of every cache derived from one workspace.
///
/// The source tree and the cache root are deliberately independent: callers may
/// keep the default `<workspace>/.build` layout or supply an external root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCacheLayout {
    root: PathBuf,
    /// The root as the caller spelled it, before canonicalisation.
    ///
    /// Kept because a file watcher reports events under the spelling its watch was
    /// armed with, and that is the pre-canonical one: on Windows `canonicalize`
    /// answers `\\?\C:\...` while the watcher says `C:\...`, and on any platform a
    /// symlinked root and its target are two names for the same directory. A caller
    /// that has to tell "is this event inside my own cache" needs both, and keeping
    /// only the canonical one is a filter that silently matches nothing.
    declared: PathBuf,
    /// The workspace whose derived state this cache holds, when the caller named one.
    ///
    /// The lease records it and refuses a claim of a cache whose record names ANOTHER
    /// workspace (github#272): one directory must not serve two configurations. It lives
    /// here rather than beside each claim so every claiming path keeps one signature — the
    /// layout already travels to all of them.
    workspace: Option<PathBuf>,
}

impl WorkspaceCacheLayout {
    /// The backwards-compatible lazy layout under `<workspace>/.build`.
    pub fn for_workspace(workspace_root: &Path) -> Self {
        let declared = workspace_root.join(".build");
        let root = std::fs::canonicalize(&declared).unwrap_or_else(|_| declared.clone());
        Self { root, declared, workspace: Some(workspace_root.to_path_buf()) }
    }

    /// A layout whose root has already been resolved by the caller.
    pub fn from_root(root: PathBuf) -> Self {
        Self { declared: root.clone(), root, workspace: None }
    }

    /// Resolve, create, and canonicalize an explicit `--cache-dir` value.
    pub fn prepare_explicit(path: &Path, current_dir: &Path) -> std::io::Result<Self> {
        let requested =
            if path.is_absolute() { path.to_path_buf() } else { current_dir.join(path) };
        std::fs::create_dir_all(&requested).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("failed to create --cache-dir {}: {error}", requested.display()),
            )
        })?;
        let root = requested.canonicalize().map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("failed to canonicalize --cache-dir {}: {error}", requested.display()),
            )
        })?;
        Ok(Self { root, declared: requested, workspace: None })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Name the workspace whose derived state this cache holds. See [`Self::workspace`].
    pub fn with_workspace(mut self, workspace: PathBuf) -> Self {
        self.workspace = Some(workspace);
        self
    }

    /// The workspace whose derived state this cache holds, when the caller named one.
    ///
    /// `None` on a layout built from a bare directory: the identity then stays unstated, and
    /// the lease's claim check has nothing to compare (an older program's records carry none
    /// either — see [`crate::workspace_lease`]).
    pub fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    /// Every spelling this cache root can appear under in a file-watcher event.
    ///
    /// Both, and deduplicated only by the caller: the two coincide whenever the path
    /// was already canonical, and differ exactly in the cases a single-spelling check
    /// gets wrong.
    pub fn spellings(&self) -> [&Path; 2] {
        [self.declared.as_path(), self.root.as_path()]
    }

    /// Every subtree of `workspace_root` that no pass may read as a source: this cache
    /// under both its spellings, plus the service directories that hold a version-control
    /// store, build output, or package-manager state — never BSL sources.
    ///
    /// Stated as PATHS, at the one place that knows the workspace root: narrowing a file
    /// universe by name is forbidden to every walk (`no_directory_is_excluded_from_the_walk`
    /// in `project_model`), so a caller states the subtrees it owns and the walks treat them
    /// as holes like any other. A root declared inside one of them still wins (`PathScope`).
    ///
    /// Stated whether or not they exist yet: under a flat layout the workspace root IS the
    /// watched scan root, and `.git` may be created — or re-created by `git init` — while
    /// the daemon runs; a hole added only for directories present at boot would miss exactly
    /// the burst the exclusion exists for.
    pub fn exclusions(&self, workspace_root: &Path) -> Vec<PathBuf> {
        let mut exclusions: Vec<PathBuf> =
            self.spellings().iter().map(|path| path.to_path_buf()).collect();
        for name in [".git", "target", "node_modules"] {
            let service = workspace_root.join(name);
            if !exclusions.iter().any(|exclusion| exclusion == &service) {
                exclusions.push(service);
            }
        }
        exclusions
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }

    pub fn graph_db_path(&self) -> PathBuf {
        self.root.join("bsl-graph.db")
    }

    pub fn search_db_path(&self) -> PathBuf {
        self.root.join("bsl-search.db")
    }

    pub fn lease_path(&self) -> PathBuf {
        self.root.join("writer.lease")
    }

    pub fn lease_lock_path(&self) -> PathBuf {
        self.root.join(LEASE_LOCK_FILE)
    }

    /// The one replacement a full build prepares next to the published graph, on the same
    /// volume, kept until it is installed or proven stale.
    pub(crate) fn graph_candidate_path(&self) -> PathBuf {
        self.root.join("bsl-graph.pending.db")
    }

    /// The file whose exclusive lock a builder holds while it prepares or installs the
    /// replacement: a superseded owner's build can outlive its hold on the graph itself.
    pub(crate) fn graph_candidate_lock_path(&self) -> PathBuf {
        self.root.join("bsl-graph.replacement.lock")
    }

    /// The file whose exclusive lock a process holds for as long as it may open the published
    /// graph. Never removed or renamed while in use.
    pub(crate) fn graph_access_lock_path(&self) -> PathBuf {
        self.root.join("bsl-graph.access.lock")
    }

    pub fn stall_report_path(&self) -> PathBuf {
        self.root.join(STALL_REPORT_FILE)
    }

    pub fn daemon_log_path(&self) -> PathBuf {
        self.root.join("bsl-analyzer-daemon.log")
    }
}

/// The per-workspace derived-cache directory (`<workspace>/.build`).
#[cfg(test)]
pub fn workspace_cache_dir(workspace_root: &Path) -> PathBuf {
    WorkspaceCacheLayout::for_workspace(workspace_root).root
}

/// Ensure the workspace cache directory exists, returning its path.
#[cfg(test)]
pub fn ensure_workspace_cache_dir(workspace_root: &Path) -> std::io::Result<PathBuf> {
    let layout = WorkspaceCacheLayout::for_workspace(workspace_root);
    layout.ensure()?;
    Ok(layout.root)
}

/// The call-graph SQLite index path under the workspace cache directory.
#[cfg(test)]
pub fn graph_db_path(workspace_root: &Path) -> PathBuf {
    WorkspaceCacheLayout::for_workspace(workspace_root).graph_db_path()
}

/// The code-search SQLite index path under the workspace cache directory.
#[cfg(test)]
pub fn search_db_path(workspace_root: &Path) -> PathBuf {
    WorkspaceCacheLayout::for_workspace(workspace_root).search_db_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_stays_under_workspace_build_without_creating_it() {
        let workspace = tempfile::tempdir().unwrap();

        let layout = WorkspaceCacheLayout::for_workspace(workspace.path());

        assert_eq!(layout.root(), workspace.path().join(".build"));
        assert!(!layout.root().exists(), "default construction stays lazy");
    }

    #[test]
    fn explicit_relative_cache_is_created_and_canonicalized() {
        let cwd = tempfile::tempdir().unwrap();

        let layout =
            WorkspaceCacheLayout::prepare_explicit(Path::new("кеш с пробелом"), cwd.path())
                .unwrap();

        assert_eq!(layout.root(), cwd.path().join("кеш с пробелом").canonicalize().unwrap());
    }

    #[test]
    fn layout_owns_every_workspace_cache_file_name() {
        let root = PathBuf::from("external-cache");
        let layout = WorkspaceCacheLayout::from_root(root.clone());

        assert_eq!(layout.graph_db_path(), root.join("bsl-graph.db"));
        assert_eq!(layout.search_db_path(), root.join("bsl-search.db"));
        assert_eq!(layout.lease_path(), root.join("writer.lease"));
        assert_eq!(layout.lease_lock_path(), root.join("writer.lease.lock"));
        assert_eq!(layout.stall_report_path(), root.join("bsl-graph-stall-report.txt"));
        assert_eq!(layout.daemon_log_path(), root.join("bsl-analyzer-daemon.log"));
    }

    /// The default layout names the workspace it serves; the other two leave the identity to
    /// the caller, who states it with `with_workspace`.
    #[test]
    fn the_layout_names_the_workspace_it_serves() {
        let workspace = tempfile::tempdir().unwrap();

        let default = WorkspaceCacheLayout::for_workspace(workspace.path());
        assert_eq!(default.workspace(), Some(workspace.path()));

        let explicit = WorkspaceCacheLayout::from_root(PathBuf::from("external-cache"));
        assert_eq!(explicit.workspace(), None);
        assert_eq!(
            explicit.with_workspace(workspace.path().to_path_buf()).workspace(),
            Some(workspace.path())
        );
    }

    /// The exclusion list is stated at the workspace root — and only there: the walk's
    /// policy keeps narrowing by name out, so a `target` deeper in the tree is nobody's
    /// to exclude.
    #[test]
    fn exclusions_state_the_cache_and_the_service_directories_of_the_root() {
        let workspace = tempfile::tempdir().unwrap();
        let layout = WorkspaceCacheLayout::for_workspace(workspace.path());

        let exclusions = layout.exclusions(workspace.path());

        for name in [".git", "target", "node_modules"] {
            assert!(
                exclusions.contains(&workspace.path().join(name)),
                "`{name}` of the workspace root is not stated"
            );
        }
        for spelling in layout.spellings() {
            assert!(
                exclusions.contains(&spelling.to_path_buf()),
                "the cache spelling {} is not stated",
                spelling.display()
            );
        }
        assert!(
            !exclusions.contains(&workspace.path().join("sub").join("target")),
            "a nested directory was excluded by name"
        );
    }
}
