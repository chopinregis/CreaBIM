//! Performance budgets (Addendum A6-7). Timing on shared machines is
//! noisy, so these are opt-in, and only mean something in an optimised
//! build:
//!
//! ```text
//! cargo test --release -p creabim-geom --test perf -- --ignored --nocapture
//! ```
//!
//! Each budget is the time the starter house took on the owner's machine
//! (16 cores) when it was set, with room to spare; a test fails when a
//! change makes a drawing clearly slower. The targets A6 aims for are in
//! `docs/PERFORMANCE.md`.

use creabim_core::{ElementKind, Model, ViewKind};
use std::time::Instant;

/// The best of a few runs, in ms: the least disturbed by other work.
fn best_ms(mut f: impl FnMut()) -> f64 {
    f(); // warm up (the shared scene, allocations)
    (0..3)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .fold(f64::INFINITY, f64::min)
}

fn views(m: &Model, kind: fn(&ViewKind) -> bool) -> Vec<(String, creabim_core::ElementId)> {
    m.iter()
        .filter_map(|e| match &e.kind {
            ElementKind::View(v) if kind(&v.kind) && v.callout.is_none() => {
                Some((e.name.clone(), e.id))
            }
            _ => None,
        })
        .collect()
}

fn check(what: &str, ms: f64, budget: f64, failures: &mut Vec<String>) {
    let flag = if ms > budget { "  OVER" } else { "" };
    println!("{what:<32} {ms:>7.1} ms  (budget {budget:.0}){flag}");
    if ms > budget {
        failures.push(format!("{what}: {ms:.1} ms > {budget:.0} ms"));
    }
}

#[test]
#[ignore = "timing: run with --release -- --ignored"]
fn starter_house_within_budgets() {
    let m = creabim_core::starter::starter_house();
    let mut failures = vec![];

    // the 3D scene is cached per revision, so time a fresh build
    let ms = best_ms(|| {
        let _ = creabim_geom::build_scene(&m);
    });
    check("3D scene", ms, 70.0, &mut failures);

    for (name, id) in views(&m, |k| matches!(k, ViewKind::Plan { .. })) {
        let ms = best_ms(|| {
            let _ = creabim_geom::section::view_drawing_with(&m, id, false);
        });
        check(&name, ms, 25.0, &mut failures);
    }
    for (name, id) in views(&m, |k| matches!(k, ViewKind::Section { .. })) {
        let ms = best_ms(|| {
            let _ = creabim_geom::section::view_drawing_with(&m, id, false);
        });
        check(&name, ms, 160.0, &mut failures);
    }
    for (name, id) in views(&m, |k| matches!(k, ViewKind::Axonometric { .. })) {
        let ms = best_ms(|| {
            let _ = creabim_geom::section::view_drawing_with(&m, id, false);
        });
        check(&name, ms, 300.0, &mut failures);
    }
    assert!(failures.is_empty(), "over budget:\n{}", failures.join("\n"));
}
