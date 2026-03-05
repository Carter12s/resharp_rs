# Comparative Benchmarks

This directory contains benchmarks for comparing `resharp_rs` against other regex engines.

## Engines Compared

| Engine | Language | Semantics | Notes |
|--------|----------|-----------|-------|
| `resharp_rs` | Rust | POSIX leftmost-longest | Brzozowski derivatives, supports `&` and `~` |
| `regex` | Rust | PCRE leftmost-first | Standard Rust regex crate |
| `resharp-dotnet` | F#/.NET | POSIX leftmost-longest | Original RE# implementation |

## Running Benchmarks

### Rust Benchmarks (resharp_rs vs regex crate)

```bash
# Run all comparative benchmarks
cargo bench --bench comparative_benchmarks

# Run with HTML report generation
cargo bench --bench comparative_benchmarks -- --noplot
```

Results are saved in `target/criterion/`.

### F# RE# Benchmarks (resharp-dotnet)

The F# benchmarks require .NET 8+ and can be run in two ways:

#### Option 1: Using the script directly

```bash
# Install .NET SDK if not already installed
# https://dotnet.microsoft.com/download

# Run the benchmark script
dotnet fsi benches/resharp_fsharp_benchmark.fsx
```

#### Option 2: Clone resharp-dotnet and run there

```bash
# Clone the original F# implementation
git clone https://github.com/ieviev/resharp-dotnet
cd resharp-dotnet

# Copy our benchmark script
cp /path/to/resharp_rs/benches/resharp_fsharp_benchmark.fsx .

# Run
dotnet fsi resharp_fsharp_benchmark.fsx
```

## Benchmark Patterns

All engines are tested on the same patterns for fair comparison:

| Name | Pattern | Description |
|------|---------|-------------|
| Literal | `Sherlock Holmes` | Simple literal search |
| Char class | `[0-9]+` | Digit sequences |
| Alternation | `the\|and\|for\|that\|with` | Common words |
| Word chars | `[a-zA-Z]+` | Word sequences |
| Compile | Various | Pattern compilation time |

## Haystack

Benchmarks use the `opensubtitles/en-medium.txt` file from the [rebar](https://github.com/BurntSushi/rebar) benchmark suite.

To set up:

```bash
git submodule update --init --recursive
```

## Expected Results

Based on the architecture differences:

- **Literal search**: Rust `regex` crate is faster (has specialized literal optimizations like Teddy SIMD)
- **Character classes**: Similar performance
- **Alternation**: `resharp_rs` competitive or faster for large alternations
- **Compilation**: `resharp_rs` often faster (simpler compilation)
- **Intersection/Complement**: Only `resharp_rs` and `resharp-dotnet` support these

## Semantic Differences

POSIX (resharp) vs PCRE (regex crate) can produce different matches:

```
Pattern: "a|ab"
Input:   "ab"

POSIX (resharp):  matches "ab" (longest)
PCRE (regex):     matches "a"  (first alternative)
```

Both results are "correct" according to their respective specifications.

