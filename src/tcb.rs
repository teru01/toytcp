use crate::packet::TCPPacket;
use anyhow::{Context, Result};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::Packet;
use pnet::transport::{
    self, TransportChannelType, TransportProtocol, TransportReceiver, TransportSender,
};
use pnet::util;
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr};

const TCP_DATA_OFFSET: u8 = 5;

pub struct TCB {
    src_addr: Ipv4Addr,
    dest_addr: Ipv4Addr,
    src_port: u16,
    dest_port: u16,
    send_param: SendParam,
    recv_param: RecvParam,
    status: TcpStatus,
    send_buffer: Vec<u8>,
    recv_buffer: Vec<u8>,
    send_channel: TransportSender,
    recv_channel: TransportReceiver,
    retransmission_map: HashMap<u32, RetransmissionHashEntry>,
}

struct RetransmissionHashEntry {
    packet: TCPPacket,
}

impl RetransmissionHashEntry {
    fn new(packet: TCPPacket) -> Self {
        Self { packet }
    }
}

#[derive(Clone, Debug, Default)]
struct SendParam {
    unacked_seq: u32, //未ACK送信
    next: u32,        //次の送信
    window: u16,
    initial_seq: u32, //初期送信seq
}

#[derive(Clone, Debug, Default)]
struct RecvParam {
    next: u32,
    window: u16,
    initial_seq: u32, //初期受信seq
}

#[derive(PartialEq, Eq, Debug, Clone)]
enum TcpStatus {
    Listen,
    SynSent,
    SynRecv,
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
            TcpStatus::SynRecv => write!(f, "SYNRECV"),
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

impl TCB {
    pub fn new(src_addr: Ipv4Addr) -> Result<Self> {
        let (sender, receiver) = transport::transport_channel(
            65535,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?;
        Ok(Self {
            src_addr,
            dest_addr: "127.0.0.1".parse().unwrap(),
            src_port: u16::default(),
            dest_port: u16::default(),
            send_param: SendParam::default(),
            recv_param: RecvParam::default(),
            status: TcpStatus::Closed,
            send_buffer: vec![0; 65535],
            recv_buffer: vec![0; 65535],
            send_channel: sender,
            recv_channel: receiver,
            retransmission_map: HashMap::new(),
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
            &self.src_addr,
            &self.dest_addr,
            IpNextHeaderProtocols::Tcp,
        ));
        let sent_size = self
            .send_channel
            .send_to(tcp_packet.clone(), IpAddr::V4(self.dest_addr))
            .context(format!("failed to send: \n{}", tcp_packet))?;

        self.retransmission_map
            .insert(seq, RetransmissionHashEntry::new(tcp_packet));
        Ok(sent_size)
    }
}
