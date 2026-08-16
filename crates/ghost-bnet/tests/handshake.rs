use std::time::Duration;

use bytes::{BufMut, BytesMut};
use ghost_bnet::{BnetConfig, BnetEvent, spawn_bnet};
use ghost_protocol::bncs::{BncsCodec, ids};
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
        let mut framed_write =
            tokio_util::codec::FramedWrite::new(write_half, BncsCodec::default());

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
        let resp = Frame::new(ids::SID_AUTH_INFO, p.freeze())
            .encode_with(0xFF)
            .unwrap();
        framed_write.send(resp).await.unwrap();

        // 3. Expect SID_AUTH_CHECK -> respond with SID_AUTH_CHECK (good)
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_AUTH_CHECK);

        let mut p = BytesMut::new();
        p.put_u32_le(0); // key_state = KR_GOOD
        p.put_slice(b"passed\0");
        let resp = Frame::new(ids::SID_AUTH_CHECK, p.freeze())
            .encode_with(0xFF)
            .unwrap();
        framed_write.send(resp).await.unwrap();

        // 4. Expect SID_AUTH_ACCOUNTLOGON -> respond with SID_AUTH_ACCOUNTLOGON (status = 0)
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_AUTH_ACCOUNTLOGON);

        let mut p = BytesMut::new();
        p.put_u32_le(0); // status = 0 (Success)
        p.put_slice(&[0u8; 32]); // salt
        p.put_slice(&[0u8; 32]); // server public key
        let resp = Frame::new(ids::SID_AUTH_ACCOUNTLOGON, p.freeze())
            .encode_with(0xFF)
            .unwrap();
        framed_write.send(resp).await.unwrap();

        // 4b. Expect SID_AUTH_ACCOUNTLOGONPROOF -> respond with SID_AUTH_ACCOUNTLOGONPROOF (status = 0)
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_AUTH_ACCOUNTLOGONPROOF);

        let mut p = BytesMut::new();
        p.put_u32_le(0); // status = 0 (Success)
        p.put_slice(&[0u8; 20]); // server password proof
        p.put_slice(b"\0"); // message
        let resp = Frame::new(ids::SID_AUTH_ACCOUNTLOGONPROOF, p.freeze())
            .encode_with(0xFF)
            .unwrap();
        framed_write.send(resp).await.unwrap();

        // 5. Expect SID_NETGAMEPORT
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_NETGAMEPORT);

        // 6. Expect SID_ENTERCHAT -> respond with SID_ENTERCHAT
        let f = framed_read.next().await.unwrap().unwrap();
        assert_eq!(f.id, ids::SID_ENTERCHAT);
        let resp = Frame::new(
            ids::SID_ENTERCHAT,
            bytes::Bytes::from_static(b"Unique\0Stat\0Account\0"),
        )
        .encode_with(0xFF)
        .unwrap();
        framed_write.send(resp).await.unwrap();
    });

    let (events_tx, mut events_rx) = mpsc::channel(32);
    let cfg = BnetConfig {
        server: addr.ip().to_string(),
        server_alias: "iCCup".into(),
        pvpgn_realm_name: "PvPGN Realm".into(),
        port: addr.port(),
        host_port: 6112,
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
        password_hash_type: "pvpgn".into(),
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

#[tokio::test]
async fn test_p2_6_bnet_client_handles_clan_friends_warden_checkad() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();

        // 1. Read protocol selector byte 0x01
        let mut proto_byte = [0u8; 1];
        stream.read_exact(&mut proto_byte).await.unwrap();

        let (read_half, write_half) = stream.into_split();
        let mut framed_read = tokio_util::codec::FramedRead::new(read_half, BncsCodec::default());
        let mut framed_write =
            tokio_util::codec::FramedWrite::new(write_half, BncsCodec::default());

        use futures_util::{SinkExt, StreamExt};

        // 2. AUTH_INFO
        let _ = framed_read.next().await.unwrap().unwrap();
        let mut p = BytesMut::new();
        p.put_u32_le(0);
        p.put_u32_le(0x1234_5678);
        p.put_slice(&[0; 4]);
        p.put_u32_le(0);
        p.put_u32_le(0);
        p.put_slice(b"IX86ver1.mpq\0");
        p.put_slice(b"A=47 B=1\0");
        framed_write.send(Frame::new(ids::SID_AUTH_INFO, p.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // 3. AUTH_CHECK
        let _ = framed_read.next().await.unwrap().unwrap();
        let mut p = BytesMut::new();
        p.put_u32_le(0);
        p.put_slice(b"passed\0");
        framed_write.send(Frame::new(ids::SID_AUTH_CHECK, p.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // 4. AUTH_ACCOUNTLOGON
        let _ = framed_read.next().await.unwrap().unwrap();
        let mut p = BytesMut::new();
        p.put_u32_le(0);
        p.put_slice(&[0u8; 32]);
        p.put_slice(&[0u8; 32]);
        framed_write.send(Frame::new(ids::SID_AUTH_ACCOUNTLOGON, p.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // 4b. AUTH_ACCOUNTLOGONPROOF
        let _ = framed_read.next().await.unwrap().unwrap();
        let mut p = BytesMut::new();
        p.put_u32_le(0);
        p.put_slice(&[0u8; 20]);
        p.put_slice(b"\0");
        framed_write.send(Frame::new(ids::SID_AUTH_ACCOUNTLOGONPROOF, p.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // Drain NETGAMEPORT, ENTERCHAT, FRIENDSLIST, CLANMEMBERLIST
        let _ = framed_read.next().await.unwrap().unwrap(); // NETGAMEPORT
        let _ = framed_read.next().await.unwrap().unwrap(); // ENTERCHAT
        let _ = framed_read.next().await.unwrap().unwrap(); // FRIENDSLIST
        let _ = framed_read.next().await.unwrap().unwrap(); // CLANMEMBERLIST

        // Send ENTERCHAT reply
        framed_write.send(Frame::new(ids::SID_ENTERCHAT, bytes::Bytes::from_static(b"Unique\0Stat\0Account\0")).encode_with(0xFF).unwrap()).await.unwrap();

        // Send SID_FRIENDSLIST response
        let mut fl_payload = BytesMut::new();
        fl_payload.put_u8(1); // total = 1
        fl_payload.put_slice(b"Friend1\0");
        fl_payload.put_u8(1); // status
        fl_payload.put_u8(2); // area
        fl_payload.put_slice(&[0; 4]);
        fl_payload.put_slice(b"Channel\0");
        framed_write.send(Frame::new(ids::SID_FRIENDSLIST, fl_payload.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // Send SID_CLANMEMBERLIST response
        let mut cl_payload = BytesMut::new();
        cl_payload.put_slice(&[0; 4]);
        cl_payload.put_u8(1); // total = 1
        cl_payload.put_slice(b"ClanMate\0");
        cl_payload.put_u8(2); // rank (Grunt)
        cl_payload.put_u8(1); // status (Online)
        cl_payload.put_slice(b"Location\0");
        framed_write.send(Frame::new(ids::SID_CLANMEMBERLIST, cl_payload.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // Send SID_CLANCREATIONINVITATION
        let mut invite_payload = BytesMut::new();
        invite_payload.put_slice(&[0; 4]); // cookie
        invite_payload.put_slice(b"TAG1"); // tag
        invite_payload.put_slice(b"EpicClan\0");
        invite_payload.put_slice(b"ChiefBob\0");
        framed_write.send(Frame::new(ids::SID_CLANCREATIONINVITATION, invite_payload.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // Expect SID_CLANCREATIONINVITATION response from client after !accept
        let mut f_accept = framed_read.next().await.unwrap().unwrap();
        while f_accept.id == ids::SID_NULL {
            f_accept = framed_read.next().await.unwrap().unwrap();
        }
        assert_eq!(f_accept.id, ids::SID_CLANCREATIONINVITATION);
        assert_eq!(&f_accept.payload[4..8], b"TAG1");
        assert_eq!(f_accept.payload[f_accept.payload.len() - 1], 0x06); // accepted

        // Send SID_CHECKAD
        framed_write.send(Frame::new(ids::SID_CHECKAD, bytes::Bytes::new()).encode_with(0xFF).unwrap()).await.unwrap();

        // Expect SID_CHECKAD response from client
        let mut f_checkad = framed_read.next().await.unwrap().unwrap();
        while f_checkad.id == ids::SID_NULL {
            f_checkad = framed_read.next().await.unwrap().unwrap();
        }
        assert_eq!(f_checkad.id, ids::SID_CHECKAD);

        // Send SID_WARDEN
        framed_write.send(Frame::new(ids::SID_WARDEN, bytes::Bytes::from_static(b"warden_check")).encode_with(0xFF).unwrap()).await.unwrap();

        // Send SID_CLANCHANGERANK response (status 0 = success)
        let mut rank_payload = BytesMut::new();
        rank_payload.put_slice(&[0; 4]); // cookie
        rank_payload.put_u8(0); // status
        framed_write.send(Frame::new(ids::SID_CLANCHANGERANK, rank_payload.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // Send SID_CLANREMOVEMEMBER response (status 0 = success)
        let mut rem_payload = BytesMut::new();
        rem_payload.put_slice(&[0; 4]);
        rem_payload.put_u8(0);
        framed_write.send(Frame::new(ids::SID_CLANREMOVEMEMBER, rem_payload.freeze()).encode_with(0xFF).unwrap()).await.unwrap();

        // Send SID_CLANSETMOTD response (status 0 = success)
        let mut motd_payload = BytesMut::new();
        motd_payload.put_slice(&[0; 4]);
        motd_payload.put_u8(0);
        framed_write.send(Frame::new(ids::SID_CLANSETMOTD, motd_payload.freeze()).encode_with(0xFF).unwrap()).await.unwrap();
    });

    let (events_tx, mut events_rx) = mpsc::channel(32);
    let cfg = BnetConfig {
        server: addr.ip().to_string(),
        server_alias: "iCCup".into(),
        pvpgn_realm_name: "PvPGN Realm".into(),
        port: addr.port(),
        host_port: 6112,
        username: "testbot".into(),
        password: "secretpassword".into(),
        cdkey_roc: "FFFFFFFFFFFFFFFFFFFFFFFFFF".into(),
        cdkey_tft: "FFFFFFFFFFFFFFFFFFFFFFFFFF".into(),
        first_channel: String::new(),
        root_admins: vec!["slash".into()],
        command_trigger: '!',
        war3_version: 26,
        exe_version: [1, 0, 26, 1],
        exe_version_hash: [0, 0, 0, 0],
        password_hash_type: "pvpgn".into(),
        reconnect_delay: Duration::from_secs(1),
    };

    let (handle, _join) = spawn_bnet(cfg, events_tx);

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

    let ev_fl = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
        .await
        .expect("friends list event")
        .expect("event");
    match ev_fl {
        BnetEvent::FriendsList(list) => {
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].account, "Friend1");
        }
        other => panic!("expected FriendsList, got {other:?}"),
    }

    let ev_cl = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
        .await
        .expect("clan list event")
        .expect("event");
    match ev_cl {
        BnetEvent::ClanList(list) => {
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].name, "ClanMate");
            assert_eq!(list[0].rank, 2);
        }
        other => panic!("expected ClanList, got {other:?}"),
    }

    let ev_inv = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
        .await
        .expect("clan invite event")
        .expect("event");
    match ev_inv {
        BnetEvent::ClanInviteReceived { clan_name, inviter, creation } => {
            assert_eq!(clan_name, "EpicClan");
            assert_eq!(inviter, "ChiefBob");
            assert!(creation);
        }
        other => panic!("expected ClanInviteReceived, got {other:?}"),
    }

    // Accept clan invite so server task can proceed past f_accept
    handle.send(ghost_bnet::BnetCmd::ClanAcceptInvite(true));

    let ev_rank = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
        .await
        .expect("clan rank changed event")
        .expect("event");
    assert_eq!(ev_rank, BnetEvent::ClanRankChanged { status: 0 });

    let ev_rem = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
        .await
        .expect("clan member removed event")
        .expect("event");
    assert_eq!(ev_rem, BnetEvent::ClanMemberRemoved { status: 0 });

    let ev_motd = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
        .await
        .expect("clan motd set event")
        .expect("event");
    assert_eq!(ev_motd, BnetEvent::ClanMotdSet { status: 0 });

    server_task.await.unwrap();
}
