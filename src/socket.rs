use crate::packet::{tcpflags, TCPPacket};
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
    mpsc::{self, Receiver, Sender, SyncSender},
    Arc, Condvar, Mutex, RwLock,
};
use std::time::SystemTime;

const TCP_DATA_OFFSET: u8 = 5;
const CHANNEL_BOUND: usize = 1 << 16;
const SOCKET_BUFFER_SIZE: usize = 14600;

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
    pub local_port: u16,
    pub remote_port: u16,
    pub send_param: SendParam,
    pub recv_param: RecvParam,
    pub status: TcpStatus,
    pub send_buffer: Vec<u8>,
    pub recv_buffer: Vec<u8>,
    pub retransmission_queue: VecDeque<RetransmissionQueueEntry>,
    pub synrecv_connection_channel: VecDeque<Socket>, // いらない
    pub connected_connection_queue: VecDeque<SockID>,
    pub listening_socket: Option<SockID>, // どのリスニングソケットから生まれたか？
}

#[derive(Clone, Debug)]
pub struct RetransmissionQueueEntry {
    pub packet: TCPPacket,
    pub latest_transmission_time: SystemTime,
    pub transmission_count: u8,
}

impl RetransmissionQueueEntry {
    fn new(packet: TCPPacket) -> Self {
        Self {
            packet,
            latest_transmission_time: SystemTime::now(),
            transmission_count: 1,
        }
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
    pub fn new(
        local_addr: Ipv4Addr,
        remote_addr: Ipv4Addr,
        local_port: u16,
        remote_port: u16,
        status: TcpStatus,
    ) -> Self {
        Self {
            local_addr,
            remote_addr: remote_addr,
            local_port,
            remote_port: remote_port,
            send_param: SendParam {
                unacked_seq: 0,
                initial_seq: 0,
                next: 0,
                window: SOCKET_BUFFER_SIZE as u16,
            },
            recv_param: RecvParam {
                initial_seq: 0,
                next: 0,
                window: SOCKET_BUFFER_SIZE as u16,
            },
            status,
            send_buffer: vec![0; SOCKET_BUFFER_SIZE],
            recv_buffer: vec![0; SOCKET_BUFFER_SIZE],
            retransmission_queue: VecDeque::new(),
            synrecv_connection_channel: VecDeque::new(),
            connected_connection_queue: VecDeque::new(),
            listening_socket: None,
        }
    }

    pub fn send_tcp_packet(
        &mut self,
        seq: u32,
        ack: u32,
        flag: u8,
        payload: &[u8],
    ) -> Result<usize> {
        let mut tcp_packet = TCPPacket::new(payload.len());
        tcp_packet.set_src(self.local_port);
        tcp_packet.set_dest(self.remote_port);
        tcp_packet.set_seq(seq);
        tcp_packet.set_ack(ack);
        tcp_packet.set_data_offset(TCP_DATA_OFFSET);
        tcp_packet.set_flag(flag);
        tcp_packet.set_window_size(self.recv_param.window);
        tcp_packet.set_payload(payload);
        // tcp_packet.set_reserved(2);
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
            .context(format!("failed to send: \n{:?}", tcp_packet))?;

        dbg!("send", &tcp_packet);
        if payload.len() > 0 || tcp_packet.get_flag() != tcpflags::ACK {
            self.retransmission_queue
                .push_back(RetransmissionQueueEntry::new(tcp_packet));
        }
        Ok(sent_size)
    }

    // pub fn send_until(
    //     &mut self,
    //     max: usize,
    //     seq: u32,
    //     ack: u32,
    //     flag: u8,
    //     payload: &[u8],
    // ) -> Result<()> {
    //     let mut n = 0;
    //     while let Err(e) = self.send_tcp_packet(seq, ack, flag, payload).as_ref() {
    //         dbg!(e);
    //         n += 1;
    //         if n >= max {
    //             return Err(*e);
    //         }
    //     }
    //     Ok(())
    // }

    pub fn get_sock_id(&self) -> SockID {
        SockID(
            self.local_addr,
            self.remote_addr,
            self.local_port,
            self.remote_port,
        )
    }
}
