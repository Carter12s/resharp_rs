//! Comparative benchmarks: resharp_rs vs Rust regex crate
//!
//! This benchmark compares the performance of resharp_rs (Brzozowski derivative-based)
//! against the standard Rust regex crate on common patterns.
//!
//! Note: resharp_rs uses POSIX leftmost-longest semantics while Rust regex uses
//! PCRE-style leftmost-first semantics. Results may differ for some patterns.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::Duration;

/// Load a haystack file from the rebar benchmarks directory.
fn load_haystack(relative_path: &str) -> String {
    let path = Path::new("benchmarks/rebar/benchmarks/haystacks").join(relative_path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e))
}

/// Benchmark: Literal search - "Sherlock Holmes"
fn bench_literal_sherlock(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare/literal-sherlock");
    group.measurement_time(Duration::from_secs(5));

    let haystack = load_haystack("opensubtitles/en-medium.txt");
    group.throughput(Throughput::Bytes(haystack.len() as u64));

    let pattern = "Sherlock Holmes";

    group.bench_with_input(
        BenchmarkId::new("resharp_rs", pattern),
        &haystack,
        |b, hay| {
            let mut re = resharp_rs::Regex::new(pattern).unwrap();
            b.iter(|| black_box(re.find_all(black_box(hay)).len()));
        },
    );

    group.bench_with_input(
        BenchmarkId::new("rust_regex", pattern),
        &haystack,
        |b, hay| {
            let re = regex::Regex::new(pattern).unwrap();
            b.iter(|| black_box(re.find_iter(black_box(hay)).count()));
        },
    );

    group.finish();
}

/// Benchmark: Character class - digits
fn bench_char_class_digits(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare/char-class-digits");
    group.measurement_time(Duration::from_secs(5));

    let haystack = load_haystack("opensubtitles/en-medium.txt");
    group.throughput(Throughput::Bytes(haystack.len() as u64));

    let pattern = "[0-9]+";

    group.bench_with_input(
        BenchmarkId::new("resharp_rs", pattern),
        &haystack,
        |b, hay| {
            let mut re = resharp_rs::Regex::new(pattern).unwrap();
            b.iter(|| black_box(re.find_all(black_box(hay)).len()));
        },
    );

    group.bench_with_input(
        BenchmarkId::new("rust_regex", pattern),
        &haystack,
        |b, hay| {
            let re = regex::Regex::new(pattern).unwrap();
            b.iter(|| black_box(re.find_iter(black_box(hay)).count()));
        },
    );

    group.finish();
}

/// Benchmark: Alternation - common words
fn bench_alternation_words(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare/alternation-words");
    group.measurement_time(Duration::from_secs(5));

    let haystack = load_haystack("opensubtitles/en-medium.txt");
    group.throughput(Throughput::Bytes(haystack.len() as u64));

    let pattern = "the|and|for|that|with";

    group.bench_with_input(
        BenchmarkId::new("resharp_rs", pattern),
        &haystack,
        |b, hay| {
            let mut re = resharp_rs::Regex::new(pattern).unwrap();
            b.iter(|| black_box(re.find_all(black_box(hay)).len()));
        },
    );

    group.bench_with_input(
        BenchmarkId::new("rust_regex", pattern),
        &haystack,
        |b, hay| {
            let re = regex::Regex::new(pattern).unwrap();
            b.iter(|| black_box(re.find_iter(black_box(hay)).count()));
        },
    );

    group.finish();
}

/// Benchmark: Word characters
fn bench_word_chars(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare/word-chars");
    group.measurement_time(Duration::from_secs(5));

    let haystack = load_haystack("opensubtitles/en-medium.txt");
    group.throughput(Throughput::Bytes(haystack.len() as u64));

    let pattern = "[a-zA-Z]+";

    group.bench_with_input(
        BenchmarkId::new("resharp_rs", pattern),
        &haystack,
        |b, hay| {
            let mut re = resharp_rs::Regex::new(pattern).unwrap();
            b.iter(|| black_box(re.find_all(black_box(hay)).len()));
        },
    );

    group.bench_with_input(
        BenchmarkId::new("rust_regex", pattern),
        &haystack,
        |b, hay| {
            let re = regex::Regex::new(pattern).unwrap();
            b.iter(|| black_box(re.find_iter(black_box(hay)).count()));
        },
    );

    group.finish();
}

/// Benchmark: Compilation time
fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare/compile");

    let patterns = [
        ("literal", "Sherlock Holmes"),
        ("char-class", "[a-zA-Z0-9]+"),
        ("alternation-5", "foo|bar|baz|qux|quux"),
    ];

    for (name, pattern) in patterns {
        group.bench_with_input(BenchmarkId::new("resharp_rs", name), &pattern, |b, &p| {
            b.iter(|| black_box(resharp_rs::Regex::new(black_box(p)).unwrap()));
        });

        group.bench_with_input(BenchmarkId::new("rust_regex", name), &pattern, |b, &p| {
            b.iter(|| black_box(regex::Regex::new(black_box(p)).unwrap()));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_literal_sherlock,
    bench_char_class_digits,
    bench_alternation_words,
    bench_word_chars,
    bench_compile,
);
criterion_main!(benches);
