# toytcp

toy tcp implementation

# setup

```
$ cd toytcp
$ chmod +x setup.sh
$ ./setup.sh
```

次のような構成になる
```
[host1-veth1]--router--[host2-veth1]
```

カーネルが持つプロトコルスタックとの競合を避けるため，RSTフラグを持つすべてのパケットはiptablesにより破棄される設定になっている．


# build & run 

```
$ cargo build
```

## echo-server & client

server
```
sudo ip netns exec host1 sudo ./target/debug/toytcp server
```

client
```
sudo ip netns exec host1 sudo ./target/debug/toytcp client
```

## file upload

server
```
sudo ip netns exec host1 sudo ./target/debug/toytcp files
```

client
```
sudo ip netns exec host1 sudo ./target/debug/toytcp filec
```

## simulate packet loss

### 外向きパケットの0.1%を破棄する

```
sudo ip netns exec host2 tc qdisc change dev host2-veth1 root netem loss 0.1%
```

この状態でファイル送信とかするとロスした時に再送される（はず）．止まるんじゃねぇぞ・・・

### パケロス設定の削除

```
sudo ip netns exec host2 tc qdisc del dev host2-veth1 root
```
