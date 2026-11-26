//! A sample file for the rustext playground.

use std::collections::HashMap;

/// Counts how often each word appears in `text`.
pub fn word_counts(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }
    counts
}

#[derive(Debug, Clone, PartialEq)]
enum Shape {
	Circle { radius: f64 },
	Rectangle { width: f64, height: f64 },
}

impl Shape {
    fn area(&self) -> f64 {
        match self {
            Shape::Circle { radius } => std::f64::consts::PI * radius * radius,
            Shape::Rectangle { width, height } => width * height,
        }
    }
}

fn main() {
    let shapes = vec![Shape::Circle { radius: 1.5 }, Shape::Rectangle { width: 2.0, height: 3.0 }];
    let total: f64 = shapes.iter().map(Shape::area).sum();
    println!("total area: {total:.2}"); // emoji: 🦀 — wide: 漢字 — combining: é
    let counts = word_counts("the quick brown fox jumps over the lazy dog the end");
    println!("{counts:?}");
}
