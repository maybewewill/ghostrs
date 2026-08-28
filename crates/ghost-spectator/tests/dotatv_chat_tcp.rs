use ghost_protocol::dotatv::{decode_chat, encode_client_chat, ids as dotatv_ids};
use ghost_spectator::{RelayConfig, spawn_relay};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn test_dotatv_viewer_chat_tcp_e2e() {
    let temp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = temp_listener.local_addr().unwrap().port();
    drop(temp_listener);

    let (_relay_handle, _join) = spawn_relay(RelayConfig {
        port,
        delay: Duration::ZERO,
        max_viewers: 10,
        game_name: "DotaTV TCP Test".into(),
        history_max_mb: 16,
    });

    // Allow the relay server time to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut viewer1 = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .expect("viewer1 must connect");
    let mut viewer2 = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .expect("viewer2 must connect");

    // Viewer 1 sends a DotaTV client chat frame (0xFD, 0x81)
    let chat_frame = encode_client_chat("Hello from viewer 1!").expect("must encode client chat");
    viewer1
        .write_all(&chat_frame)
        .await
        .expect("must write chat");
    viewer1.flush().await.expect("must flush");

    // Viewer 2 should receive the broadcasted chat message (0xFD, 0x80)
    let mut buf = vec![0u8; 1024];
    let mut received_chat = false;

    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(n) =
            tokio::time::timeout(Duration::from_millis(200), viewer2.read(&mut buf)).await
        {
            let n = n.unwrap_or(0);
            if n >= 4 {
                // Find any 0xFD 0x80 packet in received stream
                let mut offset = 0;
                while offset + 4 <= n {
                    if buf[offset] == 0xFD && buf[offset + 1] == dotatv_ids::CHAT {
                        let len = u16::from_le_bytes([buf[offset + 2], buf[offset + 3]]) as usize;
                        if offset + len <= n {
                            let payload =
                                bytes::Bytes::copy_from_slice(&buf[offset + 4..offset + len]);
                            if let Ok(chat) = decode_chat(&payload)
                                && chat.text == "Hello from viewer 1!"
                            {
                                assert!(chat.sender.starts_with("Viewer-"));
                                received_chat = true;
                                break;
                            }
                        }
                    }
                    offset += 1;
                }
                if received_chat {
                    break;
                }
            }
        }
    }

    assert!(
        received_chat,
        "viewer2 must receive broadcasted chat from viewer1"
    );
}
