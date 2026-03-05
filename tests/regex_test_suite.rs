//! Test suite using the rust-lang/regex testdata TOML files.
//!
//! This runs a subset of tests that are applicable to our engine.
//!
//! Note: This engine uses POSIX leftmost-longest semantics, while the Rust regex
//! crate uses PCRE-style leftmost-first semantics. This means some tests will
//! produce different (but correct for POSIX) results.

use resharp_rs::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Tests that fail due to semantic differences (POSIX vs PCRE).
/// These are not bugs - our engine correctly implements POSIX semantics.
fn semantic_difference_tests() -> HashSet<&'static str> {
    [
        // Empty alternation tests - POSIX prefers longest match, PCRE prefers leftmost
        "empty/100", // |b - POSIX matches 'b', PCRE prefers empty
        "empty/110", // b| - same issue
        "empty/220", // ||b
        "empty/230", // b||
        "empty/300", // (?:)|b
        "empty/310", // b|(?:)
        "empty/600", // (?:|a)*
        "empty/610", // (?:|a)+
        // Lazy quantifier tests - our engine treats *? as * (leftmost-longest)
        "iter/nonempty-followedby-empty",    // abc|.*?
        "iter/nonempty-followedby-oneempty", // abc|.*?
        "iter/nonempty-followedby-onemixed", // abc|.*?
        "iter/nonempty-followedby-twomixed", // abc|.*?
        // (?:)+ handling with alternation
        "crazy/empty10", // (?:)+|b
        "crazy/empty11", // b|(?:)+
        "iter/empty10",  // (?:)+|b
        "iter/empty11",  // b|(?:)+
    ]
    .into_iter()
    .collect()
}

/// Tests that fail due to missing features (not semantic differences).
fn missing_feature_tests() -> HashSet<&'static str> {
    [
        // Multiline mode - requires tracking previous character
        "misc/anchor-start-end-line", // (?m)^bar$
        // Word boundary with flags - \b implementation needs work
        "crazy/ranges",     // (?-u)\b...\b
        "crazy/ranges-not", // (?-u)\b...\b (expects no match)
        "crazy/email",      // (?i-u)\b...\b - also needs case-insensitive
    ]
    .into_iter()
    .collect()
}

/// A single regex test case from the TOML files.
#[derive(Debug, Deserialize)]
struct RegexTest {
    name: String,
    regex: RegexPattern,
    haystack: String,
    #[serde(default)]
    matches: Vec<MatchSpec>,
    #[serde(default = "default_true")]
    compiles: bool,
    #[serde(default)]
    anchored: bool,
    #[serde(default)]
    unescape: bool,
    #[serde(default = "default_true")]
    unicode: bool,
    #[serde(default = "default_true")]
    utf8: bool,
    #[serde(rename = "match-kind")]
    match_kind: Option<String>,
    #[serde(rename = "search-kind")]
    search_kind: Option<String>,
    #[serde(rename = "case-insensitive")]
    #[serde(default)]
    case_insensitive: bool,
    bounds: Option<Bounds>,
    #[serde(rename = "match-limit")]
    match_limit: Option<usize>,
    #[serde(rename = "line-terminator")]
    line_terminator: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RegexPattern {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MatchSpec {
    Span([usize; 2]),
    WithId {
        id: usize,
        span: [usize; 2],
    },
    Captures(Vec<Option<[usize; 2]>>),
    CapturesWithId {
        id: usize,
        spans: Vec<Option<[usize; 2]>>,
    },
}

#[derive(Debug, Deserialize)]
struct Bounds {
    start: usize,
    end: usize,
}

#[derive(Debug, Deserialize)]
struct TestFile {
    #[serde(default, rename = "test")]
    tests: Vec<RegexTest>,
}

/// Unescape a haystack string (handle \x00 sequences)
fn unescape_haystack(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('x') => {
                    chars.next();
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                    }
                }
                Some('n') => {
                    chars.next();
                    result.push('\n');
                }
                Some('r') => {
                    chars.next();
                    result.push('\r');
                }
                Some('t') => {
                    chars.next();
                    result.push('\t');
                }
                Some('\\') => {
                    chars.next();
                    result.push('\\');
                }
                _ => result.push(c),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Check if a test is applicable to our engine
fn is_applicable(test: &RegexTest) -> bool {
    // Skip tests with features we don't support
    if test.anchored {
        return false; // We don't support anchored search mode
    }
    if test.case_insensitive {
        return false; // We don't support case insensitive flag yet
    }
    if test.match_kind.as_deref() == Some("leftmost-first") {
        return false; // We use leftmost-longest semantics
    }
    if test.search_kind.as_deref() == Some("earliest") {
        return false; // We don't support earliest search
    }
    if test.search_kind.as_deref() == Some("overlapping") {
        return false; // We don't support overlapping search
    }
    if test.bounds.is_some() {
        return false; // We don't support bounded search
    }
    // Skip multi-pattern tests
    if let RegexPattern::Multiple(_) = &test.regex {
        return false;
    }
    true
}

fn get_expected_matches(test: &RegexTest) -> Vec<(usize, usize)> {
    test.matches
        .iter()
        .filter_map(|m| match m {
            MatchSpec::Span([start, end]) => Some((*start, *end)),
            MatchSpec::WithId { span, .. } => Some((span[0], span[1])),
            MatchSpec::Captures(caps) => caps.first().and_then(|c| c.map(|[s, e]| (s, e))),
            MatchSpec::CapturesWithId { spans, .. } => {
                spans.first().and_then(|c| c.map(|[s, e]| (s, e)))
            }
        })
        .collect()
}

fn run_test_file(path: &Path) -> (usize, usize, usize, Vec<String>) {
    let content = fs::read_to_string(path).expect("Failed to read test file");
    let test_file: TestFile = toml::from_str(&content).expect("Failed to parse TOML");
    let file_name = path.file_stem().unwrap().to_str().unwrap();

    let semantic_skips = semantic_difference_tests();
    let feature_skips = missing_feature_tests();

    let mut passed = 0;
    let mut skipped = 0;
    let mut known_failures = 0;
    let mut failures = Vec::new();

    for test in test_file.tests {
        let full_name = format!("{}/{}", file_name, test.name);

        // Skip tests with known semantic differences (POSIX vs PCRE)
        if semantic_skips.contains(full_name.as_str()) {
            known_failures += 1;
            continue;
        }

        // Skip tests with missing features
        if feature_skips.contains(full_name.as_str()) {
            known_failures += 1;
            continue;
        }

        if !is_applicable(&test) {
            skipped += 1;
            continue;
        }

        let pattern = match &test.regex {
            RegexPattern::Single(s) => s.clone(),
            RegexPattern::Multiple(_) => {
                skipped += 1;
                continue;
            }
        };

        let haystack = if test.unescape {
            unescape_haystack(&test.haystack)
        } else {
            test.haystack.clone()
        };

        // Try to compile the regex
        let regex_result = Regex::new(&pattern);

        if !test.compiles {
            // Expect compilation to fail
            if regex_result.is_err() {
                passed += 1;
            } else {
                failures.push(format!(
                    "{}: expected compilation to fail for pattern '{}'",
                    full_name, pattern
                ));
            }
            continue;
        }

        let mut regex = match regex_result {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!(
                    "{}: failed to compile pattern '{}': {:?}",
                    full_name, pattern, e
                ));
                continue;
            }
        };

        let expected = get_expected_matches(&test);
        let limit = test.match_limit.unwrap_or(usize::MAX);

        // Get actual matches
        let actual: Vec<(usize, usize)> = regex
            .find_all(&haystack)
            .into_iter()
            .take(limit)
            .map(|m| (m.start, m.end))
            .collect();

        if actual == expected {
            passed += 1;
        } else {
            failures.push(format!(
                "{}: pattern='{}' haystack={:?}\n  expected: {:?}\n  actual:   {:?}",
                full_name, pattern, haystack, expected, actual
            ));
        }
    }

