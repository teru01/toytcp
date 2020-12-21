use crate::packet::{tcpflags, TCPPacket};
use crate::socket::{SockID, Socket, TCPEvent, TcpStatus};
use anyhow::{Context, Result};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::Packet;
use pnet::transport::{
    self, TransportChannelType, TransportProtocol, TransportReceiver, TransportSender,
};
use pnet::util;
use rand::Rng;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
const UNDETERMINED_IP_ADDR: std::net::Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const UNDETERMINED_PORT: u16 = 0;
const WINDOW_SIZE: u16 = 65535;
use crate::MY_IPADDR;
const MAX_RETRANSMITTION: u8 = 3;
const RETRANSMITTION_TIMEOUT: u64 = 3;
use std::time::{Duration, SystemTime};

pub struct TCP {
    sockets: RwLock<HashMap<SockID, Socket>>,
    pub event_cond: (Mutex<Option<SockID>>, Condvar),
}

impl TCP {
    pub fn new() -> Arc<Self> {
        let sockets = RwLock::new(HashMap::new());
        let tcp = Arc::new(Self {
            sockets,
            event_cond: (Mutex::new(None), Condvar::new()),
        });
        let cloned_tcp = tcp.clone();
        std::thread::spawn(move || {
            // 受信スレッドではtableとsenderに触りたい
            cloned_tcp.receive_handler();
        });
        let cloned_tcp = tcp.clone();
        std::thread::spawn(move || {
            cloned_tcp.timer();
        });
        tcp
    }

