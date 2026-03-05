use resharp_rs::Regex;

fn main() {
    let patterns = [
        "[[:space:]]",
        "[^[:space:]]",
        "[a[:space:]]",
        "[^a[:space:]]",
        "[^[:space:],]",
        "[^,[:space:]]",
        "[^[:alpha:]Z]",
    ];

    for p in &patterns {
        match Regex::new(p) {
            Ok(_) => println!("OK: {}", p),
            Err(e) => println!("FAIL: {} -> {:?}", p, e),
        }
    }
}
