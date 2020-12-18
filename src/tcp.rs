use anyhow::{Context, Error, Result};
use pnet::packet::tcp::{self, MutableTcpPacket, TcpPacket};
use pnet::packet::Packet;
use pnet::transport::{TransportReceiver, TransportSender};
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr};

const TCP_HEADER_SIZE: usize = 20;
const TCP_DATA_OFFSET: u8 = 5;

struct TCB<'a> {
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
    retransmission_map: HashMap<u32, RetransmissionHashEntry<'a>>,
}

struct RetransmissionHashEntry<'a> {
    packet: TcpPacket<'a>,
}

impl<'a> RetransmissionHashEntry<'a> {
    fn new(packet: TcpPacket<'a>) -> Self {
        Self { packet }
    }
}

#[derive(Clone, Debug)]
struct SendParam {
    unacked_seq: u32, //未ACK送信
    next: u32,        //次の送信
    window: u16,
    initial_seq: u32, //初期送信seq
}

#[derive(Clone, Debug)]
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

impl<'a> TCB<'a> {
    fn send_tcp_packet(&mut self, seq: u32, ack: u32, flag: u16, payload: &[u8]) -> Result<usize> {
        let mut tcp_buffer = vec![0; TCP_HEADER_SIZE + payload.len()];
        let mut tcp_packet =
            MutableTcpPacket::owned(tcp_buffer).context("failed to create packet")?;
        tcp_packet.set_source(self.src_port);
        tcp_packet.set_destination(self.dest_port);
        tcp_packet.set_sequence(seq);
        tcp_packet.set_acknowledgement(ack);
        tcp_packet.set_data_offset(TCP_DATA_OFFSET);
        tcp_packet.set_flags(flag);
        tcp_packet.set_window(self.recv_param.window);
        tcp_packet.set_payload(payload);
        tcp_packet.set_checksum(tcp::ipv4_checksum(
            &tcp_packet.to_immutable(),
            &self.src_addr,
            &self.dest_addr,
        ));
        let sent_size = self.send_to(&tcp_packet.to_immutable())?;
        let t = TcpPacket::new(tcp_packet.packet());
        self.retransmission_map.insert(
            seq,
            RetransmissionHashEntry::new(tcp_packet.consume_to_immutable()),
        );
        Ok((sent_size))
    }

    // TCPパケットを送信
    fn send_to(&mut self, packet: &TcpPacket) -> Result<usize> {
        let packet_buf = packet.packet();
        let mut buffer = vec![0; packet_buf.len()];
        buffer.copy_from_slice(packet_buf);
        let new_packet = TcpPacket::new(&buffer).context("failed to create new TCP packet")?;
        self.send_channel
            .send_to(new_packet, IpAddr::V4(self.dest_addr))
            .map_err(|e| Error::new(e))
    }
}
