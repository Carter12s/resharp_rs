# resharp_rs

[![CI](https://github.com/ieviev/resharp_rs/actions/workflows/ci.yml/badge.svg)](https://github.com/ieviev/resharp_rs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/resharp_rs.svg)](https://crates.io/crates/resharp_rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A high-performance regex engine implementing **Brzozowski derivatives** with first-class support for **intersection** and **complement** operations.

This is a Rust port of the [RE# regex engine](https://github.com/ieviev/resharp-dotnet), based on the algorithms described in the POPL 2025 paper.

## Features

- 🚀 **Non-backtracking** — Guaranteed linear-time matching via DFA
- 🎯 **POSIX semantics** — Always finds the leftmost-longest match
- ⚡ **Fast compilation** — Automatic DFA pre-computation for optimal throughput
- 🔧 **Extended syntax** — Intersection (`&`), complement (`~`), and universal wildcard (`_`)

## Installation

```toml
[dependencies]
resharp_rs = "0.1"
```

## Quick Start

```rust
use resharp_rs::Regex;

let mut re = Regex::new(r"[a-zA-Z]+").unwrap();

// Check for a match
assert!(re.is_match("hello world"));

// Find the first match
let m = re.find("123 hello 456").unwrap();
assert_eq!(m.as_str("123 hello 456"), "hello");

// Find all matches
let matches = re.find_all("one two three");
assert_eq!(matches.len(), 3);
```

## Extended Syntax

Beyond standard regex, resharp_rs supports:

| Syntax   | Description                                                   | Example                                                           |
| -------- | ------------------------------------------------------------- | ----------------------------------------------------------------- |
| `&`      | Intersection — both sides must match                          | `.*cat.*&.*dog.*` matches strings containing both "cat" and "dog" |
| `~(...)` | Complement — matches what the inner pattern doesn't           | `~(.*error.*)` matches strings without "error"                    |
| `_`      | Universal wildcard — matches any character including newlines | `a_b` matches "a\nb"                                              |

### Example: Complex Constraints

```rust
use resharp_rs::Regex;

// Match strings that contain "cat" AND "dog" AND are 10-20 chars long
let mut re = Regex::new(r".*cat.*&.*dog.*&.{10,20}").unwrap();
assert!(re.is_match("my cat and dog")); // 14 chars, has both
assert!(!re.is_match("cat dog"));        // too short
```

## Benchmarks

Results on a 60KB text corpus (Intel Core i9-14900K):

### Matching Performance

| Pattern                     | resharp_rs (Rust) | RE# (F#/.NET) | rust regex |
| --------------------------- | ----------------- | ------------- | ---------- |
| `Sherlock Holmes` (literal) | 1.77 µs           | 7 µs          | 1.67 µs    |
| `[0-9]+`                    | 81 µs             | 19 µs         | 2.6 µs     |
| `the\|and\|for\|that\|with` | 193 µs            | 240 µs        | 28 µs      |
| `[a-zA-Z]+`                 | **260 µs**        | 1,099 µs      | 409 µs     |

### Compilation Time

| Pattern                    | resharp_rs (Rust) | RE# (F#/.NET) | rust regex |
| -------------------------- | ----------------- | ------------- | ---------- |
| `Sherlock Holmes`          | 102 µs            | 2,718 µs      | 2.9 µs     |
| `[a-zA-Z0-9]+`             | 16 µs             | 200 µs        | 6.5 µs     |
| `foo\|bar\|baz\|qux\|quux` | 104 µs            | 920 µs        | 23 µs      |

### Notes

- **resharp_rs** pre-computes the DFA at construction time (up to 1024 states), providing consistent matching performance
- **RE# (F#/.NET)** is the original implementation; benchmarked via `dotnet fsi ./benches/resharp_fsharp_benchmark.fsx`
- **rust regex** uses SIMD acceleration and specialized engines (Aho-Corasick for alternations, memchr for literals)
- resharp_rs excels at patterns with large character classes where NFA simulation is slower

## Credits

This is a Rust port of the [RE# regex engine](https://github.com/ieviev/resharp-dotnet) by **Ievgenii Shcherbina**, implementing the algorithms described in:

> **Derivative-Based Nonbacktracking Real-World Regex Matching with Intersection, Complement, and Lookarounds**
> Ievgenii Shcherbina, Margus Veanes, and Olli Saarikivi
> POPL 2025

## License

MIT
