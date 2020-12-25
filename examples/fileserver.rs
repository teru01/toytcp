use anyhow::Result;
use std::{env, fs, net::Ipv4Addr, str};
use toytcp::tcp::TCP;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr: Ipv4Addr = args[1].parse()?;
    let port: u16 = args[2].parse()?;
    let savepath: &str = &args[3];
    file_server(addr, port, savepath)?;
    Ok(())
}

fn file_server(local_addr: Ipv4Addr, local_port: u16, savepath: &str) -> Result<()> {
    let tcp = TCP::new();
    let listening_socket = tcp.listen(local_addr, local_port)?;
    dbg!("listening...");
    loop {
        let connected_socket = tcp.accept(listening_socket)?;
        dbg!("accepted!", connected_socket.1, connected_socket.3);
        let mut v = Vec::new();
        let mut buffer = [0u8; 2000];
        loop {
            let nbytes = tcp.recv(connected_socket, &mut buffer).unwrap();
            if nbytes == 0 {
                dbg!("closing connection...");
                tcp.close(connected_socket).unwrap();
                break;
            }
            v.extend_from_slice(&buffer[..nbytes]);
        }
        fs::write(savepath, &v).unwrap();
    }
}