    // タイマースレッド用
    // 全てのソケットの再送キューを見て，タイムアウトしているパケットを再送する
    fn timer(&self) {
        loop {
            let mut table = self.sockets.write().unwrap();
            for (_, socket) in table.iter_mut() {
                while let Some(mut item) = socket.retransmission_queue.pop_front() {
                    if socket.send_param.unacked_seq > item.packet.get_seq() {
                        // ackされてる
                        dbg!("successfully acked", item.packet.get_seq());
                        continue;
                    }
                    // タイムアウトを確認
                    if item.latest_transmission_time.elapsed().unwrap()
                        < Duration::from_secs(RETRANSMITTION_TIMEOUT)
                    {
                        // 取り出したエントリがタイムアウトしてないなら，キューの以降のエントリもタイムアウトしてない
                        // 先頭に戻す
                        socket.retransmission_queue.push_front(item);
                        continue;
                    }
                    // ackされてなければ再送
                    if item.transmission_count < MAX_RETRANSMITTION {
                        // 再送
                        let (mut sender, _) = transport::transport_channel(
                            65535,
                            TransportChannelType::Layer4(TransportProtocol::Ipv4(
                                IpNextHeaderProtocols::Tcp,
                            )),
                        )
                        .unwrap(); // TODO FIX
                        dbg!("retransmit", item.packet.get_seq());
                        let sent_size = sender
                            .send_to(item.packet.clone(), IpAddr::V4(socket.remote_addr))
                            .context(format!("failed to retransmit"))
                            .unwrap();
                        item.transmission_count += 1;
                        item.latest_transmission_time = SystemTime::now();
                        socket.retransmission_queue.push_back(item);
                    } else {
                        dbg!("reached MAX_RETRANSMITTION");
                    }
                }
            }
            drop(table);
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// リスニングソケットを生成してIDを返す
    pub fn listen(&self, local_addr: Ipv4Addr, local_port: u16) -> Result<SockID> {
        let socket = Socket::new(
            local_addr,
            UNDETERMINED_IP_ADDR,
            local_port,
            UNDETERMINED_PORT,
            TcpStatus::Listen,
        );
        let mut lock = self.sockets.write().unwrap();
        let sock_id = socket.get_sock_id();
        lock.insert(sock_id, socket);
        Ok(sock_id)
    }

    /// 接続済みソケットが生成されるまで待機し，されたらそのIDを返す
    /// コネクション確立キューにエントリが入るまでブロック
    /// エントリはrecvスレッドがいれる
    pub fn accept(&self, sock_id: SockID) -> Result<SockID> {
        let (lock, cvar) = &self.event_cond;
        let mut event = lock.lock().unwrap();
        while event.is_none() || event.is_some() && event.unwrap() != sock_id {
            // イベントが来てない or 来てたとしても関係ないなら待機
            // cvarに通知が来るまでeventをunlockする
            // 通知が来たらlockをとってリターン
            event = cvar.wait(event).unwrap();
            dbg!("wake up");
        }
        *event = None;

        let mut table = self.sockets.write().unwrap();
        Ok(table
            .get_mut(&sock_id)
            .unwrap()
            .connected_connection_queue
            .pop_front()
            .context("no connected socket")?)
    }

    /// ターゲットに接続し，接続済みソケットのIDを返す
    pub fn connect(&self, addr: Ipv4Addr, port: u16) -> Result<SockID> {
        let mut rng = rand::thread_rng();
        let local_port = rng.gen_range(40000..60000);
        let mut socket = Socket::new(MY_IPADDR, addr, local_port, port, TcpStatus::SynSent);
        let iss = rng.gen_range(1..1 << 31);
        socket.send_param.initial_seq = iss; // ランダムにしないと，2回目以降SYNが返ってこなくなる（ACKだけ）
        socket.recv_param.window = WINDOW_SIZE;
        socket.send_tcp_packet(socket.send_param.initial_seq, 0, tcpflags::SYN, &[])?;
        socket.send_param.unacked_seq = socket.send_param.initial_seq;
        socket.send_param.next = socket.send_param.initial_seq + 1;
        let mut table = self.sockets.write().unwrap();
        let sock_id = socket.get_sock_id();
        table.insert(sock_id, socket);
        drop(table);

        let (lock, cvar) = &self.event_cond;
        let mut event = lock.lock().unwrap();
        while event.is_none() || event.is_some() && event.unwrap() != sock_id {
            event = cvar.wait(event).unwrap();
            dbg!("wake up");
        }
        *event = None;
        Ok(sock_id)
    }

    fn receive_handler(&self) -> Result<()> {
        dbg!("begin recv thread");
        let (mut sender, mut receiver) = transport::transport_channel(
            65535,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )
        .unwrap(); // TODO FIX
        let mut packet_iter = transport::tcp_packet_iter(&mut receiver);
        loop {
            // TODO: 最初にCtrl-C検出して受信スレッド終了処理したい
            let (packet, remote_addr) = packet_iter.next()?; // TODO handling
            let packet = TCPPacket::from(packet);
            let remote_addr = match remote_addr {
                IpAddr::V4(addr) => addr,
                _ => continue,
            };
            dbg!("incoming from", &remote_addr, packet.get_src());
            let mut table = self.sockets.write().unwrap();
            dbg!("write lock");
            // for k in table.keys() {
            //     dbg!(k);
            // }
            // dbg!(MY_IPADDR, remote_addr, packet.get_dest(), packet.get_src());
            let socket = match table.get_mut(&SockID(
                MY_IPADDR,
                remote_addr,
                packet.get_dest(),
                packet.get_src(),
            )) {
                Some(socket) => {
                    dbg!("connected socket", socket.get_sock_id());
                    socket // 接続済みソケット
                }
                None => match table.get_mut(&SockID(
                    MY_IPADDR,
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
            // checksum, ack検証
            let sock_id = socket.get_sock_id();
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
                        );
                        connection_socket.recv_param.next = packet.get_seq() + 1;
                        connection_socket.recv_param.initial_seq = packet.get_seq();
                        connection_socket.send_param.initial_seq = 443322; // TODO random
                        connection_socket
                            .send_tcp_packet(
                                connection_socket.send_param.initial_seq,
                                connection_socket.recv_param.next,
                                tcpflags::SYN | tcpflags::ACK,
                                &[],
                            )
                            .unwrap(); // TODO retry
                        connection_socket.send_param.next =
                            connection_socket.send_param.initial_seq + 1;
                        connection_socket.send_param.unacked_seq =
                            connection_socket.send_param.initial_seq;
                        connection_socket.listening_socket = Some(sock_id);
                        dbg!(sock_id);
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
                            if let Some(id) = socket.listening_socket {
                                let ls = table.get_mut(&id).unwrap();
                                ls.connected_connection_queue.push_back(sock_id);
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
                TcpStatus::SynSent => {
                    dbg!("synsend handler");
                    if packet.get_flag() & tcpflags::ACK > 0 {
                        if socket.send_param.unacked_seq <= packet.get_ack()
                            && packet.get_ack() <= socket.send_param.next
                        {
                            if packet.get_flag() & tcpflags::RST > 0 {
                                drop(socket); // tableの&mutを得るためdropする必要がある
                                table.remove(&sock_id);
                                continue;
                            }
                            if packet.get_flag() & tcpflags::SYN > 0 {
                                socket.recv_param.next = packet.get_seq() + 1;
                                socket.recv_param.initial_seq = packet.get_seq();
                                socket.send_param.unacked_seq = packet.get_ack();
                                if socket.send_param.unacked_seq > socket.send_param.initial_seq {
                                    socket
                                        .send_tcp_packet(
                                            socket.send_param.next,
                                            socket.recv_param.next,
                                            tcpflags::ACK,
                                            &[],
                                        )
                                        .unwrap(); // TODO
                                    socket.status = TcpStatus::Established;
                                    dbg!("status: SynSend → Established");
                                } else {
                                    // to SYNRCVD
                                }
                            }
                        } else {
                            dbg!("invalid ack number");
                        }
                    }
                }
                _ => unimplemented!(),
            }
        }
    }
}
