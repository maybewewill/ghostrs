use tokio::net::{TcpStream, TcpSocket, TcpListener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::net::{SocketAddr, IpAddr, Ipv4Addr, ToSocketAddrs};
use std::io;
#[derive(Debug)]
pub struct TcpClient {
    stream: Option<TcpStream>,
    send_buffer: Vec<u8>,
    remote_addr: Option<SocketAddr>,
    connected: bool,
    error: Option<io::Error>,
}

impl Clone for TcpClient {
    fn clone(&self) -> Self {
        TcpClient {
            stream: None,
            send_buffer: self.send_buffer.clone(),
            remote_addr: self.remote_addr,
            connected: false,
            error: None,
        }
    }
}

impl TcpClient {
    pub fn new() -> Self {
        TcpClient {
            stream: None,
            send_buffer: Vec::new(),
            remote_addr: None,
            connected: false,
            error: None,
        }
    }

    pub async fn connect(&mut self, host: &str, port: u16) {
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        let server_addr = format!("{}:{}", host, port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut iter| iter.next());
    
        if let Some(server) = server_addr {
            match TcpSocket::new_v4() {
                Ok(socket) => {
                    if let Err(e) = socket.bind(bind_addr) {
                        self.error = Some(e);
                        self.connected = false;
                        return;
                    }
    
                    match socket.connect(server).await {
                        Ok(stream) => {
                            self.stream = Some(stream);
                            self.remote_addr = Some(server);
                            self.connected = true;
                            self.error = None;
                        }
                        Err(e) => {
                            self.error = Some(e);
                            self.connected = false;
                        }
                    }
                }
                Err(e) => {
                    self.error = Some(e);
                    self.connected = false;
                }
            }
        } else {
            self.error = Some(io::Error::new(io::ErrorKind::InvalidInput, "Invalid host"));
            self.connected = false;
        }
    }
    
    pub async fn put_bytes(&mut self, data: &[u8]) {
        self.send_buffer.extend_from_slice(data);
    }

    pub async fn do_send_buff(&mut self) {
        let buffer_copy = self.send_buffer.clone();
        self.do_send(&buffer_copy).await;
    }

    pub async fn do_send(&mut self, data: &[u8]) -> io::Result<u8> {
        match self.stream.as_mut() {
            Some(stream) => {
                match stream.write_all(data).await {
                    Ok(()) => {
                        //log_info(&format!("Sent {} bytes", data.len()));
                        Ok(data.len() as u8)
                    }
                    Err(e) => {
                        let error = io::Error::new(e.kind(), e.to_string());
                        self.error = Some(error);
                        Err(io::Error::new(e.kind(), e.to_string()))
                    }
                }
            }
            None => Err(io::Error::new(io::ErrorKind::NotConnected, "No connection")),
        }
    }
    

    pub async fn do_recv(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(stream) = &mut self.stream {
            match stream.read(buf).await {
                Ok(n) => Ok(n),
                Err(e) => {
                    let error = io::Error::new(e.kind(), e.to_string());
                    self.error = Some(error);
                    Err(io::Error::new(e.kind(), e.to_string()))
                }
            }
        } else {
            Err(io::Error::new(io::ErrorKind::NotConnected, "No connection"))
        }
    }
    
    pub fn get_port(&self) -> Option<u16> {
        self.remote_addr.map(|addr| addr.port())
    }

    pub fn get_ip(&self) -> Option<Vec<u8>> {
        self.remote_addr.map(|addr| match addr.ip() {
            IpAddr::V4(ipv4) => ipv4.octets().to_vec(),
            IpAddr::V6(ipv6) => ipv6.octets().to_vec(),
        })
    }
    

    pub fn get_ip_string(&self) -> String {
        self.remote_addr
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "Not connected".into())
    }

    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    pub fn get_error_string(&self) -> String {
        self.error
            .as_ref()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "No error".into())
    }

    pub fn connected(&self) -> bool {
        self.connected
    }

    pub fn get_connected(&self) -> bool {
        self.connected()
    }
    pub fn disconnect(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.shutdown(); // закрыть соединение (можно игнорировать результат)
        }
        self.connected = false;
        self.remote_addr = None;
    }
    
}

#[derive(Debug)]
pub struct TcpServer {
    listener: Option<TcpListener>,
    local_addr: Option<SocketAddr>,
    connected: bool,
    error: Option<io::Error>,
}

impl Clone for TcpServer {
    fn clone(&self) -> Self {
        TcpServer {
            listener: None, // Listener is not clonable; create a new one if needed
            local_addr: self.local_addr,
            connected: false,
            error: None,
        }
    }
}

impl TcpServer {
    pub fn new() -> Self {
        TcpServer {
            listener: None,
            local_addr: None,
            connected: false,
            error: None,
        }
    }

    pub async fn bind(&mut self, host: &str, port: u16) {
        let addr = format!("{}:{}", host, port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut iter| iter.next());

        if let Some(server_addr) = addr {
            match TcpListener::bind(server_addr).await {
                Ok(listener) => {
                    self.listener = Some(listener);
                    self.local_addr = Some(server_addr);
                    self.connected = true;
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(e);
                    self.connected = false;
                }
            }
        } else {
            self.error = Some(io::Error::new(io::ErrorKind::InvalidInput, "Invalid host"));
            self.connected = false;
        }
    }

    pub async fn accept(&mut self) -> io::Result<TcpClient> {
        if let Some(listener) = &mut self.listener {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let mut client = TcpClient::new();
                    client.stream = Some(stream);
                    client.remote_addr = Some(addr);
                    client.connected = true;
                    client.error = None;
                    Ok(client)
                }
                Err(e) => {
                    let error = io::Error::new(e.kind(), e.to_string());
                    self.error = Some(error);
                    Err(e)
                }
            }
        } else {
            Err(io::Error::new(io::ErrorKind::NotConnected, "Server not bound"))
        }
    }

    pub fn get_local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    pub fn get_ip(&self) -> Option<Vec<u8>> {
        self.local_addr.map(|addr| match addr.ip() {
            IpAddr::V4(ipv4) => ipv4.octets().to_vec(),
            IpAddr::V6(ipv6) => ipv6.octets().to_vec(),
        })
    }

    

    pub fn get_ip_string(&self) -> String {
        self.local_addr
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "Not bound".into())
    }

    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    pub fn get_error_string(&self) -> String {
        self.error
            .as_ref()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "No error".into())
    }

    pub fn connected(&self) -> bool {
        self.connected
    }

    pub fn get_connected(&self) -> bool {
        self.connected()
    }

    pub fn shutdown(&mut self) {
        self.listener = None;
        self.connected = false;
        self.local_addr = None;
    }

    // Placeholder methods for game server context
    pub async fn update(&mut self) {}

    pub async fn send_to_all(&mut self, _data: &[u8]) {}

    pub async fn send_to_client(&mut self, _client: &TcpClient, _data: &[u8]) {}
}
