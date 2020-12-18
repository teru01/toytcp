use anyhow::{Context, Result};
use std::sync::Arc;
use toytcp::packet::tcpflags;
use toytcp::tcb::Socket;
use toytcp::tcp::TCP;

fn main() -> Result<()> {
    let mut tcb = Socket::new("127.0.0.1".parse().unwrap())?;
    let _ = tcb
        .send_tcp_packet(22222, 44444, tcpflags::ACK, &[])
        .context("send error")?;
    Ok(())
}

fn serve() -> Result<()> {
    let tcp = Arc::new(TCP::new());
    let listening_socket = tcp.listen("127.0.0.1".parse().unwrap(), 40000)?;
    loop {
        let connected_socket = tcp.accept(listening_socket)?;
        let cloned_tcp = tcp.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 1024];
            loop {
                let nbytes = cloned_tcp.read(connected_socket, &mut buffer)?;
                if nbytes == 0 {
                    debug!("Connection closed.");
                    return Ok(());
                }
                print!("{}", str::from_utf8(&buffer[..nbytes])?);
                cloned_tcp.write(connected_socket, &buffer[..nbytes])?;
            }
        })
    }
}

fn connect() -> Result<()> {}

// pub fn serve(address: &str) -> Result<(), failure::Error> {
//     let listener = TcpListener::bind(address)?; /* [1] */
//     loop {
//         let (stream, _) = listener.accept()?;
//         // スレッドを立ち上げて接続に対処する。
//         thread::spawn(move || {
//             handler(stream).unwrap_or_else(|error| error!("{:?}", error));
//         });
//     }
// }

// /**
//  * クライアントからの入力を待ち受け、受信したら同じものを返却する。
//  */
// fn handler(mut stream: TcpStream) -> Result<(), failure::Error> {
//     debug!("Handling data from {}", stream.peer_addr()?);
//     let mut buffer = [0u8; 1024];
//     loop {
//         let nbytes = stream.read(&mut buffer)?;
//         if nbytes == 0 {
//             debug!("Connection closed.");
//             return Ok(());
//         }
//         print!("{}", str::from_utf8(&buffer[..nbytes])?);
//         stream.write_all(&buffer[..nbytes])?;
//     }
// }
