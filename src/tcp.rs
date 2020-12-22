use crate::packet::{tcpflags, TCPPacket};
use crate::socket::{SockID, Socket, TcpStatus};
use anyhow::{Context, Result};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::tcp::TcpPacket;
use pnet::packet::Packet;
use pnet::transport::{
    self, TransportChannelType, TransportProtocol, TransportReceiver, TransportSender,
};
use rand::Rng;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, SystemTime};
use std::{cmp, str};
const UNDETERMINED_IP_ADDR: std::net::Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const UNDETERMINED_PORT: u16 = 0;
const MAX_RETRANSMITTION: u8 = 3;
const RETRANSMITTION_TIMEOUT: u64 = 3;
const MSS: usize = 1460;

pub struct TCP {
    sockets: RwLock<HashMap<SockID, Socket>>,
    event_cond: (Mutex<Option<TCPEvent>>, Condvar),
}

#[derive(Debug, Clone, PartialEq)]
struct TCPEvent {
    sock_id: SockID,
    kind: TCPEventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TCPEventKind {
    ConnectionCompleted,
    Acked,
    DataArrived,
    ConnectionClosed,
}

impl TCPEvent {
    fn new(sock_id: SockID, kind: TCPEventKind) -> Self {
        Self { sock_id, kind }
    }
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
            // dbg!("timer loop");
            let mut table = self.sockets.write().unwrap();
            // dbg!("timer check start");
            for (_, socket) in table.iter_mut() {
                while let Some(mut item) = socket.retransmission_queue.pop_front() {
                    if socket.send_param.unacked_seq > item.packet.get_seq() {
                        // ackされてる
                        dbg!("successfully acked", item.packet.get_seq());
                        socket.send_param.window += item.packet.payload().len() as u16;
                        self.publish_event(socket.get_sock_id(), TCPEventKind::Acked); // イベントを受けた側がテーブルロックを取れるのは1ループ終わった後
                        if item.packet.get_flag() & tcpflags::FIN > 0
                            && socket.status == TcpStatus::LastAck
                        {
                            self.publish_event(
                                socket.get_sock_id(),
                                TCPEventKind::ConnectionClosed,
                            );
                        }
                        continue;
                    }
                    // タイムアウトを確認
                    if item.latest_transmission_time.elapsed().unwrap()
                        < Duration::from_secs(RETRANSMITTION_TIMEOUT)
                    {
                        // 取り出したエントリがタイムアウトしてないなら，キューの以降のエントリもタイムアウトしてない
                        // 先頭に戻す
                        socket.retransmission_queue.push_front(item);

                        break; //
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
                        dbg!(
                            "retransmit",
                            item.packet.get_seq() - socket.send_param.initial_seq
                        );
                        let sent_size = sender
                            .send_to(item.packet.clone(), IpAddr::V4(socket.remote_addr))
                            .context(format!("failed to retransmit"))
                            .unwrap();
                        item.transmission_count += 1;
                        item.latest_transmission_time = SystemTime::now();
                        socket.retransmission_queue.push_back(item);
                        break;
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
        // self.local_addr = local_addr;
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
        self.wait_event(sock_id, TCPEventKind::ConnectionCompleted);

        let mut table = self.sockets.write().unwrap();
        Ok(table
            .get_mut(&sock_id)
            .context(format!("no such socket: {:?}", sock_id))?
            .connected_connection_queue
            .pop_front()
            .context("no connected socket")?)
    }

    /// ターゲットに接続し，接続済みソケットのIDを返す
    pub fn connect(&self, addr: Ipv4Addr, port: u16) -> Result<SockID> {
        // self.local_addr = ;
        let mut rng = rand::thread_rng();
        let local_port = rng.gen_range(40000..60000); // TODO: 利用されてないか？
        let mut socket = Socket::new(
            get_source_addr_to(addr)?,
            addr,
            local_port,
            port,
            TcpStatus::SynSent,
        );
        let iss = rng.gen_range(1..1 << 31);
        socket.send_param.initial_seq = iss; // ランダムにしないと，2回目以降SYNが返ってこなくなる（ACKだけ）
        socket.send_tcp_packet(socket.send_param.initial_seq, 0, tcpflags::SYN, &[])?;
        socket.send_param.unacked_seq = socket.send_param.initial_seq;
        socket.send_param.next = socket.send_param.initial_seq + 1;
        let mut table = self.sockets.write().unwrap();
        let sock_id = socket.get_sock_id();
        table.insert(sock_id, socket);
        drop(table);

        self.wait_event(sock_id, TCPEventKind::ConnectionCompleted);
        Ok(sock_id)
    }

    /// セグメントが到着次第すぐにreturnする
    /// 到着イベント発生→ロック取得→読み出し
    pub fn receive(&self, sock_id: SockID, buffer: &mut [u8]) -> Result<usize> {
        let mut table = self.sockets.write().unwrap();
        let mut socket = table
            .get_mut(&sock_id)
            .context(format!("no such socket: {:?}", sock_id))?;
        let mut received_size = socket.recv_buffer.len() - socket.recv_param.window as usize;
        while received_size == 0 {
            // ペイロードを受信 or FINを受信でスキップ
            match socket.status {
                TcpStatus::CloseWait | TcpStatus::LastAck | TcpStatus::TimeWait => break,
                _ => {}
            }
            drop(table);
            dbg!("waiting incoming data");
            self.wait_event(sock_id, TCPEventKind::DataArrived);
            table = self.sockets.write().unwrap();
            socket = table
                .get_mut(&sock_id)
                .context(format!("no such socket: {:?}", sock_id))?;
            received_size = socket.recv_buffer.len() - socket.recv_param.window as usize;
            //ロスで歯抜けになってる時，windowはrecv_handlerで変化させてない，0になる
        }
        let received_size = socket.recv_buffer.len() - socket.recv_param.window as usize; // ロスの後の歯抜けに埋まった時，一気に2セグ分コピー
        let copy_size = cmp::min(buffer.len(), received_size);
        buffer[..copy_size].copy_from_slice(&socket.recv_buffer[..copy_size]);
        socket.recv_buffer.copy_within(copy_size.., 0);
        socket.recv_param.window += copy_size as u16;
        // socket.recv_param.tail -= copy_size as u32;
        dbg!(socket.recv_param.window, copy_size);
        Ok(copy_size)
    }

    /// バッファが空いてないとブロックする．受信側はバッファいっぱいにならないようにreadしておかないといけない
    ///
    pub fn send(&self, sock_id: SockID, buffer: &[u8]) -> Result<()> {
        // 送信バッファに書き込んだらreturn(Linux方式) or 送信できたらreturn
        // 送信中に受信ウィンドウが変化するが，それには対応できない（MSS < windowなら一定なので問題ない）
        // 非同期でACKを受けている（タイマースレッドで）なので，送信ウィンドウがとても大きい（スロースタートになってない）
        // 送信バッファなしでやる
        // 送信ウィンドウだけしかin flightにできない
        let mut cursor = 0;
        while cursor < buffer.len() {
            let mut table = self.sockets.write().unwrap();
            let mut socket = table
                .get_mut(&sock_id)
                .context(format!("no such socket: {:?}", sock_id))?;
            let mut send_size = cmp::min(
                MSS,
                cmp::min(socket.send_param.window as usize, buffer.len() - cursor),
            );
            while send_size == 0 {
                dbg!("recv buffer is full");
                // バッファがいっぱいなので待つ
                drop(table);
                self.wait_event(sock_id, TCPEventKind::Acked);
                table = self.sockets.write().unwrap();
                socket = table
                    .get_mut(&sock_id)
                    .context(format!("no such socket: {:?}", sock_id))?;
                // 送信サイズを再計算する
                send_size = cmp::min(
                    MSS,
                    cmp::min(socket.send_param.window as usize, buffer.len() - cursor),
                );
                dbg!("next while", send_size, socket.send_param.window);
            }
            socket.send_tcp_packet(
                socket.send_param.next,
                socket.recv_param.next,
                tcpflags::ACK,
                &buffer[cursor..cursor + send_size],
            )?;
            cursor += send_size;
            socket.send_param.next += send_size as u32;
            socket.send_param.window -= send_size as u16;
            // 送信中にACKが入ってこれるようにしている．
            // そうしないとsend_bufferが0になるまで必ず送り続けてブロックする
            drop(table);
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    pub fn close(&self, sock_id: SockID) -> Result<()> {
        let mut table = self.sockets.write().unwrap();
        let mut socket = table
            .get_mut(&sock_id)
            .context(format!("no such socket: {:?}", sock_id))?;
        socket.send_tcp_packet(
            socket.send_param.next,
            socket.recv_param.next,
            tcpflags::FIN | tcpflags::ACK,
            &[],
        )?;
        socket.send_param.next += 1;
        match socket.status {
            TcpStatus::Established => {
                socket.status = TcpStatus::FinWait1;
                drop(table);
                self.wait_event(sock_id, TCPEventKind::ConnectionClosed);
                let mut table = self.sockets.write().unwrap();
                table.remove(&sock_id);
                dbg!("closed & removed", sock_id);
            }
            TcpStatus::CloseWait => {
                socket.status = TcpStatus::LastAck;
                drop(table);
                self.wait_event(sock_id, TCPEventKind::ConnectionClosed);
                let mut table = self.sockets.write().unwrap();
                table.remove(&sock_id);
                dbg!("closed & removed", sock_id);
            }
            TcpStatus::Listen => {
                table.remove(&sock_id);
            }
            _ => return Ok(()),
        }
        Ok(())
    }

    /// 指定したsock_idでイベントを待機
    fn wait_event(&self, sock_id: SockID, kind: TCPEventKind) {
        let (lock, cvar) = &self.event_cond;
        let mut event = lock.lock().unwrap();
        loop {
            if let Some(ref e) = *event {
                if e.sock_id == sock_id && e.kind == kind {
                    break;
                }
            }
            event = cvar.wait(event).unwrap();
        }
        dbg!(&event);
        *event = None;
    }

    fn receive_handler(&self) -> Result<()> {
        dbg!("begin recv thread");
        let (mut sender, mut receiver) = transport::transport_channel(
            65535,
            TransportChannelType::Layer3(IpNextHeaderProtocols::Tcp), // IPv4
        )
        .unwrap(); // TODO FIX
        let mut packet_iter = transport::ipv4_packet_iter(&mut receiver);
        loop {
            // TODO: 最初にCtrl-C検出して受信スレッド終了処理したい
            let (packet, remote_addr) = packet_iter.next().unwrap(); // TODO handling
            dbg!("recvd");
            let local_addr = packet.get_destination();
            let tcp_packet = match TcpPacket::new(packet.payload()) {
                Some(p) => p,
                None => {
                    dbg!("none");
                    continue;
                }
            };
            let packet = TCPPacket::from(tcp_packet);
            let remote_addr = match remote_addr {
                IpAddr::V4(addr) => addr,
                _ => {
                    dbg!("none");
                    continue;
                }
            };
            dbg!("incoming from", &remote_addr, packet.get_src());
            let mut table = self.sockets.write().unwrap();
            dbg!("after table lock");
            let socket = match table.get_mut(&SockID(
                local_addr,
                remote_addr,
                packet.get_dest(),
                packet.get_src(),
            )) {
                Some(socket) => {
                    // dbg!("connected socket", socket.get_sock_id());
                    socket // 接続済みソケット
                }
                None => match table.get_mut(&SockID(
                    local_addr,
                    UNDETERMINED_IP_ADDR,
                    packet.get_dest(),
                    UNDETERMINED_PORT,
                )) {
                    Some(socket) => {
                        // dbg!("listening socket", socket.get_sock_id());
                        socket
                    } // リスニングソケット
                    None => {
                        for k in table.keys() {
                            dbg!(k);
                        }
                        dbg!(local_addr, remote_addr, packet.get_dest(), packet.get_src());
                        dbg!("SOCKET NOT FOUND");
                        continue;
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
                    // seqを見て受け入れ可能テスト

                    // check RST
                    // check SYN

                    // ACK
                    if packet.get_flag() & tcpflags::ACK > 0 {
                        if socket.send_param.unacked_seq <= packet.get_ack()
                            && packet.get_ack() <= socket.send_param.next
                        {
                            // SYN|ACKが入っている
                            if let None = socket.retransmission_queue.pop_front() {
                                dbg!("initial SYN|ACK NOT FOUND");
                            }
                            socket.recv_param.next = packet.get_seq();
                            socket.send_param.unacked_seq = packet.get_ack();
                            socket.status = TcpStatus::Established;
                            if let Some(id) = socket.listening_socket {
                                let ls = table.get_mut(&id).unwrap();
                                ls.connected_connection_queue.push_back(sock_id);
                                self.publish_event(
                                    ls.get_sock_id(),
                                    TCPEventKind::ConnectionCompleted,
                                );
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
                    // self.synsent_handler(&packet, table, socket)?;
                    dbg!("synsend handler");
                    if packet.get_flag() & tcpflags::ACK > 0 {
                        if socket.send_param.unacked_seq <= packet.get_ack()
                            && packet.get_ack() <= socket.send_param.next
                        {
                            if packet.get_flag() & tcpflags::RST > 0 {
                                drop(socket);
                                table.remove(&sock_id);
                                // return Ok(());
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
                                    if let None = socket.retransmission_queue.pop_front() {
                                        // 最初のSYNが入っている
                                        dbg!("initial SYN not fount!");
                                    }
                                    dbg!("status: SynSend → Established");
                                    self.publish_event(sock_id, TCPEventKind::ConnectionCompleted);
                                } else {
                                    // to SYNRCVD
                                }
                            }
                        } else {
                            dbg!("invalid ack number");
                        }
                    }
                }
                TcpStatus::Established => {
                    self.established_handler(&packet, socket)?;
                }
                TcpStatus::CloseWait | TcpStatus::LastAck => {
                    dbg!("CloseWait | LastAck handler");
                    socket.send_param.unacked_seq = packet.get_ack();
                }
                TcpStatus::FinWait1 => {
                    dbg!("FinWait1 handler");
                    // TODO: まだデータは受け取らないといけない

                    if packet.get_flag() & tcpflags::ACK == 0 {
                        // ACKが立っていないパケットは破棄
                        return Ok(());
                    }
                    socket.send_param.unacked_seq = packet.get_ack();
                    socket.recv_param.next = packet.get_seq() + packet.payload().len() as u32;
                    if socket.send_param.next == socket.send_param.unacked_seq {
                        socket.status = TcpStatus::FinWait2;
                        if packet.get_flag() & tcpflags::FIN > 0 {
                            socket.recv_param.next += 1;
                            socket.send_tcp_packet(
                                socket.send_param.next,
                                socket.recv_param.next,
                                tcpflags::ACK,
                                &[],
                            )?;
                            self.publish_event(sock_id, TCPEventKind::ConnectionClosed);
                        }
                    }
                }
                TcpStatus::FinWait2 => {
                    dbg!("FinWait2 handler");

                    // TODO: まだデータは受け取らないといけない
                    if packet.get_flag() & tcpflags::ACK == 0 {
                        // ACKが立っていないパケットは破棄
                        return Ok(());
                    }
                    socket.send_param.unacked_seq = packet.get_ack();
                    socket.recv_param.next = packet.get_seq() + packet.payload().len() as u32;
                    if packet.get_flag() & tcpflags::FIN > 0 {
                        socket.recv_param.next += 1;
                        socket.send_tcp_packet(
                            socket.send_param.next,
                            socket.recv_param.next,
                            tcpflags::ACK,
                            &[],
                        )?;
                        self.publish_event(sock_id, TCPEventKind::ConnectionClosed);
                    }
                }
                _ => unimplemented!(),
            }
            drop(table);
        }
    }

    fn established_handler(&self, packet: &TCPPacket, socket: &mut Socket) -> Result<()> {
        dbg!("established handler");
        // !RSTならCLOSE
        // !SYNチェック

        // 受け入れ
        dbg!(
            "before accept",
            socket.send_param.unacked_seq,
            packet.get_ack(),
            socket.send_param.next
        );
        if socket.send_param.unacked_seq < packet.get_ack()
            && packet.get_ack() <= socket.send_param.next
        {
            socket.send_param.unacked_seq = packet.get_ack();
            dbg!("ack accept", socket.send_param.unacked_seq);
            while let Some(item) = socket.retransmission_queue.pop_front() {
                if socket.send_param.unacked_seq > item.packet.get_seq() {
                    // ackされてる
                    dbg!("successfully acked", item.packet.get_seq());
                    socket.send_param.window += item.packet.payload().len() as u16;
                    self.publish_event(socket.get_sock_id(), TCPEventKind::Acked); // イベントを受けた側がテーブルロックを取れるのは1ループ終わった後
                    if item.packet.get_flag() & tcpflags::FIN > 0
                        && socket.status == TcpStatus::LastAck
                    {
                        self.publish_event(socket.get_sock_id(), TCPEventKind::ConnectionClosed);
                    }
                } else {
                    // ackされてない．戻す．
                    socket.retransmission_queue.push_front(item);
                    break;
                }
            }
        // このタイミングで
        // !ウィンドウ操作
        } else if socket.send_param.next < packet.get_ack() {
            // おかしなackは破棄
            dbg!(
                "invalid ack num",
                packet.get_ack(),
                socket.send_param.unacked_seq
            );
            // !ACKを送る
            return Ok(());
        }
        // 重複のACKは無視
        // socket.send_param.send
        if packet.get_flag() & tcpflags::ACK == 0 {
            // ACKが立っていないパケットは破棄
            return Ok(());
        }

        let payload_len = packet.payload().len();
        if payload_len > 0 {
            // バッファにおける読み込みのヘッド位置．
            dbg!(
                // offset,
                socket.recv_buffer.len(),
                socket.recv_param.window,
                packet.get_seq(),
                socket.recv_param.next
            );
            let offset = socket.recv_buffer.len() - socket.recv_param.window as usize
                + (packet.get_seq() - socket.recv_param.next) as usize;
            let copied_size = cmp::min(payload_len, socket.recv_buffer.len() - offset);
            socket.recv_buffer[offset..offset + copied_size]
                .copy_from_slice(&packet.payload()[..copied_size]);
            socket.recv_param.tail = cmp::max(
                socket.recv_param.tail,
                packet.get_seq() + copied_size as u32,
            ); // ロス再送で穴埋めされる時のためにmaxをとる

            // TODO 受信バッファ溢れの時どうする？以下は溢れない前提のコード
            if packet.get_seq() == socket.recv_param.next {
                // ロス・順序入れ替わり無しの場合のみrecv_param.nextを進められる
                socket.recv_param.next = socket.recv_param.tail;
                socket.recv_param.window -= (socket.recv_param.tail - packet.get_seq()) as u16;
            }
            // ロスの時はこれではダメ，

            // TODO ウィンドウサイズも送る
            socket.send_tcp_packet(
                socket.send_param.next,
                socket.recv_param.next,
                tcpflags::ACK,
                &[],
            )?;
            self.publish_event(socket.get_sock_id(), TCPEventKind::DataArrived);
            dbg!(packet.get_seq());
        }
        if packet.get_flag() & tcpflags::FIN > 0 {
            socket.recv_param.next = packet.get_seq() + 1;
            socket.send_tcp_packet(
                socket.send_param.next,
                socket.recv_param.next,
                tcpflags::ACK,
                &[],
            )?;
            // ここでFINを送って直接LAST-ACKに遷移するのもあり
            // ソケットの送信ウィンドウにデータが残ってしまうので？ 明示的にFINさせよう
            socket.status = TcpStatus::CloseWait;
            self.publish_event(socket.get_sock_id(), TCPEventKind::DataArrived);
        }
        Ok(())
    }

    fn publish_event(&self, sock_id: SockID, kind: TCPEventKind) {
        let (lock, cvar) = &self.event_cond;
        let mut e = lock.lock().unwrap();
        *e = Some(TCPEvent::new(sock_id, kind));
        cvar.notify_all();
    }
}

fn get_source_addr_to(addr: Ipv4Addr) -> Result<Ipv4Addr> {
    use std::process::Command;
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "ip route get {}| awk 'match($0, /src (([0-9]|\\.)*) /, a){{print a[1]}}'",
            addr
        ))
        .output()?;
    let ip = str::from_utf8(&output.stdout)?.trim();
    dbg!("source addr", ip);
    ip.parse().context("failed to parse source ip")
}
