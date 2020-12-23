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
$ cargo build --examples
```

## echo-server & client

server
```
$ sudo ip netns exec host1 sudo ./target/debug/examples/echoserver 10.0.0.1 30000
```

client
```
$ sudo ip netns exec host2 sudo ./target/debug/examples/echoclient 10.0.0.1 30000
```

## file upload

server
```
$ sudo ip netns exec host1 sudo ./target/debug/examples/fileserver 10.0.0.1 30000 <save file name>
```

client
```
$ sudo ip netns exec host2 sudo ./target/debug/examples/fileclient 10.0.0.1 30000 <send file name>
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
