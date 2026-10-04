// SPDX-License-Identifier: MPL-2.0
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use ignore::{Walk as IgnoreWalk, WalkBuilder};

/// Directories Scorecard leaves out by default. Dot-directories are also
/// skipped, except the generated and dependency directories listed here can
/// be included with `scope.include_generated`.
pub const GENERATED_SKIP_DIRS: &[&str] = &[
    "target",
    "dist",
    "build",
    "out",
    "cdk.out",
    "coverage",
    "generated",
    "vendor",
    ".yarn",
    ".cache",
    "node_modules",
    "__pycache__",
    ".next",
    ".nuxt",
    ".output",
    ".turbo",
    ".parcel-cache",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".venv",
    "venv",
    ".gradle",
    "bin",
    "obj",
    "Pods",
    "Carthage",
    "bower_components",
    "storybook-static",
];

const ALWAYS_SKIP_DIRS: &[&str] = &[".git", ".hg", ".svn", ".sc"];
const GENERATED_HEADER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkEntry {
    pub path: PathBuf,
    pub kind: WalkKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkKind {
    Directory,
    File,
    Other,
}

#[derive(Debug)]
pub enum WalkItem {
    Entry(WalkEntry),
    Error {
        path: Option<PathBuf>,
        message: String,
    },
}

pub struct Walk {
    root: PathBuf,
    includes: Vec<String>,
    inner: IgnoreWalk,
}

/// Controls which directory names a project walk includes.
#[derive(Debug, Clone, Copy, Default)]
pub struct WalkOptions {
    /// Include dot-directories other than `.git`, `.hg`, `.svn`, and `.sc`.
    pub include_hidden_directories: bool,
    /// Match `directory/**` exclusions from the project root only.
    pub root_directory_excludes: bool,
}

/// Walk a project subtree with the shared defaults, nested `.gitignore`
/// handling, and the user's additional exclusions.
pub fn walk(
    root: &Path,
    start: &Path,
    exclude: &[String],
    include_generated: &[String],
    max_depth: Option<usize>,
) -> Walk {
    walk_with_options(
        root,
        start,
        exclude,
        include_generated,
        max_depth,
        WalkOptions::default(),
    )
}

/// Walk a project subtree with custom directory inclusion options.
pub fn walk_with_options(
    root: &Path,
    start: &Path,
    exclude: &[String],
    include_generated: &[String],
    max_depth: Option<usize>,
    options: WalkOptions,
) -> Walk {
    let root = root.to_path_buf();
    let start = start.to_path_buf();
    let excludes = exclude.to_vec();
    let includes = include_generated.to_vec();
    let mut builder = WalkBuilder::new(&start);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        // Machine-wide ignores must not change a project's scan results.
        .git_global(false)
        .ignore(true)
        .parents(true)
        // Standalone project roots can still use `.gitignore` files.
        .require_git(false)
        .follow_links(false)
        .max_depth(max_depth);
    let filter_root = root.clone();
    let filter_excludes = excludes.clone();
    let filter_includes = includes.clone();
    let include_hidden_directories = options.include_hidden_directories;
    let root_directory_excludes = options.root_directory_excludes;
    builder.filter_entry(move |entry| {
        let rel = relative_path(&filter_root, entry.path());
        if rel.is_empty() {
            return true;
        }
        if is_excluded(&rel, &filter_excludes, root_directory_excludes) {
            return false;
        }
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
        !is_dir || !skip_directory(&rel, &filter_includes, include_hidden_directories)
    });
    Walk {
        root,
        includes,
        inner: builder.build(),
    }
}

/// Return regular files from a shared project walk.
pub fn walk_files(
    root: &Path,
    start: &Path,
    exclude: &[String],
    include_generated: &[String],
    max_depth: Option<usize>,
) -> Vec<PathBuf> {
    walk(root, start, exclude, include_generated, max_depth)
        .filter_map(|item| match item {
            WalkItem::Entry(entry) if entry.kind == WalkKind::File => Some(entry.path),
            _ => None,
        })
        .collect()
}

/// Keep explicit file paths only when a project walk would include them.
pub fn filter_paths(
    root: &Path,
    paths: &[String],
    exclude: &[String],
    include_generated: &[String],
) -> Vec<String> {
    let parents: std::collections::BTreeSet<PathBuf> = paths
        .iter()
        .filter_map(|rel| Path::new(rel).parent().map(Path::to_path_buf))
        .collect();
    let mut scanned = std::collections::HashSet::new();
    for parent in parents {
        for path in walk_files(
            root,
            &root.join(parent),
            exclude,
            include_generated,
            Some(1),
        ) {
            if let Ok(rel) = path.strip_prefix(root) {
                scanned.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    paths
        .iter()
        .filter(|rel| scanned.contains(*rel))
        .cloned()
        .collect()
}

/// Find generated or opted-in generated-directory files among paths the
/// scanner actually analyzed.
pub fn analyzed_generated_files(
    root: &Path,
    paths: &[String],
    include_generated: &[String],
) -> Vec<String> {
    let mut generated: Vec<String> = paths
        .iter()
        .filter(|rel| {
            is_generated_file(&root.join(rel))
                || included_generated_directory(rel, include_generated)
        })
        .cloned()
        .collect();
    generated.sort();
    generated.dedup();
    generated
}

impl Iterator for Walk {
    type Item = WalkItem;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let item = self.inner.next()?;
            match item {
                Ok(entry) => {
                    let path = entry.path().to_path_buf();
                    let rel = relative_path(&self.root, &path);
                    let Some(file_type) = entry.file_type() else {
                        continue;
                    };
                    let kind = if file_type.is_dir() {
                        WalkKind::Directory
                    } else if file_type.is_file() {
                        if !included_path(&rel, &self.includes) && is_generated_file(&path) {
                            continue;
                        }
                        WalkKind::File
                    } else {
                        WalkKind::Other
                    };
                    return Some(WalkItem::Entry(WalkEntry { path, kind }));
                }
                Err(err) => {
                    return Some(WalkItem::Error {
                        path: error_path(&err),
                        message: err.to_string(),
                    });
                }
            }
        }
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn error_path(error: &ignore::Error) -> Option<PathBuf> {
    match error {
        ignore::Error::WithPath { path, .. } => Some(path.clone()),
        ignore::Error::Partial(errors) => errors.iter().find_map(error_path),
        ignore::Error::WithLineNumber { err, .. } | ignore::Error::WithDepth { err, .. } => {
            error_path(err)
        }
        _ => None,
    }
}

fn is_excluded(rel: &str, excludes: &[String], root_directory_excludes: bool) -> bool {
    if !root_directory_excludes {
        return crate::is_excluded(rel, excludes);
    }
    excludes.iter().any(|pattern| {
        let pattern = pattern.trim().trim_start_matches("./");
        if let Some(body) = pattern.strip_suffix("/**") {
            if !body.starts_with("**/") {
                let body = body.trim_matches('/');
                return body.is_empty() || rel == body || rel.starts_with(&format!("{body}/"));
            }
        }
        crate::is_excluded(rel, &[pattern.to_string()])
    })
}

fn skip_directory(rel: &str, includes: &[String], include_hidden_directories: bool) -> bool {
    rel.split('/').enumerate().any(|(index, name)| {
        ALWAYS_SKIP_DIRS.contains(&name)
            || (!include_hidden_directories
                && name.starts_with('.')
                && !GENERATED_SKIP_DIRS.contains(&name))
            || (GENERATED_SKIP_DIRS.contains(&name)
                // Cargo's default target directory is at the project root.
                && (name != "target" || index == 0)
                && !included_generated_directory(rel, includes))
    })
}

fn included_generated_directory(rel: &str, includes: &[String]) -> bool {
    rel.split('/')
        .scan(String::new(), |prefix, part| {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            Some(prefix.clone())
        })
        .any(|prefix| {
            GENERATED_SKIP_DIRS
                .iter()
                .any(|dir| prefix.rsplit('/').next() == Some(*dir))
                && included_path(&prefix, includes)
        })
}

fn included_path(rel: &str, includes: &[String]) -> bool {
    includes.iter().any(|pattern| {
        let pattern = pattern.trim().trim_start_matches("./");
        crate::is_excluded(rel, &[pattern.to_string()])
    })
}

/// Generated source markers are normally in the first comment block. Reading
/// a bounded prefix keeps large files cheap to classify.
pub fn is_generated_file(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut bytes = Vec::with_capacity(GENERATED_HEADER_BYTES);
    if file
        .take(GENERATED_HEADER_BYTES as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return false;
    }
    let header = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    header.contains("@generated")
        || (header.contains("code generated") && header.contains("do not edit"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("sc-walk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn paths(root: &Path, include_generated: &[String]) -> Vec<String> {
        let mut paths: Vec<String> = walk_files(root, root, &[], include_generated, None)
            .into_iter()
            .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn skips_build_generated_headers_and_nested_gitignore_files() {
        let root = temp("defaults");
        fs::create_dir_all(root.join("build")).unwrap();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::create_dir_all(root.join("src/visible")).unwrap();
        fs::write(root.join("build/output.rs"), "fn generated() {}\n").unwrap();
        fs::write(
            root.join("src/generated.rs"),
            "// Code generated by tool; DO NOT EDIT.\nfn generated() {}\n",
        )
        .unwrap();
        fs::write(root.join("src/nested/ignored.rs"), "fn ignored() {}\n").unwrap();
        fs::write(root.join("src/visible/kept.rs"), "fn kept() {}\n").unwrap();
        fs::write(root.join(".gitignore"), "build/\n").unwrap();
        fs::write(root.join("src/nested/.gitignore"), "ignored.rs\n").unwrap();

        let actual = paths(&root, &[]);
        assert_eq!(
            actual,
            [".gitignore", "src/nested/.gitignore", "src/visible/kept.rs"]
        );
    }

    #[test]
    fn include_generated_overrides_builtins_but_not_scope_exclude() {
        let root = temp("include");
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build/kept.rs"), "// @generated\nfn kept() {}\n").unwrap();
        fs::write(root.join("build/excluded.rs"), "fn excluded() {}\n").unwrap();
        let include = vec!["build/**".into()];
        let exclude = vec!["build/excluded.rs".into()];
        let actual: Vec<_> = walk(&root, &root, &exclude, &include, None)
            .filter_map(|item| match item {
                WalkItem::Entry(entry) if entry.kind == WalkKind::File => Some(
                    entry
                        .path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(actual, ["build/kept.rs"]);
        assert_eq!(
            analyzed_generated_files(&root, &actual, &include),
            ["build/kept.rs"]
        );
    }

    #[test]
    fn nested_target_is_source_but_root_target_is_generated() {
        let root = temp("nested-target");
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join("src/target")).unwrap();
        fs::write(root.join("target/build.rs"), "fn generated() {}\n").unwrap();
        fs::write(root.join("src/target/source.rs"), "fn source() {}\n").unwrap();

        assert_eq!(paths(&root, &[]), ["src/target/source.rs"]);
    }

    #[test]
    fn skips_cdk_out_and_gitignored_generated_dirs() {
        let root = temp("cdk-out");
        fs::create_dir_all(root.join("aws/infra/cdk.out/asset.123")).unwrap();
        fs::create_dir_all(root.join("synth-out")).unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(
            root.join("aws/infra/cdk.out/asset.123/synth.py"),
            "def handler(event):\n    return event\n",
        )
        .unwrap();
        fs::write(
            root.join("synth-out/bundle.py"),
            "def bundle():\n    return 1\n",
        )
        .unwrap();
        fs::write(root.join("app/main.py"), "def main():\n    return 0\n").unwrap();
        // cdk.out is skipped by name; synth-out is skipped because it is
        // git-ignored, even though the name is not a well-known build dir.
        fs::write(root.join(".gitignore"), "synth-out/\n").unwrap();

        assert_eq!(paths(&root, &[]), [".gitignore", "app/main.py"]);
    }

    #[test]
    fn gitignore_negation_reincludes_a_nested_file() {
        let root = temp("negation");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/skip.rs"), "fn skip() {}\n").unwrap();
        fs::write(root.join("src/keep.rs"), "fn keep() {}\n").unwrap();
        fs::write(root.join(".gitignore"), "src/*.rs\n!src/keep.rs\n").unwrap();
        assert_eq!(paths(&root, &[]), [".gitignore", "src/keep.rs"]);
    }
}
