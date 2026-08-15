use std::time::Duration;

use bytes::{BufMut, BytesMut};
use ghost_bnet::{BnetConfig, BnetEvent, spawn_bnet};
use ghost_protocol::bncs::{ids, BncsCodec};
use ghost_protocol::frame::Frame;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

#[tokio::test]
async fn bnet_client_completes_handshake_to_login() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();

        // 1. Read protocol selector byte 0x01
        let mut proto_byte = [0u8; 1];
        stream.read_exact(&mut proto_byte).await.unwrap();
        assert_eq!(proto_byte[0], 0x01);

        let (read_half, write_half) = stream.into_split();
        let mut framed_read = tokio_util::codec::FramedRead::new(read_half, BncsCodec::default());
        let mut framed_write = tokio_util::codec::FramedWrite::new(write_half, BncsCodec::default());

        use futures_util::{SinkExt, StreamExt};

        // 2. Expect SID_AUTH_INFO -> respond with SID_AUTH_INFO
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_AUTH_INFO);

        let mut p = BytesMut::new();
        p.put_u32_le(0); // logon type (NLS)
        p.put_u32_le(0x1234_5678); // server token
        p.put_slice(&[0; 4]);
        p.put_u32_le(0); // mpq low
        p.put_u32_le(0); // mpq high
        p.put_slice(b"IX86ver1.mpq\0");
        p.put_slice(b"A=47 B=1\0");
        let resp = Frame::new(ids::SID_AUTH_INFO, p.freeze()).encode_with(0xFF).unwrap();
        framed_write.send(resp).await.unwrap();

        // 3. Expect SID_AUTH_CHECK -> respond with SID_AUTH_CHECK (good)
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_AUTH_CHECK);

        let mut p = BytesMut::new();
        p.put_u32_le(0); // key_state = KR_GOOD
        p.put_slice(b"passed\0");
        let resp = Frame::new(ids::SID_AUTH_CHECK, p.freeze()).encode_with(0xFF).unwrap();
        framed_write.send(resp).await.unwrap();

        // 4. Expect SID_AUTH_ACCOUNTLOGON -> respond with SID_AUTH_ACCOUNTLOGON (salt + server key)
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_AUTH_ACCOUNTLOGON);

        let mut p = BytesMut::new();
        p.put_u32_le(0); // status = 0
        p.put_slice(&[0xAA; 32]); // salt
        p.put_slice(&[0xBB; 32]); // server public key
        let resp = Frame::new(ids::SID_AUTH_ACCOUNTLOGON, p.freeze()).encode_with(0xFF).unwrap();
        framed_write.send(resp).await.unwrap();

        // 5. Expect SID_AUTH_ACCOUNTLOGONPROOF -> respond with SID_AUTH_ACCOUNTLOGONPROOF (success)
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_AUTH_ACCOUNTLOGONPROOF);

        let mut p = BytesMut::new();
        p.put_u32_le(0); // status = 0
        let resp = Frame::new(ids::SID_AUTH_ACCOUNTLOGONPROOF, p.freeze()).encode_with(0xFF).unwrap();
        framed_write.send(resp).await.unwrap();

        // 6. Expect SID_ENTERCHAT -> respond with SID_ENTERCHAT
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_ENTERCHAT);
    });

    let (events_tx, mut events_rx) = mpsc::channel(32);
    let cfg = BnetConfig {
        server: addr.ip().to_string(),
        port: addr.port(),
        username: "testbot".into(),
        password: "secretpassword".into(),
        cdkey_roc: "FFFFFFFFFFFFFFFFFFFFFFFFFF".into(),
        cdkey_tft: "FFFFFFFFFFFFFFFFFFFFFFFFFF".into(),
        first_channel: "The Abyss".into(),
        root_admins: vec!["slash".into()],
        command_trigger: '!',
        war3_version: 26,
        exe_version: [1, 0, 26, 1],
        exe_version_hash: [0, 0, 0, 0],
        reconnect_delay: Duration::from_secs(1),
    };

    let (_handle, _join) = spawn_bnet(cfg, events_tx);

    let ev1 = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
        .await
        .expect("connected event")
        .expect("event");
    assert_eq!(ev1, BnetEvent::Connected);

    let ev2 = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
        .await
        .expect("logged in event")
        .expect("event");
    assert_eq!(ev2, BnetEvent::LoggedIn);

    server_task.await.unwrap();
}
