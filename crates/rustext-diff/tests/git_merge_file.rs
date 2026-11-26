//! Compares the three-way merge with `git merge-file` on generated text triples: when git
//! merges cleanly the default merge must give the same text, and when git conflicts there must
//! be a conflict. Skipped when `git` is not installed.

use std::process::Command;

use rustext_core::Text;
use rustext_diff::merge::{MergeInput, RegionKind, Resolution, merge_regions, merged_text};

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok_and(|output| output.status.success())
}

/// A small deterministic pseudo-random generator (xorshift), so failures reproduce.
struct Random(u64);

impl Random {
    fn next(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }
}

/// `base` with a few lines replaced, inserted or deleted.
fn edited(base: &[String], random: &mut Random, tag: &str) -> Vec<String> {
    let mut lines = base.to_vec();
    for edit in 0..1 + random.next(3) {
        let position = random.next(lines.len() + 1);
        match random.next(3) {
            0 if position < lines.len() => lines[position] = format!("{tag} changed {edit}"),
            1 => lines.insert(position, format!("{tag} inserted {edit}")),
            _ if position < lines.len() && lines.len() > 1 => {
                lines.remove(position);
            }
            _ => lines.insert(position, format!("{tag} added {edit}")),
        }
    }
    lines
}

fn joined(lines: &[String]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// `git merge-file -p`: the merged text and whether it had conflicts.
fn git_merge(base: &str, ours: &str, theirs: &str, directory: &std::path::Path) -> (String, bool) {
    let write = |name: &str, content: &str| {
        let path = directory.join(name);
        std::fs::write(&path, content).unwrap();
        path
    };
    let (ours_path, base_path, theirs_path) = (write("ours", ours), write("base", base), write("theirs", theirs));
    let output =
        Command::new("git").args(["merge-file", "-p"]).args([&ours_path, &base_path, &theirs_path]).output().unwrap();
    let status = output.status.code().expect("git merge-file exits");
    assert!(status >= 0, "git merge-file failed");
    (String::from_utf8(output.stdout).unwrap(), status > 0)
}

#[test]
fn agrees_with_git_merge_file() {
    if !git_available() {
        eprintln!("git not installed: skipped");
        return;
    }
    let directory = std::env::temp_dir().join(format!("rustext-merge-file-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut random = Random(0x2545_f491_4f6c_dd1d);
    let (mut clean_count, mut conflict_count) = (0, 0);

    for case in 0..300 {
        let base: Vec<String> = (0..4 + random.next(8)).map(|line| format!("line {line}")).collect();
        let ours = edited(&base, &mut random, "ours");
        let theirs = edited(&base, &mut random, "theirs");
        let (base, ours, theirs) = (joined(&base), joined(&ours), joined(&theirs));

        let input = MergeInput { base: Text::new(&base), ours: Text::new(&ours), theirs: Text::new(&theirs) };
        let regions = merge_regions(&input);
        let has_conflict = regions.iter().any(|region| region.kind == RegionKind::Conflict);
        let (git_text, git_has_conflict) = git_merge(&base, &ours, &theirs, &directory);

        if git_has_conflict {
            conflict_count += 1;
            assert!(
                has_conflict,
                "case {case}: git conflicts, we don't\nbase:\n{base}\nours:\n{ours}\ntheirs:\n{theirs}"
            );
        } else {
            clean_count += 1;
            assert!(
                !has_conflict,
                "case {case}: git merges cleanly, we conflict\nbase:\n{base}\nours:\n{ours}\ntheirs:\n{theirs}"
            );
            let resolutions: Vec<Resolution> = regions.iter().map(|region| region.kind.default_resolution()).collect();
            assert_eq!(merged_text(&input, &regions, &resolutions), git_text, "case {case}");
        }
    }
    let _ = std::fs::remove_dir_all(&directory);
    assert!(
        clean_count > 30 && conflict_count > 30,
        "{clean_count} clean, {conflict_count} conflicting: generate more varied cases"
    );
}
