//! Input Validation Example
//!
//! This example demonstrates how to use resharp_rs's extended regex features
//! (intersection & complement) for complex input validation scenarios.
//!
//! Run with: cargo run --example validation

use resharp_rs::Regex;

fn main() {
    println!("=== resharp_rs Input Validation Examples ===\n");

    // Example 1: Password validation using intersection
    // Must be 8-20 chars AND contain a digit AND contain a letter
    println!("--- Password Validation ---");
    let mut password_re = Regex::new(r".*[0-9].*&.*[a-zA-Z].*&.{8,20}").unwrap();

    let passwords = [
        ("secret123", true), // valid: has letter, digit, 8+ chars
        ("abcdefgh", false), // invalid: no digit
        ("12345678", false), // invalid: no letter
        ("abc123", false),   // invalid: too short
        ("MySecurePassword99", true),
    ];

    for (pwd, expected) in passwords {
        let valid = password_re.is_match(pwd);
        let status = if valid == expected { "✓" } else { "✗" };
        println!(
            "  {} '{}' -> {}",
            status,
            pwd,
            if valid { "valid" } else { "invalid" }
        );
    }

    // Example 2: Username validation using complement
    // Alphanumeric only, no spaces or special chars, 3-16 chars
    println!("\n--- Username Validation ---");
    let mut username_re = Regex::new(r"[a-zA-Z0-9]{3,16}").unwrap();

    let usernames = [
        ("alice", true),
        ("bob_smith", false), // underscore not allowed
        ("user 123", false),  // space not allowed
        ("ab", false),        // too short
        ("validUsername99", true),
    ];

    for (user, expected) in usernames {
        let valid = username_re.is_match(user)
            && username_re
                .find(user)
                .map(|m| m.end - m.start == user.len())
                .unwrap_or(false);
        let status = if valid == expected { "✓" } else { "✗" };
        println!(
            "  {} '{}' -> {}",
            status,
            user,
            if valid { "valid" } else { "invalid" }
        );
    }

    // Example 3: Finding strings that contain a keyword
    println!("\n--- Log Filtering ---");
    // Match "error" or "Error" or "ERROR" etc.
    let mut error_re = Regex::new(r"_*([Ee][Rr][Rr][Oo][Rr]|ERROR)_*").unwrap();

    let log_lines = [
        "INFO: Server started",
        "ERROR: Connection failed",
        "DEBUG: Processing request",
        "WARN: Timeout error detected",
    ];

    println!("  Filtering for lines containing 'error':");
    for line in log_lines {
        if error_re.is_match(line) {
            println!("    ⚠ {}", line);
        } else {
            println!("    ✓ {}", line);
        }
    }

    // Example 4: Complex intersection - text containing multiple keywords
    println!("\n--- Multi-Keyword Search ---");
    let mut multi_keyword_re = Regex::new(r"_*cat_*&_*dog_*").unwrap();

    let texts = [
        "The cat and dog are friends",
        "My cat sleeps all day",
        "The dog barks loudly",
        "A dog chased the cat",
    ];

    println!("  Texts containing BOTH 'cat' AND 'dog':");
    for text in texts {
        let matches = multi_keyword_re.is_match(text);
        let icon = if matches { "✓" } else { "✗" };
        println!("    {} {}", icon, text);
    }

    // Example 5: Extract all word-like tokens
    println!("\n--- Token Extraction ---");
    let mut word_re = Regex::new(r"[a-zA-Z]+").unwrap();
    let text = "Hello, world! This is resharp_rs v0.1.";
    let matches = word_re.find_all(text);

    println!("  Tokens from: \"{}\"", text);
    print!("  Found {} words: ", matches.len());
    for m in &matches {
        print!("'{}' ", m.as_str(text));
    }
    println!();

    println!("\n=== Examples Complete ===");
}
