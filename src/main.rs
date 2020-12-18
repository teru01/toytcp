use anyhow::{Context, Result};
use toytcp::packet::tcpflags;
use toytcp::tcb::TCB;

fn main() -> Result<()> {
    let mut tcb = TCB::new("127.0.0.1".parse().unwrap())?;
    let _ = tcb
        .send_tcp_packet(22222, 44444, tcpflags::ACK, &[])
        .context("send error")?;
    Ok(())
}
