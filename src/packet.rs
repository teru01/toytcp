use pnet::packet::Packet;

const TCP_HEADER_SIZE: usize = 20;
const TCP_DATA_OFFSET: u8 = 5;
pub struct TCPPacket {
    buffer: Vec<u8>,
}

impl TCPPacket {
    pub fn new() -> Self {
        Self {
            buffer: vec![0; TCP_HEADER_SIZE],
        }
    }

    pub fn get_src(&self) -> u16 {
        // use std::convert::TryInto;
        // u16::from_be_bytes(self.buffer[0..2].try_into().unwrap())
        u16::from_be_bytes([self.buffer[0], self.buffer[1]])
    }

    pub fn get_dest(&self) -> u16 {
        u16::from_be_bytes([self.buffer[2], self.buffer[3]])
    }

    pub fn get_seq(&self) -> u32 {
        u32::from_be_bytes([
            self.buffer[4],
            self.buffer[5],
            self.buffer[6],
            self.buffer[7],
        ])
    }

    pub fn get_ack(&self) -> u32 {
        u32::from_be_bytes([
            self.buffer[8],
            self.buffer[9],
            self.buffer[10],
            self.buffer[11],
        ])
    }

    pub fn get_flag(&self) -> u8 {
        self.buffer[13]
    }

    pub fn get_window_size(&self) -> u16 {
        u16::from_be_bytes([self.buffer[14], self.buffer[15]])
    }

    pub fn get_checksum(&self) -> u16 {
        u16::from_be_bytes([self.buffer[16], self.buffer[17]])
    }

    pub fn set_src(&mut self, port: u16) {
        self.buffer[0..2].copy_from_slice(&port.to_be_bytes())
    }

    pub fn set_dest(&mut self, port: u16) {
        self.buffer[2..4].copy_from_slice(&port.to_be_bytes())
    }

    pub fn set_seq(&mut self, num: u32) {
        self.buffer[4..8].copy_from_slice(&num.to_be_bytes())
    }

    pub fn set_ack(&mut self, num: u32) {
        self.buffer[8..12].copy_from_slice(&num.to_be_bytes())
    }

    pub fn set_data_offset(&mut self, offset: u8) {
        self.buffer[13] = offset << 4; // TODO bit order
    }

    pub fn set_flag(&mut self, flag: u8) {
        self.buffer[14] = flag;
    }

    pub fn set_window_size(&mut self, window: u16) {
        self.buffer[14..16].copy_from_slice(&window.to_be_bytes())
    }

    pub fn set_checksum(&mut self, checksum: u16) {
        self.buffer[16..18].copy_from_slice(&checksum.to_be_bytes())
    }

    pub fn set_payload(&mut self, payload: &[u8]) {
        self.buffer[TCP_HEADER_SIZE..].copy_from_slice(payload)
    }
}

impl Packet for TCPPacket {
    fn packet(&self) -> &[u8] {
        &self.buffer
    }

    fn payload(&self) -> &[u8] {
        &self.buffer[TCP_HEADER_SIZE..]
    }
}
