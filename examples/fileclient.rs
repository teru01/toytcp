use anyhow::Result;
use std::{env, fs, net::Ipv4Addr, str};
use toytcp::tcp::TCP;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr: Ipv4Addr = args[1].parse()?;
    let port: u16 = args[2].parse()?;
    let filepath: &str = &args[3];
    file_client(addr, port, filepath)?;
    Ok(())
}

fn file_client(remote_addr: Ipv4Addr, remote_port: u16, filepath: &str) -> Result<()> {
    let tcp = TCP::new();
    let sock_id = tcp.connect(remote_addr, remote_port)?;
    let cloned_tcp = tcp.clone();
    ctrlc::set_handler(move || {
        cloned_tcp.close(sock_id).unwrap();
        std::process::exit(0);
    })?;
    let input = fs::read(filepath)?;
    tcp.send(sock_id, &input)?;
    tcp.close(sock_id).unwrap();
    Ok(())
}
