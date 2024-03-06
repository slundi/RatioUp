use rand::Rng;
use tokio::net::UdpSocket;

use crate::CONFIG;

/// Get a random UDP port, if the port is used, try the next port
pub async fn get_udp_socket() -> UdpSocket {
    // TODO : Currently this function exhaustively checks for each port and tries to
    // give one of the ports incrementing from 6881
    let config = CONFIG.get().unwrap();
    //Bind to a random port
    let mut port = rand::thread_rng().gen_range(1025..65000);
    // TODO: Get a list of all the ports used by the entire application as well,
    // i.e store a global use  of entire sockets somewhere in a global state
    //
    // Gets a port that is not used by the application
    loop {
        let adr = format!("0.0.0.0:{}", config.port);
        match UdpSocket::bind(adr).await {
            Ok(socket) => {
                return socket;
            }
            Err(_e) => {
                //println!("{:?}", e.to_string());
                port = port + 1;
            }
        }
    }
}
