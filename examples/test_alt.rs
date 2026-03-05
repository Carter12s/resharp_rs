use resharp_rs::Regex;

fn main() {
    let mut re = Regex::new("|b").unwrap();
    let matches = re.find_all("abc");
    println!("|b on 'abc': {:?}", matches);

    let mut re2 = Regex::new("b|").unwrap();
    let matches2 = re2.find_all("abc");
    println!("b| on 'abc': {:?}", matches2);

    let mut re3 = Regex::new("a|ab").unwrap();
    let matches3 = re3.find_all("ab");
    println!("a|ab on 'ab': {:?}", matches3);
}
