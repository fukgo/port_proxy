
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::{io::AsyncWriteExt, net::TcpStream, time::timeout};
use tracing::{error, info, info_span, warn, Instrument};
use uuid::Uuid;

use crate::auth::Authenticator;
use crate::shared::{
    proxy, ClientMessage, DelimitedStream, ServerMessage, NETWORK_TIMEOUT,
};

pub struct Client {
    conn: Option<DelimitedStream<TcpStream>>,
    server_addr: SocketAddr,
    local_ip:String,
    local_port: u16,
    remote_port: u16,
    auth: Option<Authenticator>,
}

impl Client {
    /// Create a new client.
    pub async fn new(
        server_addr: SocketAddr,
        host: &str,
        local_port:u16,
        remote_port:u16,
        secret: Option<&str>,

    ) -> Result<Self> {
        let mut stream = DelimitedStream::new(connect_with_timeout(server_addr).await?);
        let auth = secret.map(Authenticator::new);
        if let Some(auth) = &auth {
            auth.client_handshake(&mut stream).await?;
        }

        stream.send(ClientMessage::Hello(remote_port)).await?;
        match stream.recv_with_timeout().await? {
            Some(ServerMessage::Hello(_)) => {},
            Some(ServerMessage::Error(message)) => bail!("server error: {message}"),
            Some(ServerMessage::Challenge(_)) => {
                bail!("server requires authentication, but no client secret was provided");
            }
            Some(_) => bail!("unexpected initial non-hello message"),
            None => bail!("unexpected EOF"),
        };
        info!("connected to server {}",server_addr);
        info!("public port on server: {}",remote_port);
        Ok(Client {
            conn: Some(stream),
            server_addr,
            local_ip: host.to_string(),
            local_port,
            remote_port,
            auth,

        })
    }

    /// Returns the port publicly available on the remote.
    pub fn remote_port(&self) -> u16 {
        self.remote_port
    }

    /// Start the client, listening for new connections.
    pub async fn listen_local(mut self) -> Result<()> {
        let mut conn = self.conn.take().unwrap();
        let this = Arc::new(self);
        loop {
            match conn.recv().await? {
                Some(ServerMessage::Hello(_)) => warn!("unexpected hello"),
                Some(ServerMessage::Challenge(_)) => warn!("unexpected challenge"),
                Some(ServerMessage::HeartBeat) => (),
                Some(ServerMessage::Connection(id)) => {
                    let this = Arc::clone(&this);
                    tokio::spawn(
                        async move {
                            info!("new connection");
                            match this.handle_connection(id).await {
                                Ok(_) => info!("connection exited"),
                                Err(err) => warn!(%err, "connection exited with error"),
                            }
                        }
                        .instrument(info_span!("proxy", %id)),
                    );
                }
                Some(ServerMessage::Error(err)) => error!(%err, "server error"),
                None => return Ok(()),
            }
        }
    }

    async fn handle_connection(&self, id: Uuid) -> Result<()> {
        let mut remote_conn =
        DelimitedStream::new(connect_with_timeout(self.server_addr).await?);
        if let Some(auth) = &self.auth {
            auth.client_handshake(&mut remote_conn).await?;
        }
        remote_conn.send(ClientMessage::Accept(id)).await?;
        let local_addr = format!("{host}:{local_port}", host = self.local_ip, local_port = self.local_port);
        let mut local_conn = connect_with_timeout(local_addr.parse::<SocketAddr>().unwrap()).await?;
        let parts = remote_conn.into_parts();
        debug_assert!(parts.write_buf.is_empty(), "framed write buffer not empty");
        local_conn.write_all(&parts.read_buf).await?; // mostly of the cases, this will be empty
        proxy(local_conn, parts.io).await?;
        Ok(())
    }
}

async fn connect_with_timeout(addr:SocketAddr) -> Result<TcpStream> {
    match timeout(NETWORK_TIMEOUT, TcpStream::connect(addr)).await {
        Ok(res) => res,
        Err(err) => Err(err.into()),
    }
    .with_context(|| format!("could not connect to {addr}"))
}
