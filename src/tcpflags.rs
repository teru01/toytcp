pub const CWR: u8 = 1 << 7;
pub const ECE: u8 = 1 << 6;
pub const URG: u8 = 1 << 5;
pub const ACK: u8 = 1 << 4;
pub const PSH: u8 = 1 << 3;
pub const RST: u8 = 1 << 2;
pub const SYN: u8 = 1 << 1;
pub const FIN: u8 = 1;

/// TCP flagを文字列に変換する
pub fn flag_to_string(flag: u8) -> String {
    let mut flag_str = String::new();
    if flag & SYN > 0 {
        flag_str += "SYN ";
    }
    if flag & ACK > 0 {
        flag_str += "ACK ";
    }
    if flag & FIN > 0 {
        flag_str += "FIN ";
    }
    if flag & RST > 0 {
        flag_str += "RST ";
    }
    if flag & CWR > 0 {
        flag_str += "CWR ";
    }
    if flag & ECE > 0 {
        flag_str += "ECE ";
    }
    if flag & PSH > 0 {
        flag_str += "PSH ";
    }
    if flag & URG > 0 {
        flag_str += "URG ";
    }
    flag_str
}
