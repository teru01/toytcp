# !/bin/bash

# reference: https://techblog.ap-com.co.jp/entry/2019/06/28/100439

set -eux

sudo ip netns add host1
sudo ip netns add router
sudo ip netns add host2

sudo ip link add name host1-veth1 type veth peer name router-veth1
sudo ip link add name router-veth2 type veth peer name host2-veth1

sudo ip link set host1-veth1 netns host1
sudo ip link set router-veth1 netns router
sudo ip link set router-veth2 netns router
sudo ip link set host2-veth1 netns host2

sudo ip netns exec host1 ip addr add 10.0.0.1/24 dev host1-veth1
sudo ip netns exec router ip addr add 10.0.0.254/24 dev router-veth1
sudo ip netns exec router ip addr add 10.0.1.254/24 dev router-veth2
sudo ip netns exec host2 ip addr add 10.0.1.1/24 dev host2-veth1

sudo ip netns exec host1 ip link set host1-veth1 up
sudo ip netns exec router ip link set router-veth1 up
sudo ip netns exec router ip link set router-veth2 up
sudo ip netns exec host2 ip link set host2-veth1 up
sudo ip netns exec host1 ip link set lo up
sudo ip netns exec router ip link set lo up
sudo ip netns exec host2 ip link set lo up

sudo ip netns exec host1 ip route add 0.0.0.0/0 via 10.0.0.254
sudo ip netns exec host2 ip route add 0.0.0.0/0 via 10.0.1.254
sudo ip netns exec router sysctl -w net.ipv4.ip_forward=1

# drop RST
sudo ip netns exec host1 sudo iptables -A OUTPUT -p tcp --tcp-flags RST RST -j DROP
sudo ip netns exec host2 sudo iptables -A OUTPUT -p tcp --tcp-flags RST RST -j DROP

# turn off checksum offloading
sudo ip netns exec host2 sudo ethtool -K host2-veth1 tx off
sudo ip netns exec host1 sudo ethtool -K host1-veth1 tx off
