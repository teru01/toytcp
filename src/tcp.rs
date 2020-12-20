use crate::packet::{tcpflags, TCPPacket};
use crate::socket::{SockID, Socket, TCPEvent, TcpStatus};
use anyhow::{Context, Result};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::Packet;
use pnet::transport::{
    self, TransportChannelType, TransportProtocol, TransportReceiver, TransportSender,
};
use pnet::util;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
const UNDETERMINED_IP_ADDR: std::net::Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const UNDETERMINED_PORT: u16 = 0;

// type CondMutex = (Mutex<bool>, Condvar);

pub struct TCP {
    sockets: RwLock<HashMap<SockID, Socket>>,
    // locker: Arc<CondMutex>
    // event_channel: Arc<Receiver<TCPEvent>>,
    my_ip: Ipv4Addr,
    pub event_cond: (Mutex<Option<SockID>>, Condvar),
}

impl TCP {
    pub fn new() -> Arc<Self> {
        // let (sender, reciever) = mpsc::channel();
        let sockets = RwLock::new(HashMap::new());
        let tcp = Arc::new(Self {
            sockets, // event_channel: Arc::new(reciever),
            my_ip: "127.0.0.1".parse().unwrap(),
            // my_ip: "192.168.69.100".parse().unwrap(),
            event_cond: (Mutex::new(None), Condvar::new()),
        });
        let cloned_tcp = tcp.clone();
        // let cloned_sockets = sockets.clone();
        std::thread::spawn(move || {
            // 受信スレッドではtableとsenderに触りたい
            cloned_tcp.receive_handler();
        });
        // ハンドラスレッドではtableとreceiverに触りたい
        tcp
    }

    /// リスニングソケットを生成してIDを返す
    pub fn listen(&self, local_addr: Ipv4Addr, local_port: u16) -> Result<SockID> {
        let socket = Socket::new(
            local_addr,
            UNDETERMINED_IP_ADDR,
            local_port,
            UNDETERMINED_PORT,
            TcpStatus::Listen,
        )?;
        let mut lock = self.sockets.write().unwrap();
        let sock_id = socket.get_sock_id();
        lock.insert(sock_id, socket);
        Ok(sock_id)
    }

    /// 接続済みソケットが生成されるまで待機し，されたらそのIDを返す
    /// コネクション確立キューにエントリが入るまでブロック
    /// エントリはrecvスレッドがいれる
    pub fn accept(&self, socket_id: SockID) -> Result<SockID> {
        let (lock, cvar) = &self.event_cond;
        let mut event = lock.lock().unwrap();
        while event.is_none() || event.is_some() && event.unwrap() != socket_id {
            // イベントが来てない or 来てたとしても関係ないなら待機
            // cvarに通知が来るまでeventをunlockする
            // 通知が来たらlockをとってリターン
            event = cvar.wait(event).unwrap();
            dbg!("wake up");
        }
        *event = None;

        let mut table = self.sockets.write().unwrap();
        Ok(table
            .get_mut(&socket_id)
            .unwrap()
            .connected_connection_queue
            .pop_front()
            .context("no connected socket")?)
    }

    /// ターゲットに接続し，接続済みソケットのIDを返す
    pub fn connect(&self, addr: Ipv4Addr, port: u16) -> Result<SockID> {
        // create socket
        // send SYN
        // to SYNSENT
        // lock table insert
        // unlock
        // select
        // <- ESTAB event
        // to ESTAB
        // lock table insert
        // return sockid
        // time up
        //
        //  send SYN
        // let socket = Socket::new(local_addr, addr, local_port, port, status: TcpStatus);
        unimplemented!()
    }

