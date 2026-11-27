//! Diff timings on large texts. Ignored by default; run with
//! `cargo test --release -p rustext-diff --test performance -- --ignored --nocapture`.

use std::time::Instant;

use rustext_core::Text;
use rustext_core::style::Rgba;
use rustext_diff::{
    CollapseOptions, Collapsing, DiffColors, DiffOptions, diff_lines, inner_change_decorations, side_by_side_layouts,
};

fn texts(line_count: usize, change_every: usize) -> (Text, Text) {
    let old: String =
        (0..line_count).map(|line| format!("    let value_{line} = compute({line}, \"text {line}\");\n")).collect();
    let new: String = (0..line_count)
        .map(|line| {
            if line % change_every == 0 {
                format!("    let renamed_{line} = compute({line}, \"other {line}\");\n")
            } else {
                format!("    let value_{line} = compute({line}, \"text {line}\");\n")
            }
        })
        .collect();
    (Text::new(&old), Text::new(&new))
}

#[test]
#[ignore = "slow; run in release"]
fn large_diff_timings() {
    let colors = DiffColors {
        removed_line: Rgba::hex(0xff0000),
        added_line: Rgba::hex(0x00ff00),
        removed_text: Rgba::hex(0x800000),
        added_text: Rgba::hex(0x008000),
        moved_line: Rgba::hex(0x0000ff),
    };
    for (line_count, change_every) in [(10_000, 50), (10_000, 3), (100_000, 50)] {
        let (old, new) = texts(line_count, change_every);
        let started = Instant::now();
        let _ = diff_lines(&old, &new, &DiffOptions { inner_changes: false, ..DiffOptions::default() });
        println!("  without inner changes: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        let diff = diff_lines(&old, &new, &DiffOptions::default());
        let diff_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let collapsing = Collapsing { options: Some(CollapseOptions::default()), ..Collapsing::default() };
        let _ = side_by_side_layouts(&diff, &old, &new, &collapsing, &colors, &rustext_diff::WrapPoints::NONE);
        let _ = inner_change_decorations(&diff, &old, &new, &colors);
        let layout_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!("{line_count} lines, {} hunks: diff {diff_ms:.1} ms, layouts {layout_ms:.1} ms", diff.hunks.len());
    }
}

#[test]
#[ignore = "slow; run in release"]
fn imara_algorithms() {
    use imara_diff::{Algorithm, Diff, InternedInput};
    for (line_count, change_every) in [(10_000, 3), (100_000, 50)] {
        let (old, new) = texts(line_count, change_every);
        let (old_lines, new_lines): (Vec<String>, Vec<String>) = (old.raw_lines().collect(), new.raw_lines().collect());
        for algorithm in [Algorithm::Histogram, Algorithm::Myers, Algorithm::MyersMinimal] {
            let started = Instant::now();
            let mut input: InternedInput<String> = InternedInput::default();
            input.update_before(old_lines.iter().cloned());
            input.update_after(new_lines.iter().cloned());
            let interned_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = Instant::now();
            let mut diff = Diff::compute(algorithm, &input);
            let compute_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = Instant::now();
            diff.postprocess_lines(&input);
            let postprocess_ms = started.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{line_count}/{change_every} {algorithm:?}: intern {interned_ms:.1} ms, compute {compute_ms:.1} ms, postprocess {postprocess_ms:.1} ms"
            );
        }
    }
}
