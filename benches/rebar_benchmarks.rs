//! Benchmarks using patterns and haystacks from the rebar benchmark suite.
//!
//! See: https://github.com/BurntSushi/rebar

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use resharp_rs::Regex;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::Duration;

/// Load a haystack file from the rebar benchmarks directory.
fn load_haystack(relative_path: &str) -> String {
    let path = Path::new("testdata/rebar/benchmarks/haystacks").join(relative_path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e))
}

/// Benchmark group: Literal searches
/// Tests simple literal pattern matching performance.
fn bench_literal(c: &mut Criterion) {
    let mut group = c.benchmark_group("literal");
    group.measurement_time(Duration::from_secs(5));

    // Load English subtitles haystack
    let haystack_en = load_haystack("opensubtitles/en-medium.txt");
    group.throughput(Throughput::Bytes(haystack_en.len() as u64));

    // Simple literal - Sherlock Holmes
    group.bench_function("sherlock-en", |b| {
        let mut re = Regex::new("Sherlock Holmes").unwrap();
        b.iter(|| {
            let count = re.find_all(black_box(&haystack_en)).len();
            black_box(count)
        });
    });

    // Case-insensitive literal would require flag support in parser
    // For now, test with character class approximation
    group.bench_function("word-the-en", |b| {
        let mut re = Regex::new("[Tt]he").unwrap();
        b.iter(|| {
            let count = re.find_all(black_box(&haystack_en)).len();
            black_box(count)
        });
    });

    group.finish();
}

/// Benchmark group: Character classes and dot
fn bench_char_classes(c: &mut Criterion) {
    let mut group = c.benchmark_group("char_classes");
    group.measurement_time(Duration::from_secs(5));

    let haystack = load_haystack("opensubtitles/en-medium.txt");
    group.throughput(Throughput::Bytes(haystack.len() as u64));

    // Digit sequences
    group.bench_function("digits", |b| {
        let mut re = Regex::new("[0-9]+").unwrap();
        b.iter(|| {
            let count = re.find_all(black_box(&haystack)).len();
            black_box(count)
        });
    });

    // Word characters
    group.bench_function("words", |b| {
        let mut re = Regex::new("[a-zA-Z]+").unwrap();
        b.iter(|| {
            let count = re.find_all(black_box(&haystack)).len();
            black_box(count)
        });
    });

    // Dot star (match any line)
    group.bench_function("dot-star", |b| {
        let mut re = Regex::new(".*").unwrap();
        b.iter(|| {
            let count = re.find_all(black_box(&haystack)).len();
            black_box(count)
        });
    });

    group.finish();
}

/// Benchmark group: Alternation patterns
fn bench_alternation(c: &mut Criterion) {
    let mut group = c.benchmark_group("alternation");
    group.measurement_time(Duration::from_secs(5));

    let haystack = load_haystack("opensubtitles/en-medium.txt");
    group.throughput(Throughput::Bytes(haystack.len() as u64));

    // Small alternation
    group.bench_function("small-2", |b| {
        let mut re = Regex::new("Sherlock|Holmes").unwrap();
        b.iter(|| {
            let count = re.find_all(black_box(&haystack)).len();
            black_box(count)
        });
    });

    // Medium alternation
    group.bench_function("medium-5", |b| {
        let mut re = Regex::new("the|and|for|that|with").unwrap();
        b.iter(|| {
            let count = re.find_all(black_box(&haystack)).len();
            black_box(count)
        });
    });

    group.finish();
}

/// Benchmark group: Intersection (unique to resharp)
/// This is a feature not available in most regex engines!
fn bench_intersection(c: &mut Criterion) {
    let mut group = c.benchmark_group("intersection");
    group.measurement_time(Duration::from_secs(5));

    let haystack = load_haystack("opensubtitles/en-medium.txt");
    group.throughput(Throughput::Bytes(haystack.len() as u64));

    // Find words that are exactly 5 letters AND contain 'e'
    group.bench_function("5-letter-with-e", |b| {
        let mut re = Regex::new("[a-zA-Z]{5}&.*e.*").unwrap();
        b.iter(|| {
            let count = re.find_all(black_box(&haystack)).len();
            black_box(count)
        });
    });

    group.finish();
}

/// Benchmark group: Compilation time
fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile");

    // Simple literal
    group.bench_function("literal", |b| {
        b.iter(|| {
            let re = Regex::new(black_box("Sherlock Holmes")).unwrap();
            black_box(re)
        });
    });

    // Character class
    group.bench_function("char-class", |b| {
        b.iter(|| {
            let re = Regex::new(black_box("[a-zA-Z0-9]+")).unwrap();
            black_box(re)
        });
    });

    // Alternation
    group.bench_function("alternation", |b| {
        b.iter(|| {
            let re = Regex::new(black_box("foo|bar|baz|qux")).unwrap();
            black_box(re)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_literal,
    bench_char_classes,
    bench_alternation,
    bench_intersection,
    bench_compile,
);
criterion_main!(benches);
