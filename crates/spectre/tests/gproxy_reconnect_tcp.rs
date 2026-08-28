use bytes::{BufMut, Bytes, BytesMut};
use spectre_protocol::gps;
use spectre_protocol::w3gs::ids as w3gs_ids;
use spectre::Config;
use spectre::Supervisor;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn make_reqjoin(name: &str) -> Bytes {
    let mut b = BytesMut::new();
    b.put_u32_le(1);
    b.put_u32_le(0);
    b.put_u8(0);
    b.put_u16_le(6112);
    b.put_u32_le(0);
    b.put_slice(name.as_bytes());
    b.put_u8(0);
    b.put_slice(&[0; 6]);
    b.put_slice(&[127, 0, 0, 1]);
    b.freeze()
}

#[tokio::test]
async fn gproxy_reconnect_listener_tcp_e2e() {
    let l1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host_port = l1.local_addr().unwrap().port();
    drop(l1);

    let l2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let reconnect_port = l2.local_addr().unwrap().port();
    drop(l2);

    let l3 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let spectator_port = l3.local_addr().unwrap().port();
    drop(l3);

    let mut cfg = Config::from_toml("").unwrap();
    cfg.bot.bind_address = "127.0.0.1".into();
    cfg.bot.host_port = host_port;
    cfg.bot.gproxy_reconnect_port = reconnect_port;
    cfg.spectator.port = spectator_port;
    cfg.bnet.server = "127.0.0.1".into();
    cfg.db_path = format!("reconnect_test_{host_port}.db").into();

    let sup_handle = tokio::spawn(async move {
        let _ = Supervisor::run(cfg, vec!["ReconnectMatch".into()], None, false).await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // 1. Connect player to game host_port
    let mut player_sock = TcpStream::connect(format!("127.0.0.1:{host_port}"))
        .await
        .expect("must connect to host_port");

    // 2. Send REQ_JOIN
    let reqjoin = make_reqjoin("ReconnectTester");
    let mut frame = BytesMut::new();
    frame.put_u8(0xF7);
    frame.put_u8(w3gs_ids::REQ_JOIN);
    frame.put_u16_le(4 + reqjoin.len() as u16);
    frame.put_slice(&reqjoin);
    player_sock.write_all(&frame).await.unwrap();
    player_sock.flush().await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. Send GPS_INIT to register as GProxy player
    let mut init_frame = BytesMut::new();
    init_frame.put_u8(0xF8);
    init_frame.put_u8(gps::ids::INIT);
    init_frame.put_u16_le(8);
    init_frame.put_u32_le(1);
    player_sock.write_all(&init_frame).await.unwrap();
    player_sock.flush().await.unwrap();

    // 4. Read GPS_INIT response from bot
    let mut init_buf = vec![0u8; 1024];
    let mut pid = 0;
    let mut reconnect_key = 0;
    let mut got_init = false;

    for _ in 0..10 {
        if let Ok(Ok(n)) =
            tokio::time::timeout(Duration::from_millis(100), player_sock.read(&mut init_buf)).await
        {
            let mut offset = 0;
            while offset + 14 <= n {
                if init_buf[offset] == 0xF8 && init_buf[offset + 1] == gps::ids::INIT {
                    pid = init_buf[offset + 8];
                    reconnect_key = u32::from_le_bytes([
                        init_buf[offset + 9],
                        init_buf[offset + 10],
                        init_buf[offset + 11],
                        init_buf[offset + 12],
                    ]);
                    got_init = true;
                    break;
                }
                offset += 1;
            }
            if got_init {
                break;
            }
        }
    }
    assert!(got_init, "must receive GPS_INIT from bot");
    assert!(pid > 0);
    assert!(reconnect_key != 0);

    // 5. Drop the player socket (simulate network drop)
    drop(player_sock);
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 6. Connect to reconnect_port with valid key
    let mut reconn_sock = TcpStream::connect(format!("127.0.0.1:{reconnect_port}"))
        .await
        .expect("must connect to reconnect_port");

    let mut reconn_pkt = BytesMut::new();
    reconn_pkt.put_u8(0xF8);
    reconn_pkt.put_u8(gps::ids::RECONNECT);
    reconn_pkt.put_u16_le(13);
    reconn_pkt.put_u8(pid);
    reconn_pkt.put_u32_le(reconnect_key);
    reconn_pkt.put_u32_le(0); // last_packet: 0
    reconn_sock.write_all(&reconn_pkt).await.unwrap();
    reconn_sock.flush().await.unwrap();

    // 7. Verify we get GPS_RECONNECT / GPS_ACK response on the new socket
    let mut reply_buf = vec![0u8; 1024];
    let mut reconnected = false;
    for _ in 0..10 {
        if let Ok(Ok(n)) =
            tokio::time::timeout(Duration::from_millis(100), reconn_sock.read(&mut reply_buf)).await
            && n >= 4
            && reply_buf[0] == 0xF8
            && (reply_buf[1] == gps::ids::RECONNECT || reply_buf[1] == gps::ids::ACK)
        {
            reconnected = true;
            break;
        }
    }
    assert!(
        reconnected,
        "must receive GPS_RECONNECT/ACK on reconnect socket"
    );

    // 8. Test invalid key on a separate connection -> rejected
    let mut bad_sock = TcpStream::connect(format!("127.0.0.1:{reconnect_port}"))
        .await
        .expect("must connect to reconnect_port for bad test");

    let mut bad_pkt = BytesMut::new();
    bad_pkt.put_u8(0xF8);
    bad_pkt.put_u8(gps::ids::RECONNECT);
    bad_pkt.put_u16_le(13);
    bad_pkt.put_u8(pid);
    bad_pkt.put_u32_le(0xDEAD_BEEF); // wrong key
    bad_pkt.put_u32_le(0);
    bad_sock.write_all(&bad_pkt).await.unwrap();
    bad_sock.flush().await.unwrap();

    let mut bad_buf = vec![0u8; 1024];
    let mut got_reject = false;
    if let Ok(Ok(n)) =
        tokio::time::timeout(Duration::from_millis(300), bad_sock.read(&mut bad_buf)).await
    {
        if (n >= 4 && bad_buf[0] == 0xF8 && bad_buf[1] == gps::ids::REJECT) || n == 0 {
            got_reject = true;
        }
    } else {
        got_reject = true;
    }
    assert!(got_reject, "bad reconnect must be rejected or closed");

    sup_handle.abort();
    let _ = std::fs::remove_file(format!("reconnect_test_{host_port}.db"));
}
