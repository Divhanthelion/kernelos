//! User-directed VFS forks (PLAN M7b).
//!
//! Branches are ephemeral RAM clones of trunk (`FileSystem::fork_ephemeral`).
//! Divergence comes from the user (different prompt / edits), not from
//! identical-prompt fanout. Only trunk + named restore points persist.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::agent::journal::PathState;
use crate::agent::restore::apply_trunk;
use crate::filesystem::FileSystem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceId {
    Trunk,
    Branch(String),
}

#[derive(Clone)]
pub struct Branch {
    pub id: String,
    pub name: String,
    pub fs: Rc<RefCell<FileSystem>>,
}

/// One path that differs between branch and trunk.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchDiff {
    pub path: String,
    pub trunk: PathState,
    pub branch: PathState,
}

impl Branch {
    pub fn from_trunk(trunk: &FileSystem, name: &str) -> Result<Self, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("branch name must not be empty".into());
        }
        let fs = trunk.fork_ephemeral()?;
        assert!(!fs.is_persistent());
        Ok(Self {
            id: format!("br-{}", now_ms()),
            name: name.to_string(),
            fs: Rc::new(RefCell::new(fs)),
        })
    }
}

/// Diff branch against current trunk (union of paths).
pub fn diff_against_trunk(branch: &FileSystem, trunk: &FileSystem) -> Vec<BranchDiff> {
    let mut paths: HashSet<String> = HashSet::new();
    paths.extend(branch.all_paths());
    paths.extend(trunk.all_paths());
    let mut paths: Vec<String> = paths.into_iter().collect();
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let t = PathState::capture(trunk, &path);
        let b = PathState::capture(branch, &path);
        if t != b {
            out.push(BranchDiff {
                path,
                trunk: t,
                branch: b,
            });
        }
    }
    out
}

/// Copy one path's branch state onto trunk (cherry-pick).
pub fn promote_path(
    trunk: &mut FileSystem,
    branch: &FileSystem,
    path: &str,
) -> Result<(), String> {
    let path = FileSystem::normalize_path(path);
    let state = PathState::capture(branch, &path);
    apply_trunk(
        trunk,
        &merge_one_path_into_trunk_snapshot(trunk, &path, state)?,
    )?;
    // apply_trunk replaces the whole tree with a snapshot — too heavy for one
    // path. Use restore_path directly instead.
    Err("internal: use promote_path_direct".into())
}

/// Promote a single path from branch → trunk.
pub fn promote_path_direct(
    trunk: &mut FileSystem,
    branch: &FileSystem,
    path: &str,
) -> Result<(), String> {
    use crate::agent::journal::restore_path;
    let path = FileSystem::normalize_path(path);
    let state = PathState::capture(branch, &path);
    restore_path(trunk, &path, &state)
}

/// Promote every differing path from branch → trunk (keep branch).
pub fn promote_all(trunk: &mut FileSystem, branch: &FileSystem) -> Result<usize, String> {
    let diffs = diff_against_trunk(branch, trunk);
    let n = diffs.len();
    // Apply deletes deepest-first, restores shortest-first — reuse apply via
    // building a full trunk-matching snapshot from branch for changed paths only
    // is error-prone. Just walk diffs:
    let mut deletes: Vec<&BranchDiff> = diffs
        .iter()
        .filter(|d| matches!(d.branch, PathState::Absent))
        .collect();
    deletes.sort_by_key(|d| std::cmp::Reverse(d.path.len()));
    for d in deletes {
        promote_path_direct(trunk, branch, &d.path)?;
    }
    let mut restores: Vec<&BranchDiff> = diffs
        .iter()
        .filter(|d| !matches!(d.branch, PathState::Absent))
        .collect();
    restores.sort_by_key(|d| d.path.len());
    for d in restores {
        promote_path_direct(trunk, branch, &d.path)?;
    }
    Ok(n)
}

fn merge_one_path_into_trunk_snapshot(
    _trunk: &FileSystem,
    _path: &str,
    _state: PathState,
) -> Result<Vec<(String, PathState)>, String> {
    unreachable!()
}

fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Prompt for a branch name; `None` if cancelled.
pub fn prompt_branch_name(default: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window()?;
        let name = window
            .prompt_with_message_and_default("Name this branch:", default)
            .ok()??;
        let name = name.trim().to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(default.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> FileSystem {
        let mut fs = FileSystem::default();
        let _ = fs.write_file("/home/documents/a.txt", "alpha");
        let _ = fs.write_file("/home/documents/b.txt", "bravo");
        fs
    }

    #[test]
    fn fork_is_ephemeral_and_isolated() {
        let trunk = seed();
        let branch = Branch::from_trunk(&trunk, "exp").unwrap();
        assert!(!branch.fs.borrow().is_persistent());

        branch
            .fs
            .borrow_mut()
            .write_file("/home/documents/a.txt", "BRANCH")
            .unwrap();
        assert_eq!(trunk.read_file("/home/documents/a.txt").unwrap(), "alpha");
        assert_eq!(
            branch.fs.borrow().read_file("/home/documents/a.txt").unwrap(),
            "BRANCH"
        );
    }

    #[test]
    fn diff_lists_changed_paths() {
        let trunk = seed();
        let branch = Branch::from_trunk(&trunk, "exp").unwrap();
        branch
            .fs
            .borrow_mut()
            .write_file("/home/documents/a.txt", "BRANCH")
            .unwrap();
        branch
            .fs
            .borrow_mut()
            .write_file("/home/documents/c.txt", "new")
            .unwrap();
        branch
            .fs
            .borrow_mut()
            .delete("/home/documents/b.txt", false)
            .unwrap();

        let diffs = diff_against_trunk(&branch.fs.borrow(), &trunk);
        let paths: Vec<_> = diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"/home/documents/a.txt"));
        assert!(paths.contains(&"/home/documents/b.txt"));
        assert!(paths.contains(&"/home/documents/c.txt"));
    }

    #[test]
    fn promote_all_updates_trunk() {
        let mut trunk = seed();
        let branch = Branch::from_trunk(&trunk, "exp").unwrap();
        branch
            .fs
            .borrow_mut()
            .write_file("/home/documents/a.txt", "BRANCH")
            .unwrap();
        branch
            .fs
            .borrow_mut()
            .delete("/home/documents/b.txt", false)
            .unwrap();

        let n = promote_all(&mut trunk, &branch.fs.borrow()).unwrap();
        assert!(n >= 2);
        assert_eq!(trunk.read_file("/home/documents/a.txt").unwrap(), "BRANCH");
        assert!(!trunk.exists("/home/documents/b.txt"));
    }

    #[test]
    fn promote_path_cherry_picks() {
        let mut trunk = seed();
        let branch = Branch::from_trunk(&trunk, "exp").unwrap();
        branch
            .fs
            .borrow_mut()
            .write_file("/home/documents/a.txt", "A")
            .unwrap();
        branch
            .fs
            .borrow_mut()
            .write_file("/home/documents/b.txt", "B")
            .unwrap();

        promote_path_direct(&mut trunk, &branch.fs.borrow(), "/home/documents/a.txt").unwrap();
        assert_eq!(trunk.read_file("/home/documents/a.txt").unwrap(), "A");
        assert_eq!(trunk.read_file("/home/documents/b.txt").unwrap(), "bravo");
    }

    #[test]
    fn empty_branch_name_rejected() {
        let trunk = seed();
        assert!(Branch::from_trunk(&trunk, "  ").is_err());
    }
}
