use crate::packet::TCPPacket;
use anyhow::{Context, Result};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::Packet;
use pnet::transport::{
    self, TransportChannelType, TransportProtocol, TransportReceiver, TransportSender,
};
use pnet::util;
use std::collections::{HashMap, VecDeque};
use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc, Mutex, RwLock,
};

const TCP_DATA_OFFSET: u8 = 5;

// enum Socket {
//     ListenSocket(Socket),
//     ConnectionSocket(Socket),
// }
// srcIP, destIP, srcPort, destPort
#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub struct SockID(pub Ipv4Addr, pub Ipv4Addr, pub u16, pub u16);

#[derive(Debug)]
pub struct Socket {
    pub local_addr: Ipv4Addr,
    pub remote_addr: Ipv4Addr,
    pub src_port: u16,
    pub dest_port: u16,
    pub send_param: SendParam,
    pub recv_param: RecvParam,
    pub status: TcpStatus,
    pub send_buffer: Vec<u8>,
    pub recv_buffer: Vec<u8>,
    retransmission_map: HashMap<u32, RetransmissionHashEntry>,
    pub synrecv_connection_channel: VecDeque<Socket>, // いらない
    pub connected_connection_queue: VecDeque<SockID>,
    pub event_channel: (Mutex<Sender<TCPEvent>>, Mutex<Receiver<TCPEvent>>),
}

pub enum TCPEvent {
    ConnectionCompleted,
}

#[derive(Clone, Debug)]
struct RetransmissionHashEntry {
    packet: TCPPacket,
}

impl RetransmissionHashEntry {
    fn new(packet: TCPPacket) -> Self {
        Self { packet }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SendParam {
    pub unacked_seq: u32, //未ACK送信
    pub next: u32,        //次の送信
    pub window: u16,
    pub initial_seq: u32, //初期送信seq
}

#[derive(Clone, Debug, Default)]
pub struct RecvParam {
    pub next: u32,
    pub window: u16,
    pub initial_seq: u32, //初期受信seq
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TcpStatus {
    Listen,
    SynSent,
    SynRcvd,
    Established,
    FinWait1,
    FinWait2,
    Closing,
    TimeWait,
    CloseWait,
    LastAck,
    Closed,
}

impl Display for TcpStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TcpStatus::Listen => write!(f, "LISTEN"),
            TcpStatus::SynSent => write!(f, "SYNSENT"),
            TcpStatus::SynRcvd => write!(f, "SynRcvd"),
            TcpStatus::Established => write!(f, "ESTABLISHED"),
            TcpStatus::FinWait1 => write!(f, "FINWAIT1"),
            TcpStatus::FinWait2 => write!(f, "FINWAIT2"),
            TcpStatus::Closing => write!(f, "CLOSING"),
            TcpStatus::TimeWait => write!(f, "TIMEWAIT"),
            TcpStatus::CloseWait => write!(f, "CLOSEWAIT"),
            TcpStatus::LastAck => write!(f, "LASTACK"),
            TcpStatus::Closed => write!(f, "CLOSED"),
        }
    }
}

impl Socket {
    pub fn new(local_addr: Ipv4Addr, src_port: u16, status: TcpStatus) -> Result<Self> {
        let (s, r) = mpsc::channel();
        Ok(Self {
            local_addr,
            remote_addr: "127.0.0.1".parse().unwrap(),
            src_port,
            dest_port: u16::default(),
            send_param: SendParam::default(),
            recv_param: RecvParam::default(),
            status,
            send_buffer: vec![0; 65535],
            recv_buffer: vec![0; 65535],
            retransmission_map: HashMap::new(),
            synrecv_connection_channel: VecDeque::new(),
            connected_connection_queue: VecDeque::new(),
            event_channel: (Mutex::new(s), Mutex::new(r)),
        })
    }

    pub fn send_tcp_packet(
        &mut self,
        seq: u32,
        ack: u32,
        flag: u8,
        payload: &[u8],
    ) -> Result<usize> {
        let mut tcp_packet = TCPPacket::new();
        tcp_packet.set_src(self.src_port);
        tcp_packet.set_dest(self.dest_port);
        tcp_packet.set_seq(seq);
        tcp_packet.set_ack(ack);
        tcp_packet.set_data_offset(TCP_DATA_OFFSET);
        tcp_packet.set_flag(flag);
        tcp_packet.set_window_size(self.recv_param.window);
        tcp_packet.set_payload(payload);
        tcp_packet.set_checksum(util::ipv4_checksum(
            &tcp_packet.packet(),
            8,
            &[],
            &self.local_addr,
            &self.remote_addr,
            IpNextHeaderProtocols::Tcp,
        ));
        let (mut sender, _) = transport::transport_channel(
            65535,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?; // TODO FIX
        let sent_size = sender
            .send_to(tcp_packet.clone(), IpAddr::V4(self.remote_addr))
            .context(format!("failed to send: \n{}", tcp_packet))?;

        self.retransmission_map
            .insert(seq, RetransmissionHashEntry::new(tcp_packet));
        Ok(sent_size)
    }

    pub fn get_sock_id(&self) -> SockID {
        SockID(
            self.local_addr,
            self.remote_addr,
            self.src_port,
            self.dest_port,
        )
    }
}
