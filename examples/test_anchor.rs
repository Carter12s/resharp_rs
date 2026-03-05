use resharp_rs::Regex;

fn main() {
    println!("=== Test 1: ^abc on 'abc' ===");
    let mut regex = Regex::new("^abc").unwrap();
    let result = regex.find("abc");
    println!(
        "Result: {:?}, Expected: Some(Match {{ start: 0, end: 3 }})",
        result
    );

    println!("\n=== Test 2: abc on 'abc' ===");
    let mut regex2 = Regex::new("abc").unwrap();
    let result2 = regex2.find("abc");
    println!(
        "Result: {:?}, Expected: Some(Match {{ start: 0, end: 3 }})",
        result2
    );

    println!("\n=== Test 3: ^$ on '' ===");
    let mut regex3 = Regex::new("^$").unwrap();
    let result3 = regex3.find("");
    println!(
        "Result: {:?}, Expected: Some(Match {{ start: 0, end: 0 }})",
        result3
    );

    println!("\n=== Test 4: ^ on 'abc' ===");
    let mut regex4 = Regex::new("^").unwrap();
    let result4 = regex4.find("abc");
    println!(
        "Result: {:?}, Expected: Some(Match {{ start: 0, end: 0 }})",
        result4
    );

    println!("\n=== Test 5: $ on '' ===");
    let mut regex5 = Regex::new("$").unwrap();
    let result5 = regex5.find("");
    println!(
        "Result: {:?}, Expected: Some(Match {{ start: 0, end: 0 }})",
        result5
    );

    println!("\n=== Test 6: $ on 'abc' ===");
    let mut regex6 = Regex::new("$").unwrap();
    let result6 = regex6.find("abc");
    println!(
        "Result: {:?}, Expected: Some(Match {{ start: 3, end: 3 }})",
        result6
    );

    // Summary
    println!("\n=== Summary ===");
    let tests = [
        (
            result.map(|m| (m.start, m.end)),
            Some((0, 3)),
            "^abc on 'abc'",
        ),
        (
            result2.map(|m| (m.start, m.end)),
            Some((0, 3)),
            "abc on 'abc'",
        ),
        (result3.map(|m| (m.start, m.end)), Some((0, 0)), "^$ on ''"),
        (
            result4.map(|m| (m.start, m.end)),
            Some((0, 0)),
            "^ on 'abc'",
        ),
        (result5.map(|m| (m.start, m.end)), Some((0, 0)), "$ on ''"),
        (
            result6.map(|m| (m.start, m.end)),
            Some((3, 3)),
            "$ on 'abc'",
        ),
    ];

    let mut passed = 0;
    let mut failed = 0;
    for (actual, expected, name) in tests {
        if actual == expected {
            println!("✓ {}", name);
            passed += 1;
        } else {
            println!("✗ {}: got {:?}, expected {:?}", name, actual, expected);
            failed += 1;
        }
    }
    println!("\nPassed: {}, Failed: {}", passed, failed);
}
