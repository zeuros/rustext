//! Applies built patches with the real `git apply`, the way gitgud does, and checks the
//! result against what the selection means. Every subset of changed lines is tried on a set
//! of text pairs. Skipped when `git` is not installed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rustext_core::Text;
use rustext_diff::line_diff::git_line_count;
use rustext_diff::{DiffOptions, LineDiff, LineSelection, PatchDirection, build_patch, diff_lines};

const FILE_NAME: &str = "file.txt";

const TEXT_PAIRS: &[(&str, &str)] = &[
    ("a\nb\nc\n", "a\nx\ny\nc\n"),
    ("a\nb\nc\nd\n", "a\nb2\nc\nd2\ne\n"),
    ("one\ntwo\nthree\n", "zero\none\nthree\nfour\n"),
    ("keep\nold1\nold2\nold3\nkeep\n", "keep\nnew1\nkeep\n"),
    ("a\nb\n", "a\nb"),
    ("a\nb", "a\nb\nc\n"),
    ("crlf\r\nline\r\n", "crlf\nline\r\n"),
    ("", "new\nfile\n"),
    ("whole\nfile\n", ""),
];

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok_and(|output| output.status.success())
}

fn git(repository: &Path, arguments: &[&str], input: Option<&str>) -> String {
    let mut child = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git runs");
    if let Some(input) = input {
        use std::io::Write;
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}\n{input:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// A fresh repository with `content` committed and staged as `file.txt`.
fn repository_with(name: &str, content: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("rustext-git-apply-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    git(&directory, &["init", "-q"], None);
    git(&directory, &["config", "core.autocrlf", "false"], None);
    std::fs::write(directory.join(FILE_NAME), content).unwrap();
    git(&directory, &["add", FILE_NAME], None);
    directory
}

fn changed_lines(diff: &LineDiff) -> Vec<(bool, usize)> {
    diff.hunks
        .iter()
        .flat_map(|hunk| hunk.old.iter().map(|line| (true, line)).chain(hunk.new.iter().map(|line| (false, line))))
        .collect()
}

fn selection_of(changed: &[(bool, usize)], mask: u32) -> LineSelection {
    let mut selection = LineSelection::default();
    for (index, &(is_old, line)) in changed.iter().enumerate() {
        if mask & (1 << index) != 0 {
            if is_old {
                selection.old_lines.insert(line);
            } else {
                selection.new_lines.insert(line);
            }
        }
    }
    selection
}

/// Raw lines of a text, git's way.
fn raw_lines(text: &Text) -> Vec<String> {
    (0..git_line_count(text)).map(|line| rustext_diff::line_diff::raw_line(text, line)).collect()
}

/// What applying the selection means, computed without git: per hunk, staging keeps the
/// unselected old lines then adds the selected new ones; reverting keeps the selected old
/// lines then the unselected new ones.
fn expected(diff: &LineDiff, old: &Text, new: &Text, selection: &LineSelection, direction: PatchDirection) -> String {
    let (old_lines, new_lines) = (raw_lines(old), raw_lines(new));
    let mut result = Vec::new();
    let mut unchanged_old_line = 0;
    let mut unchanged_new_line = 0;
    for hunk in &diff.hunks {
        match direction {
            PatchDirection::Stage => result.extend(old_lines[unchanged_old_line..hunk.old.start].iter().cloned()),
            PatchDirection::Revert => result.extend(new_lines[unchanged_new_line..hunk.new.start].iter().cloned()),
        }
        for line in hunk.old.iter() {
            let is_selected = selection.old_lines.contains(&line);
            if is_selected == (direction == PatchDirection::Revert) {
                result.push(old_lines[line].clone());
            }
        }
        for line in hunk.new.iter() {
            let is_selected = selection.new_lines.contains(&line);
            if is_selected == (direction == PatchDirection::Stage) {
                result.push(new_lines[line].clone());
            }
        }
        unchanged_old_line = hunk.old.end;
        unchanged_new_line = hunk.new.end;
    }
    match direction {
        PatchDirection::Stage => result.extend(old_lines[unchanged_old_line..].iter().cloned()),
        PatchDirection::Revert => result.extend(new_lines[unchanged_new_line..].iter().cloned()),
    }
    result.concat()
}

fn check_every_selection(direction: PatchDirection) {
    if !git_available() {
        eprintln!("git not installed: skipped");
        return;
    }
    let mut applied_patch_count = 0;
    for (pair_index, (old_content, new_content)) in TEXT_PAIRS.iter().enumerate() {
        let (old, new) = (Text::new(old_content), Text::new(new_content));
        let diff = diff_lines(&old, &new, &DiffOptions::default());
        let changed = changed_lines(&diff);
        assert!(changed.len() <= 12, "too many combinations");

        for mask in 1..(1u32 << changed.len()) {
            let selection = selection_of(&changed, mask);
            let Some(patch) = build_patch(&diff, &old, &new, &selection, direction, FILE_NAME) else { continue };
            applied_patch_count += 1;
            let name = format!("{direction:?}-{pair_index}-{mask}");
            let applied = match direction {
                PatchDirection::Stage => {
                    let repository = repository_with(&name, old_content);
                    git(&repository, &["apply", "--cached", "--unidiff-zero", "--allow-overlap", "-"], Some(&patch));
                    let staged = git(&repository, &["show", &format!(":{FILE_NAME}")], None);
                    let _ = std::fs::remove_dir_all(&repository);
                    staged
                }
                PatchDirection::Revert => {
                    let repository = repository_with(&name, new_content);
                    git(&repository, &["apply", "-R", "--unidiff-zero", "--allow-overlap", "-"], Some(&patch));
                    let reverted = std::fs::read_to_string(repository.join(FILE_NAME)).unwrap();
                    let _ = std::fs::remove_dir_all(&repository);
                    reverted
                }
            };
            assert_eq!(
                applied,
                expected(&diff, &old, &new, &selection, direction),
                "pair {pair_index}, mask {mask:b}, patch:\n{patch}"
            );
        }
    }
    // Sum over the pairs of 2^(changed lines) - 1 non-empty selections.
    assert_eq!(applied_patch_count, 79);
}

#[test]
fn staging_any_selection_with_git_apply() {
    check_every_selection(PatchDirection::Stage);
}

#[test]
fn reverting_any_selection_with_git_apply() {
    check_every_selection(PatchDirection::Revert);
}
