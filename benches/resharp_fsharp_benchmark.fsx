#!/usr/bin/env dotnet fsi
/// Comparative benchmarks for F# RE# (resharp-dotnet)
/// 
/// This script benchmarks the original F# RE# engine on the same patterns
/// used in benches/comparative_benchmarks.rs for comparison.
///
/// To run:
///   1. Clone resharp-dotnet: git clone https://github.com/ieviev/resharp-dotnet
///   2. Copy this script to the resharp-dotnet directory
///   3. Run: dotnet fsi resharp_fsharp_benchmark.fsx
///
/// Or install the NuGet package:
///   dotnet add package Resharp
///   dotnet fsi resharp_fsharp_benchmark.fsx

#r "nuget: Resharp"

open System
open System.IO
open System.Diagnostics
open Resharp

/// Load haystack from the rebar benchmarks directory
let loadHaystack (relativePath: string) =
    // Adjust path based on where the script is run from
    let paths = [
        Path.Combine("benchmarks/rebar/benchmarks/haystacks", relativePath)
        Path.Combine("../benchmarks/rebar/benchmarks/haystacks", relativePath)
        Path.Combine("../../resharp_rs/benchmarks/rebar/benchmarks/haystacks", relativePath)
    ]
    paths
    |> List.tryFind File.Exists
    |> Option.map File.ReadAllText
    |> Option.defaultWith (fun () -> 
        failwithf "Could not find haystack at any of: %A" paths)

/// Run a benchmark and return (iterations, totalMs, avgMs)
let benchmark name (iterations: int) (action: unit -> 'a) =
    // Warmup
    for _ in 1..3 do action() |> ignore
    
    GC.Collect()
    GC.WaitForPendingFinalizers()
    GC.Collect()
    
    let sw = Stopwatch.StartNew()
    for _ in 1..iterations do
        action() |> ignore
    sw.Stop()
    
    let totalMs = sw.Elapsed.TotalMilliseconds
    let avgMs = totalMs / float iterations
    printfn "  %s: %.3f ms avg (%.1f ms total, %d iters)" name avgMs totalMs iterations
    (iterations, totalMs, avgMs)

/// Format throughput
let formatThroughput (bytes: int64) (ms: float) =
    let seconds = ms / 1000.0
    let mbPerSec = (float bytes / 1024.0 / 1024.0) / seconds
    sprintf "%.1f MB/s" mbPerSec

printfn "=== F# RE# (resharp-dotnet) Benchmarks ==="
printfn ""

// Try to load the haystack
let haystack = 
    try
        loadHaystack "opensubtitles/en-medium.txt"
    with ex ->
        printfn "Warning: Could not load haystack file. Using sample text."
        String.replicate 1000 "The quick brown fox jumps over the lazy dog. Sherlock Holmes investigates. "

let haystackBytes = int64 (System.Text.Encoding.UTF8.GetByteCount haystack)
printfn "Haystack size: %d bytes (%.2f KB)" haystackBytes (float haystackBytes / 1024.0)
printfn ""

// Benchmark 1: Literal search
printfn "--- Literal Search: 'Sherlock Holmes' ---"
let literalPattern = "Sherlock Holmes"
let literalRe = Regex(literalPattern)
let _, literalMs, _ = benchmark "resharp_fsharp" 100 (fun () -> 
    literalRe.Matches(haystack) |> Seq.length)
printfn "  Throughput: %s" (formatThroughput haystackBytes literalMs)
printfn ""

// Benchmark 2: Character class - digits
printfn "--- Character Class: '[0-9]+' ---"
let digitsPattern = "[0-9]+"
let digitsRe = Regex(digitsPattern)
let _, digitsMs, _ = benchmark "resharp_fsharp" 100 (fun () ->
    digitsRe.Matches(haystack) |> Seq.length)
printfn "  Throughput: %s" (formatThroughput haystackBytes digitsMs)
printfn ""

// Benchmark 3: Alternation
printfn "--- Alternation: 'the|and|for|that|with' ---"
let altPattern = "the|and|for|that|with"
let altRe = Regex(altPattern)
let _, altMs, _ = benchmark "resharp_fsharp" 100 (fun () ->
    altRe.Matches(haystack) |> Seq.length)
printfn "  Throughput: %s" (formatThroughput haystackBytes altMs)
printfn ""

// Benchmark 4: Word characters
printfn "--- Word Characters: '[a-zA-Z]+' ---"
let wordPattern = "[a-zA-Z]+"
let wordRe = Regex(wordPattern)
let _, wordMs, _ = benchmark "resharp_fsharp" 100 (fun () ->
    wordRe.Matches(haystack) |> Seq.length)
printfn "  Throughput: %s" (formatThroughput haystackBytes wordMs)
printfn ""

// Benchmark 5: RE# extension - Intersection
printfn "--- RE# Extension: Intersection '_*cat_*&_*dog_*' ---"
let intersectPattern = "_*cat_*&_*dog_*"
let intersectRe = Regex(intersectPattern)
let testText = "the cat and the dog went for a walk"
let _, intersectMs, _ = benchmark "resharp_fsharp" 10000 (fun () ->
    intersectRe.IsMatch(testText))
printfn ""

// Benchmark 6: Compilation time
printfn "--- Compilation Time ---"
let compilePatterns = [
    ("literal", "Sherlock Holmes")
    ("char-class", "[a-zA-Z0-9]+")
    ("alternation-5", "foo|bar|baz|qux|quux")
    ("intersection", "_*foo_*&_*bar_*")
]

for (name, pattern) in compilePatterns do
    let _, _, avgMs = benchmark (sprintf "compile/%s" name) 1000 (fun () ->
        Regex(pattern))
    ()

printfn ""
printfn "=== Benchmarks Complete ==="
printfn ""
printfn "To compare with Rust resharp_rs, run:"
printfn "  cd /path/to/resharp_rs"
printfn "  cargo bench --bench comparative_benchmarks"

