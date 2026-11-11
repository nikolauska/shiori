//! Runs the parser against Anitomy's own test data.
//!
//! Some expected outputs in that data are not reached by any implementation,
//! including Anitomy's own build, so the test does not demand a perfect score.
//! Instead the set of failing inputs is recorded, and any change to it fails:
//! a new failure is a regression, a new pass must be recorded as progress.

use std::collections::{BTreeMap, BTreeSet};

use shiori_core::recognition::{ParseOptions, parse};

const DATA: &str = include_str!("data/anitomy.json");
const KNOWN_FAILURES: &str = "tests/data/anitomy-known-failures.txt";

fn kind_name(kind: shiori_core::ElementKind) -> String {
    // Element kinds in the test data are snake_case names of our variants.
    let debug = format!("{kind:?}");
    let mut out = String::new();
    for (i, c) in debug.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

#[test]
fn anitomy_test_data() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(DATA).unwrap();
    assert_eq!(cases.len(), 251);

    let mut failing = BTreeSet::new();
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let mut want: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (kind, value) in case["output"].as_object().unwrap() {
            let values = match value {
                serde_json::Value::Array(a) => {
                    a.iter().map(|v| v.as_str().unwrap().to_owned()).collect()
                }
                v => vec![v.as_str().unwrap().to_owned()],
            };
            want.insert(kind.clone(), values);
        }
        let mut got: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for element in parse(input, ParseOptions::default()).elements {
            got.entry(kind_name(element.kind))
                .or_default()
                .push(element.value);
        }
        if got != want {
            failing.insert(input.to_owned());
        }
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(KNOWN_FAILURES);
    if std::env::var_os("UPDATE_KNOWN_FAILURES").is_some() {
        let text: String = failing.iter().map(|f| format!("{f}\n")).collect();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let known: BTreeSet<String> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    let new_failures: Vec<_> = failing.difference(&known).collect();
    let new_passes: Vec<_> = known.difference(&failing).collect();
    assert!(
        new_failures.is_empty() && new_passes.is_empty(),
        "new failures: {new_failures:#?}\nnow passing: {new_passes:#?}"
    );
    eprintln!("{} of {} cases pass", cases.len() - failing.len(), cases.len());
}
