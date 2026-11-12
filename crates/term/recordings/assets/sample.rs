//! A small sample for the vim recording.
//! 日本語のコメントと絵文字 🦀 も表示する。

use std::collections::HashMap;

/// Counts words, ignoring case.
pub fn word_counts(text: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word.to_lowercase()).or_insert(0) += 1;
    }
    counts
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Circle { r: f64 },
    Rect { w: f64, h: f64 },
}

impl Shape {
    fn area(self) -> f64 {
        match self {
            Shape::Circle { r } => std::f64::consts::PI * r * r,
            Shape::Rect { w, h } => w * h,
        }
    }
}

fn main() {
    let shapes = [Shape::Circle { r: 1.5 }, Shape::Rect { w: 2.0, h: 3.0 }];
    let total: f64 = shapes.iter().map(|s| s.area()).sum();
    println!("total area: {total:.2}");
    let counts = word_counts("the quick brown fox jumps over the lazy dog");
    println!("{:?}", counts.get("the"));
}
