// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The orders a folder's entries are served in besides by name (AC-52): recent first, by
//! extension; folders always before files. A listing is read in name order (listing.rs); an
//! order is the entries' indexes, made the first time a page asks for it and kept with the
//! listing, so paging does not sort again. Only the recent order stat()s every entry.

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::listing::{mtime_ns, natural_cmp, Entry};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    /// case-insensitive, runs of digits as numbers (Finder's): the order a listing is read in
    #[default]
    Name,
    /// last modified first (a link by its own time: never a wait on what it points to)
    Recent,
    /// by extension, case-insensitive, names without one first; then by name
    Ext,
}

/// A name's extension: after its last dot, unless that dot starts the name (".gitignore").
pub fn extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[i + 1..],
        _ => "",
    }
}

/// The indexes of `entries` (in name order, folders first) in `sort`'s order; None: name order.
pub fn order(entries: &[Entry], dir: &Path, sort: Sort) -> Option<Vec<u32>> {
    let mut idx: Vec<u32> = (0..entries.len() as u32).collect();
    let folder = |i: u32| entries[i as usize].kind.is_dir();
    match sort {
        Sort::Name => return None,
        Sort::Recent => {
            let time = times(entries, dir);
            // stable: the same time keeps name order
            idx.sort_by(|&a, &b| folder(b).cmp(&folder(a)).then_with(|| time[b as usize].cmp(&time[a as usize])));
        }
        Sort::Ext => {
            let ext = |i: u32| if folder(i) { "" } else { extension(&entries[i as usize].name) };
            idx.sort_by(|&a, &b| folder(b).cmp(&folder(a)).then_with(|| match (ext(a), ext(b)) {
                ("", "") => Ordering::Equal,
                ("", _) => Ordering::Less,
                (_, "") => Ordering::Greater,
                (x, y) => natural_cmp(x, y),
            }));
        }
    }
    Some(idx)
}

/// Each entry's last modified time (i128::MIN: unreadable), read on a few threads: one lstat()
/// after another took 3.7 s for 500,000 entries, past AC-52's 2.5 s for the first rows.
fn times(entries: &[Entry], dir: &Path) -> Vec<i128> {
    let time = |e: &Entry| fs::symlink_metadata(dir.join(&*e.name)).map_or(i128::MIN, |m| mtime_ns(&m));
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    if entries.len() < 4096 || threads < 2 {
        return entries.iter().map(time).collect();
    }
    let part = entries.len().div_ceil(threads);
    std::thread::scope(|s| {
        let parts: Vec<_> = entries.chunks(part).map(|c| s.spawn(move || c.iter().map(time).collect::<Vec<_>>())).collect();
        parts.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::Kind;
    use std::time::{Duration, SystemTime};

    fn entries(names: &[(&str, Kind)]) -> Vec<Entry> {
        names.iter().map(|(n, k)| Entry { name: (*n).into(), kind: *k }).collect()
    }
    fn names(e: &[Entry], order: &[u32]) -> Vec<String> {
        order.iter().map(|&i| e[i as usize].name.to_string()).collect()
    }

    #[test]
    fn extensions() {
        for (name, ext) in [("a.rs", "rs"), ("a.tar.GZ", "GZ"), (".gitignore", ""), ("Makefile", ""), ("a.", ""), (".env.local", "local")] {
            assert_eq!(extension(name), ext, "{name}");
        }
    }

    #[test]
    fn by_extension_folders_first_then_names_without_one_then_by_extension_and_name() {
        let e = entries(&[("src", Kind::Dir), ("x.md", Kind::Dir), ("a.TXT", Kind::File), ("b.md", Kind::File),
            ("Makefile", Kind::File), ("z.rs", Kind::File), ("c.txt", Kind::LinkFile), (".gitignore", Kind::File)]);
        let o = order(&e, Path::new("/nonexistent"), Sort::Ext).unwrap();
        assert_eq!(names(&e, &o), ["src", "x.md", "Makefile", ".gitignore", "b.md", "z.rs", "a.TXT", "c.txt"]);
        assert_eq!(order(&e, Path::new("/"), Sort::Name), None, "name order: the listing's own");
    }

    #[test]
    fn recent_first_folders_first_ties_and_unreadable_ones_by_name() {
        let dir = crate::testutil::temp_root().join(format!("tm-sort-{}", std::process::id()));
        fs::create_dir_all(dir.join("old")).unwrap();
        fs::create_dir_all(dir.join("new")).unwrap();
        let now = SystemTime::now();
        let at = |name: &str, ago: u64| {
            let f = fs::File::options().write(true).create(true).truncate(false).open(dir.join(name)).unwrap();
            f.set_modified(now - Duration::from_secs(ago)).unwrap();
        };
        for (n, ago) in [("a.txt", 300), ("b.txt", 10), ("C.txt", 100), ("d.txt", 100)] { at(n, ago); }
        // a folder's time set through its descriptor, opened to read
        let folder = |name: &str, ago: u64| fs::File::open(dir.join(name)).unwrap().set_modified(now - Duration::from_secs(ago)).unwrap();
        folder("old", 1000);
        folder("new", 5);
        let e = entries(&[("new", Kind::Dir), ("old", Kind::Dir), ("a.txt", Kind::File), ("b.txt", Kind::File),
            ("C.txt", Kind::File), ("d.txt", Kind::File), ("gone.txt", Kind::File)]);
        let o = order(&e, &dir, Sort::Recent).unwrap();
        assert_eq!(names(&e, &o), ["new", "old", "b.txt", "C.txt", "d.txt", "a.txt", "gone.txt"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