    fn receive_handler(&self) -> Result<()> {
        dbg!("begin recv thread");
        let (mut sender, mut receiver) = transport::transport_channel(
            65535,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?; // TODO FIX
        let mut packet_iter = transport::tcp_packet_iter(&mut receiver);
        loop {
            let (packet, remote_addr) = packet_iter.next()?;
            let packet = TCPPacket::from(packet);
            // let packet = translate_packet()
            let remote_addr = match remote_addr {
                IpAddr::V4(addr) => addr,
                _ => continue,
            };
            if !(remote_addr == "127.0.0.1".parse::<Ipv4Addr>().unwrap()
            // if !(remote_addr == "192.168.69.101".parse::<Ipv4Addr>().unwrap()
                && packet.get_dest() == 40000)
            {
                continue;
            }
            dbg!("incoming from", &remote_addr, packet.get_src());
            let mut table = self.sockets.write().unwrap();
            dbg!("write lock");
            let socket = match table.get_mut(&SockID(
                self.my_ip,
                remote_addr,
                packet.get_dest(),
                packet.get_src(),
            )) {
                Some(socket) => {
                    dbg!("connected socket", socket.get_sock_id());
                    socket // 接続済みソケット
                }
                None => match table.get_mut(&SockID(
                    self.my_ip,
                    UNDETERMINED_IP_ADDR,
                    packet.get_dest(),
                    UNDETERMINED_PORT,
                )) {
                    Some(socket) => {
                        dbg!("listening socket", socket.get_sock_id());
                        socket
                    } // リスニングソケット
                    None => {
                        unimplemented!();
                    }
                }, // return RST
                   // unimplemented!();
            };
            // dbg!("socket found: {:?}", &socket);
            // checksum, ack検証

            // ホントはちゃんとエラー処理
            match socket.status {
                TcpStatus::Listen => {
                    dbg!("listen handler");
                    // check RST
                    // check ACK
                    if packet.get_flag() & tcpflags::SYN > 0 {
                        let mut connection_socket = Socket::new(
                            socket.local_addr,
                            remote_addr,
                            socket.local_port,
                            packet.get_src(),
                            TcpStatus::SynRcvd,
                        )?;
                        connection_socket.recv_param.next = packet.get_seq() + 1;
                        connection_socket.recv_param.initial_seq = packet.get_seq();
                        connection_socket.send_param.initial_seq = 443322; // TODO random
                        connection_socket.send_tcp_packet(
                            connection_socket.send_param.initial_seq,
                            connection_socket.recv_param.next,
                            tcpflags::SYN | tcpflags::ACK,
                            &[],
                        )?;
                        connection_socket.send_param.next =
                            connection_socket.send_param.initial_seq + 1;
                        connection_socket.send_param.unacked_seq =
                            connection_socket.send_param.initial_seq;
                        connection_socket.listening_socket = Some(socket.get_sock_id());
                        dbg!(socket.get_sock_id());
                        dbg!("status: listen → synrcvd");
                        table.insert(connection_socket.get_sock_id(), connection_socket);
                    }
                }
                TcpStatus::SynRcvd => {
                    dbg!("synrcvd handler");
                    // check RST
                    // check SYN
                    if packet.get_flag() & tcpflags::ACK > 0 {
                        if socket.send_param.unacked_seq <= packet.get_ack()
                            && packet.get_ack() <= socket.send_param.next
                        {
                            socket.recv_param.next = packet.get_seq();
                            socket.send_param.unacked_seq = packet.get_ack();
                            socket.status = TcpStatus::Established;
                            let connection_sock_id = socket.get_sock_id();
                            if let Some(id) = socket.listening_socket {
                                let ls = table.get_mut(&id).unwrap();
                                ls.connected_connection_queue.push_back(connection_sock_id);
                                let (lock, cvar) = &self.event_cond;
                                let mut ready = lock.lock().unwrap();
                                *ready = Some(ls.get_sock_id());
                                cvar.notify_one();
                            }
                            dbg!("status: synrcvd → established");
                        } else {
                            dbg!("invalid params");
                        }
                    } else {
                        dbg!("unexpected flag");
                    }
                }
                _ => unimplemented!(),
            }
        }
    }
}

// fn receive_handler(
//     sockets: Arc<RwLock<HashMap<SockID, Socket>>>,
//     sender: Sender<TCPEvent>,
// ) -> Result<()> {
//     // recv
//     // look sock_id
//     // s = table.write().get(sock_id) or self.pair.clone()
//     //
//     dbg!("begin recv thread");
//     let (mut sender, mut receiver) = transport::transport_channel(
//         65535,
//         TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
//     )?; // TODO FIX
//     let mut packet_iter = transport::tcp_packet_iter(&mut receiver);
//     loop {
//         let (packet, src_addr) = packet_iter.next()?;
//         let src_addr = match src_addr {
//             IpAddr::V4(addr) => addr,
//             _ => continue,
//         };
//     }
// }
