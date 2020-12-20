pub mod packet;
pub mod socket;
pub mod tcp;

use std::net::Ipv4Addr;
pub const MY_IPADDR: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);
