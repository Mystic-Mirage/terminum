// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The frame budget (spec §11: capture + cut + SGR < 3 ms for 200×60): a colored 200×60
//! capture (SGR runs, 256 colors, RGB, wide CJK characters, box drawing) is cut into rows
//! and every row encoded, 1000 times; prints the mean and fails over 3 ms.
//! Run: `cargo bench --bench frame` (release profile).

#![allow(dead_code, unused_imports)] // the modules' tests are compiled out of a bench

#[path = "../src/screen"]
mod screen {
    pub mod rows;
    pub mod sgr;
}
#[path = "../src/termtext.rs"]
mod termtext;                    // sgr.rs's rule of a link's characters

use screen::rows;
use std::hint::black_box;
use std::time::Instant;

const WIDTH: usize = 200;
const HEIGHT: usize = 60;
const RUNS: u32 = 1000;
const BUDGET_MS: f64 = 3.0;

/// One capture of a busy 200×60 screen, as `capture-pane -p -e -J` prints it.
fn fixture() -> String {
    let mut out = String::new();
    let mut rows = 0;
    let mut n = 0;
    while rows < HEIGHT {
        let line = match n % 6 {
            // an `ls --color` style row of 256-color and bold names
            0 => (0..20)
                .map(|i| format!("\x1b[1;38;5;{}mfile_{i:02}.rs\x1b[0m ", 16 + (n * 7 + i) % 200))
                .collect::<String>(),
            // a box-drawing frame row in RGB
            1 => format!("\x1b[38;2;80;160;240m\u{250c}{}\u{2510}\x1b[0m", "\u{2500}".repeat(WIDTH - 2)),
            // wide CJK text on a colored background
            2 => format!("\x1b[48;5;236m{}\x1b[49m", "漢字かなカナ한국어".repeat(11)),
            // a long log line that wraps (two rows)
            3 => format!(
                "\x1b[2m2026-01-01T00:00:00Z\x1b[22m \x1b[32mINFO\x1b[39m {}",
                "request handled in 3 ms; ".repeat(14)
            ),
            // a diff-like row with RGB backgrounds and underlined words
            4 => format!(
                "\x1b[48;2;20;60;20m+ {} \x1b[4mchanged\x1b[24m {}\x1b[0m",
                "let x = compute(a, b);".repeat(4),
                "// note".repeat(6)
            ),
            // a prompt with a short command
            _ => "\x1b[1;34m/Users/alex/project\x1b[0m \x1b[35m(main)\x1b[0m $ cargo test".to_string(),
        };
        rows += rows::capture_rows(&format!("{line}\n"), WIDTH).len();
        out.push_str(&line);
        out.push('\n');
        n += 1;
    }
    out
}

fn frame(capture: &str) -> usize {
    let rows = rows::screen_rows(capture, WIDTH, HEIGHT);
    rows.iter()
        .enumerate()
        .map(|(y, row)| rows::encode_str(row, (y == HEIGHT - 1).then_some(14)).len())
        .sum()
}

fn main() {
    let capture = fixture();
    let bytes = frame(&capture);
    for _ in 0..50 {
        black_box(frame(black_box(&capture)));
    }
    let start = Instant::now();
    for _ in 0..RUNS {
        black_box(frame(black_box(&capture)));
    }
    let mean_ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(RUNS);
    println!(
        "frame {WIDTH}x{HEIGHT}: capture {} bytes -> {bytes} bytes of rows; mean {mean_ms:.3} ms over {RUNS} runs (budget {BUDGET_MS} ms)",
        capture.len()
    );
    if mean_ms > BUDGET_MS {
        eprintln!("over budget");
        std::process::exit(1);
    }
}
