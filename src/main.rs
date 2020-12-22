use anyhow::{Context, Result};
use std::sync::Arc;
use std::{
    env,
    io::{self, BufReader},
    str,
};
use toytcp::packet::tcpflags;
use toytcp::socket::{SockID, Socket};
use toytcp::tcp::TCP;

fn main() -> Result<()> {
    // let mut socket = Socket::new("127.0.0.1".parse().unwrap())?;
    // let _ = socket
    //     .send_tcp_packet(22222, 44444, tcpflags::ACK, &[])
    //     .context("send error")?;
    let args: Vec<String> = env::args().collect();
    let role: &str = &args[1];
    match role {
        "server" => serve()?,
        "client" => connect()?,
        _ => unimplemented!(),
    }
    // connect()?;
    Ok(())
}

fn serve() -> Result<()> {
    let tcp = TCP::new();
    let listening_socket = tcp.listen(toytcp::MY_IPADDR, 40000)?;
    // let listening_socket = tcp.listen("192.168.69.100".parse().unwrap(), 40000)?;
    dbg!("listening..");
    let cloned_tcp = tcp.clone();
    loop {
        let connected_socket = tcp.accept(listening_socket)?;
        dbg!("accepted!", connected_socket.1, connected_socket.3);
        let cloned_tcp = tcp.clone();

        std::thread::spawn(move || {
            let mut buffer = [0u8; 1024];
            loop {
                let nbytes = cloned_tcp.receive(connected_socket, &mut buffer).unwrap();
                if nbytes == 0 {
                    dbg!("closing connection...");
                    cloned_tcp.close(connected_socket).unwrap();
                    return;
                }
                print!("> {}", str::from_utf8(&buffer[..nbytes]).unwrap());
                cloned_tcp
                    .send(connected_socket, &buffer[..nbytes])
                    .unwrap();
            }
        });
    }
}

fn connect() -> Result<()> {
    let tcp = TCP::new();
    let sock_id = tcp.connect("10.0.0.1".parse().unwrap(), 40000)?;
    let cloned_tcp = tcp.clone();
    ctrlc::set_handler(move || {
        cloned_tcp.close(sock_id).unwrap();
        std::process::exit(0);
    })?;
    loop {
        dbg!("loop");
        // 入力データをソケットから送信。
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        dbg!("input: ", &input);

        tcp.send(sock_id, input.as_bytes())?;

        // ソケットから受信したデータを表示。
        let mut buffer = vec![0; 1500];
        let n = tcp.receive(sock_id, &mut buffer)?;
        print!("> {}", str::from_utf8(&buffer[..n])?);
    }
}
