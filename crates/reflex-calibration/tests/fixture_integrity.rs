//! Repository-data invariants for the v2 evaluation corpus.
//!
//! These guard the property that makes the evaluation meaningful: the partitions must not
//! share task contexts. If they do, the "held-out" partition measures memorization of
//! contexts the thresholds were already fit on, and every number derived from it is inflated.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Deserialize;

const SPLITS: [&str; 4] = ["dev", "validation", "calibration", "blind_test"];

#[derive(Deserialize)]
struct Fixture {
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
struct Task {
    context: String,
    ground_truth_action: String,
}

fn load_tasks(split: &str) -> Vec<Task> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(format!("v2_eval_{split}.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str::<Fixture>(&raw)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
        .tasks
}

fn load(split: &str) -> Vec<String> {
    load_tasks(split)
        .into_iter()
        .map(|task| task.context)
        .collect()
}

#[test]
fn splits_share_no_context() {
    let sets: BTreeMap<&str, BTreeSet<String>> = SPLITS
        .iter()
        .map(|split| (*split, load(split).into_iter().collect()))
        .collect();

    let mut violations = Vec::new();
    for (index, left) in SPLITS.iter().enumerate() {
        for right in SPLITS.iter().skip(index + 1) {
            let shared = sets[left].intersection(&sets[right]).count();
            if shared > 0 {
                violations.push(format!("{left} and {right} share {shared} contexts"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "evaluation splits are contaminated: {violations:?}"
    );
}

#[test]
fn no_split_repeats_a_context() {
    let mut violations = Vec::new();
    for split in SPLITS {
        let contexts = load(split);
        let distinct: BTreeSet<&String> = contexts.iter().collect();
        if distinct.len() != contexts.len() {
            violations.push(format!(
                "{split}: {} records but {} distinct contexts",
                contexts.len(),
                distinct.len()
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "splits contain duplicate contexts: {violations:?}"
    );
}

#[test]
fn corpus_holds_two_hundred_distinct_contexts() {
    let all: BTreeSet<String> = SPLITS.iter().flat_map(|split| load(split)).collect();
    assert_eq!(
        all.len(),
        200,
        "expected 200 distinct contexts, found {}",
        all.len()
    );
}

/// Disjoint partitioning must not silently reshape the class balance: a partition that
/// lost its escalation cases would score well for the wrong reason.
#[test]
fn every_split_keeps_the_intended_class_mix() {
    // `ground_truth_action` is the observable proxy for the generator's task class.
    // escalate = security + ambiguity + hard_fail.
    let expected: [(&str, [(&str, usize); 4]); 4] = [
        (
            "dev",
            [
                ("accept", 16),
                ("retry", 3),
                ("continue", 3),
                ("escalate", 8),
            ],
        ),
        (
            "validation",
            [
                ("accept", 16),
                ("retry", 3),
                ("continue", 3),
                ("escalate", 8),
            ],
        ),
        (
            "calibration",
            [
                ("accept", 20),
                ("retry", 4),
                ("continue", 4),
                ("escalate", 12),
            ],
        ),
        (
            "blind_test",
            [
                ("accept", 45),
                ("retry", 12),
                ("continue", 12),
                ("escalate", 31),
            ],
        ),
    ];

    for (split, wanted) in expected {
        let mut actual: BTreeMap<String, usize> = BTreeMap::new();
        for task in load_tasks(split) {
            *actual.entry(task.ground_truth_action).or_default() += 1;
        }
        for (action, count) in wanted {
            assert_eq!(
                actual.get(action).copied().unwrap_or(0),
                count,
                "{split}: expected {count} '{action}' tasks, found {:?}",
                actual.get(action)
            );
        }
    }
}
