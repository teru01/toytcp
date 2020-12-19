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
}

impl TCP {
    pub fn new() -> Arc<Self> {
        // let (sender, reciever) = mpsc::channel();
        let sockets = RwLock::new(HashMap::new());
        let tcp = Arc::new(Self {
            sockets, // event_channel: Arc::new(reciever),
            my_ip: "127.0.0.1".parse().unwrap(),
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
    pub fn listen(&self, local_addr: Ipv4Addr, src_port: u16) -> Result<SockID> {
        let socket = Socket::new(local_addr, src_port, TcpStatus::Listen)?;
        let socket_id = SockID(
            local_addr,
            UNDETERMINED_IP_ADDR,
            src_port,
            UNDETERMINED_PORT,
        );
        self.sockets.write().unwrap().insert(socket_id, socket);
        Ok(socket_id)
    }

    /// 接続済みソケットが生成されるまで待機し，されたらそのIDを返す
    /// コネクション確立キューにエントリが入るまでブロック
    /// エントリはrecvスレッドがいれる
    pub fn accept(&self, socket_id: SockID) -> Result<SockID> {
        // チャネルを使えばいい感じになると思ったが，リードロックをとってしまっているので他スレッドが書き込めない
        // チャネルをTCPに持たせて，そのタイミングでロック取れば．．？
        // let listening_socket = self
        //     .sockets
        //     .read()
        //     .unwrap()
        //     .get(&socket_id)
        //     .context("no such socket")?;
        // listening_socket.connected_connection_channel.1.recv();

        unimplemented!();
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
        unimplemented!()
    }
    fn receive_handler(&self) -> Result<()> {
        // recv
        // look sock_id
        // s = table.write().get(sock_id) or self.pair.clone()
        //
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
            let mut table = self.sockets.write().unwrap();
            let socket = match table.get_mut(&SockID(
                self.my_ip,
                remote_addr,
                packet.get_dest(),
                packet.get_src(),
            )) {
                Some(socket) => socket, // 接続済みソケット
                None => match table.get_mut(&SockID(
                    self.my_ip,
                    UNDETERMINED_IP_ADDR,
                    packet.get_dest(),
                    UNDETERMINED_PORT,
                )) {
                    Some(socket) => socket, // リスニングソケット
                    None => {
                        unimplemented!();
                    }
                }, // return RST
                                         // unimplemented!();
            };
            dbg!("socket found: {:?}", &socket);
            // checksum, ack検証
            if let Err(e) = match socket.status {
                TcpStatus::Listen => self.listen_handler(&packet, socket, remote_addr),
                // TcpStatus::SynRcvd => self.synrcvd_handler(),
                _ => unimplemented!(),
            } {
                dbg!("error, {}", e);
            }
        }
    }

    fn listen_handler(
        &self,
        packet: &TCPPacket,
        listening_socket: &mut Socket,
        remote_addr: Ipv4Addr,
    ) -> Result<()> {
        // check RST
        // check ACK
        if packet.get_flag() & tcpflags::SYN > 0 {
            let mut socket = Socket::new(
                listening_socket.local_addr,
                listening_socket.src_port,
                TcpStatus::SynRcvd,
            )?;
            socket.remote_addr = remote_addr;
            socket.dest_port = packet.get_dest();
            socket.recv_param.next = packet.get_seq() + 1;
            socket.recv_param.initial_seq = packet.get_seq();
            socket.send_param.initial_seq = 443322; // TODO random
            socket.send_tcp_packet(
                socket.send_param.initial_seq,
                socket.recv_param.next,
                tcpflags::SYN | tcpflags::ACK,
                &[],
            )?;
            socket.send_param.next = socket.send_param.initial_seq + 1;
            socket.send_param.unacked_seq = socket.send_param.initial_seq;
            self.sockets.write().unwrap().insert(
                SockID(
                    listening_socket.local_addr,
                    remote_addr,
                    listening_socket.src_port,
                    packet.get_src(),
                ),
                socket,
            );
        }
        Ok(())
    }

    fn synrcvd_handler(&self, packet: &TCPPacket, socket: &mut Socket) -> Result<()> {
        // check RST
        // check SYN
        if packet.get_flag() & tcpflags::ACK > 0 {
            if socket.send_param.unacked_seq <= packet.get_ack()
                && packet.get_ack() <= socket.send_param.next
            {
                socket.recv_param.next = packet.get_seq();
                socket.send_param.unacked_seq = packet.get_ack();
                socket.status = TcpStatus::Established;
                socket
                    .connected_connection_queue
                    .push_back(socket.get_sock_id());
                socket
                    .event_channel
                    .0
                    .lock()
                    .unwrap()
                    .send(TCPEvent::ConnectionCompleted)?; // ブロックさせてはダメ
            }
        }
        Ok(())
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
