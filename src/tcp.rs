use crate::tcb::{TcpStatus, TCB};
use anyhow::{Context, Result};
use std::collections::{HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::sync::RwLock;
const UNDETERMINED_IP_ADDR: std::net::Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const UNDETERMINED_PORT: u16 = 0;

// srcIP, destIP, srcPort, destPort
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
struct SockID(Ipv4Addr, Ipv4Addr, u16, u16);

pub struct TCP {
    tcbs: RwLock<HashMap<SockID, TCB>>,
    synrecv_connection_queue: VecDeque<TCB>,
    connected_connection_queue: VecDeque<TCB>,
}

impl TCP {
    pub fn new() -> Self {
        Self {
            tcbs: RwLock::new(HashMap::new()),
            synrecv_connection_queue: VecDeque::new(),
            connected_connection_queue: VecDeque::new(),
        }
    }

    pub fn listen(&self, src_addr: Ipv4Addr, src_port: u16) -> Result<TCB> {
        let tcb = TCB::new(src_addr, src_port, TcpStatus::Listen)?;
        self.tcbs.write().unwrap().insert(
            SockID(src_addr, UNDETERMINED_IP_ADDR, src_port, UNDETERMINED_PORT),
            tcb.clone(),
        );
        Ok(tcb)
    }
}
