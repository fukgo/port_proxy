use log::{error, info,warn};
use std::net::SocketAddr;
use crate::{auth::Authenticator, shared::{proxy, ClientMessage, ServerMessage}};
use std::sync::Arc;
use tokio::{io::AsyncWriteExt, net::{TcpListener, TcpStream}};
use uuid::Uuid;
use dashmap::DashMap;
use crate::shared::DelimitedStream;
use tokio::time::Duration;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
pub struct Server{    /// 可选的认证器，用于客户端认证。
    auth: Option<Authenticator>,

    /// 并发的哈希表，用于存储 UUID 对应的连接。
    conns: Arc<DashMap<Uuid, TcpStream>>,
    bind: SocketAddr,
    
}

impl Server {
    pub fn new(key:Option<&str>,bind_ip:SocketAddr) -> Self {
        Self {
            auth: key.map(Authenticator::new),
            conns: Arc::new(DashMap::new()),
            bind: bind_ip
        }
    }
    pub async fn bind(self) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(self.bind).await?;
        info!("server is running on {}", self.bind);
        let server = Arc::new(self);

        loop{
            let (stream, _) = listener.accept().await?;
            let server = Arc::clone(&server);
            tokio::spawn(async move {
                info!("new connection from {}", stream.peer_addr().unwrap());
                if let Err(e) = server.handle_connection(stream).await {
                    error!("Error: {}", e);
                }
            });

        }
    }
    async fn handle_connection(&self, stream: TcpStream) -> anyhow::Result<()> {
        let mut stream = DelimitedStream::new(stream);
        if let Some(auth) = &self.auth {
            auth.server_handshake(&mut stream).await?;
            if let Some(auth) =&self.auth{
                if let Err(e) = auth.server_handshake(&mut stream).await{
                    error!("Error: {}", e);
                    stream.send(ServerMessage::Error(e.to_string())).await?;
                    return Ok(());
                }
            }
        }
        match stream.recv_with_timeout().await?{
            Some(ClientMessage::Hello(port)) =>{
                let listener = match self.create_listener(port).await{
                    Ok(listener) => listener,
                    Err(e) => {
                        stream.send(ServerMessage::Error(e.to_string())).await?;
                        return Ok(());
                    }

                };
                stream.send(ServerMessage::Hello(port)).await?;
                loop{
                    if let Ok(res) = tokio::time::timeout(CONNECT_TIMEOUT,listener.accept()).await{
                        match res{
                            Ok((income_stream,_addr))=>{
                                let id = Uuid::new_v4();
                                let conns = Arc::clone(&self.conns);
                                conns.insert(id,income_stream);
                                //异步实现将链接使用后的连接移除
                                tokio::spawn(async move { // 启动异步任务处理连接。
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                    if conns.remove(&id).is_some() {
                                        warn!("移除过期连接:{}",id);
                                    }
                                });
                                stream.send(ServerMessage::Connection(id)).await?;

                            },
                            Err(e)=>{
                                stream.send(ServerMessage::Error(e.to_string())).await?;
                                return Ok(());
                            }
                        }
                    }
                }

            },
            Some(ClientMessage::Accept(id)) =>{
                info!("accept connection:{} and start forward stream",id);
                if let Some((_id,mut income_stream)) = self.conns.remove(&id) {
                    let parts = stream.into_parts();
                    income_stream.write_all(&parts.read_buf).await?;
                    let local_stream = parts.io;
                    proxy(local_stream, income_stream).await?;
                } else {
                    stream.send(ServerMessage::Error("invalid connection id".to_string())).await?;
                    return Ok(());
                }
                self.conns.remove(&id);

            },
            Some(ClientMessage::ChallengeResponse(_)) =>{},
            Some(ClientMessage::HeartBeat) =>{},
            _ =>{
                stream.send(ServerMessage::Error("unexpected message".to_string())).await?;
                return Ok(());
            }


        }


        Ok(())
    }
    async fn create_listener(&self, port:u16)->anyhow::Result<TcpListener,&'static str>{
        let try_bind = |port:u16| async move{
            TcpListener::bind((self.bind.ip(),port))
                .await
                .map_err(|e|match e.kind(){
                    tokio::io::ErrorKind::AddrInUse => "端口已被占用",
                    tokio::io::ErrorKind::PermissionDenied => "权限不足",
                    _ => "绑定端口失败",
                })
            };
        match try_bind(port).await{
            Ok(listener)=>return Ok(listener),
            Err(e)=>return Err(e),
        }
    }
}


