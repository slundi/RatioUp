extern crate rand;

mod client;
mod algorithm;

fn main() {
    let c:  client::Client;
    println!("RatioUp");
    for c in client::load_clients().into_iter() {
        println!("{}", c.0);
    }
}