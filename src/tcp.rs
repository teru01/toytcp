use crate::socket::{Socket, TcpStatus};
use anyhow::{Context, Result};
use std::collections::{HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::sync::RwLock;
const UNDETERMINED_IP_ADDR: std::net::Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const UNDETERMINED_PORT: u16 = 0;

// srcIP, destIP, srcPort, destPort
#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub struct SockID(Ipv4Addr, Ipv4Addr, u16, u16);

pub struct TCP {
    tcbs: RwLock<HashMap<SockID, Socket>>,
}

impl TCP {
    pub fn new() -> Self {
        Self {
            tcbs: RwLock::new(HashMap::new()),
        }
    }

    pub fn listen(&self, src_addr: Ipv4Addr, src_port: u16) -> Result<SockID> {
        let tcb = Socket::new(src_addr, src_port, TcpStatus::Listen)?;
        let socket_id = SockID(src_addr, UNDETERMINED_IP_ADDR, src_port, UNDETERMINED_PORT);
        self.tcbs.write().unwrap().insert(socket_id, tcb);
        Ok(socket_id)
    }

    // pub fn accept(&self, socket_id) -> Result<SockID> {}
}
