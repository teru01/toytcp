use crate::tcb::TCB;
use std::collections::{HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::sync::RwLock;

// srcIP, destIP, srcPort, destPort
type SockID = (Ipv4Addr, Ipv4Addr, u16, u16);

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
}