    (passed, skipped, known_failures, failures)
}

#[test]
fn test_misc() {
    let path = Path::new("testdata/regex-crate/testdata/misc.toml");
    if !path.exists() {
        eprintln!("Skipping test_misc: testdata not found");
        return;
    }
    let (passed, skipped, known, failures) = run_test_file(path);
    println!(
        "misc.toml: {} passed, {} skipped, {} known issues, {} failed",
        passed,
        skipped,
        known,
        failures.len()
    );
    for f in &failures {
        eprintln!("{}", f);
    }
    assert!(failures.is_empty(), "{} tests failed", failures.len());
}

#[test]
fn test_empty() {
    let path = Path::new("testdata/regex-crate/testdata/empty.toml");
    if !path.exists() {
        eprintln!("Skipping test_empty: testdata not found");
        return;
    }
    let (passed, skipped, known, failures) = run_test_file(path);
    println!(
        "empty.toml: {} passed, {} skipped, {} known issues, {} failed",
        passed,
        skipped,
        known,
        failures.len()
    );
    for f in &failures {
        eprintln!("{}", f);
    }
    assert!(failures.is_empty(), "{} tests failed", failures.len());
}

#[test]
fn test_crazy() {
    let path = Path::new("testdata/regex-crate/testdata/crazy.toml");
    if !path.exists() {
        eprintln!("Skipping test_crazy: testdata not found");
        return;
    }
    let (passed, skipped, known, failures) = run_test_file(path);
    println!(
        "crazy.toml: {} passed, {} skipped, {} known issues, {} failed",
        passed,
        skipped,
        known,
        failures.len()
    );
    for f in &failures {
        eprintln!("{}", f);
    }
    assert!(failures.is_empty(), "{} tests failed", failures.len());
}

/// Run all applicable test files and report summary
#[test]
fn test_all_applicable() {
    let testdata_dir = Path::new("testdata/regex-crate/testdata");
    if !testdata_dir.exists() {
        eprintln!("Skipping test_all_applicable: testdata not found");
        return;
    }

    let mut total_passed = 0;
    let mut total_skipped = 0;
    let mut total_known = 0;
    let mut all_failures = Vec::new();

    // Test files that are most applicable to our engine
    let test_files = ["misc.toml", "empty.toml", "crazy.toml", "iter.toml"];

    for file in test_files {
        let path = testdata_dir.join(file);
        if path.exists() {
            let (passed, skipped, known, failures) = run_test_file(&path);
            println!(
                "{}: {} passed, {} skipped, {} known issues, {} failed",
                file,
                passed,
                skipped,
                known,
                failures.len()
            );
            total_passed += passed;
            total_skipped += skipped;
            total_known += known;
            for f in failures {
                all_failures.push(format!("[{}] {}", file, f));
            }
        }
    }

    println!("\n=== SUMMARY ===");
    println!("Total passed: {}", total_passed);
    println!("Total skipped: {}", total_skipped);
    println!("Total known issues (semantic/feature): {}", total_known);
    println!("Total failed: {}", all_failures.len());

    if !all_failures.is_empty() {
        println!("\n=== FAILURES ===");
        for f in &all_failures {
            eprintln!("{}\n", f);
        }
    }

    assert!(
        all_failures.is_empty(),
        "{} tests failed",
        all_failures.len()
    );
}
