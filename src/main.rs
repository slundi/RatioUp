mod client;

fn main() {
    let c:  client::Client;
    println!("RatioUp");
    for c in client::load_clients().into_iter() {
        println!("{}", c.0);
    }
    println!("input:: {}, result: \"{}\", expected: {}", 0x00, client::get_URL_encoded_char("", 0x00 as char, false), "%01");
    println!("input:: {}, result: \"{}\", expected: {}", 0x01, client::get_URL_encoded_char("", 0x01 as char, false), "%01");
}