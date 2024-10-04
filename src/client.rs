use anyhow::Context;
use log::{error, info, warn};
use tokio::io::AsyncWriteExt;
use uuid::Uuid; // 导入 AsyncReadExt 和 AsyncWriteExt
use std::sync::Arc;
use tokio::net::TcpStream;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::time::timeout;
use crate::auth::*;
use crate::shared::*;
pub struct Client {
    server:Option<DelimitedStream<TcpStream>>,
    server_host:SocketAddr,
    local_ip:Ipv4Addr,
    local_port:u16,
    remote_port:u16,                                      
    auth:Option<Authenticator>,

}
impl Client {
    pub async fn new(server_host:SocketAddr,local_ip:Ipv4Addr,local_port:u16,remote_port:u16,key:Option<&str>) -> anyhow::Result<Self> {
        let stream = connect_with_timeout(server_host).await?;
        let mut stream = DelimitedStream::new(stream);
        let auth = key.map(Authenticator::new);
        if  let Some(auth) = &auth{
            auth.client_handshake(&mut stream).await?;
        }
        stream.send(&ClientMessage::Hello(remote_port)).await?;
        match stream.recv().await?{
            Some(ServerMessage::Hello(port))=>{
                 // 打印调试信息
                if port != remote_port{
                    warn!("{} != {}",port,local_port);
                    anyhow::bail!("server sent unexpected port number: {}",port);
                }
            },
            Some(ServerMessage::Error(e))=>{
                anyhow::bail!("server sent error message: {}",e);
            },
            _=>{}

        }
        info!("connected to server, receive permit port {}",remote_port);
        
        let this = Client{
            server:Some(stream),
            server_host,
            local_ip,
            local_port,
            remote_port,
            auth:key.map(Authenticator::new),
        };
        Ok(this)
    }
    pub async fn listen_local(mut self) -> anyhow::Result<()>{
        let mut server = self.server.take().unwrap();
        let this = Arc::new(self);
        loop{
            match server.recv().await?{
                Some(ServerMessage::Hello(_))=>{
                    warn!("server sent unexpected hello message");
                },
                Some(ServerMessage::HeartBeat)=>{
                    info!("received heartbeat from server");
                },
                Some(ServerMessage::Challenge(_uuid))=>{
                    warn!("server sent unexpected challenge message");
                },
                Some(ServerMessage::Connection(id))=>{
                    let this = Arc::clone(&this);
                    tokio::spawn(async move {
                        info!("new connection from server");
                        if let Err(e) = this.handle_connection(id).await{
                            error!("Error: {}",e);
                            
                        }

                    });
                },
                Some(ServerMessage::Error(e))=>{
                    error!("server sent error message: {}",e);
                },
                _=>{}

            }
        }
    }
    async fn handle_connection(&self,id:Uuid)->anyhow::Result<()>{
        let server_ip = self.server_host.ip();
        let local_ip = self.local_ip;
        let mut remote_stream = DelimitedStream::new(TcpStream::connect(format!("{}:{}",server_ip,self.remote_port)).await?);
        if let Some(auth) = &self.auth{
            auth.client_handshake(&mut remote_stream).await?;
        }
        remote_stream.send(&ClientMessage::Accept(id)).await?;
        let mut local_stream = TcpStream::connect(format!("{}:{}",local_ip,self.local_port)).await?;
        let parts = remote_stream.into_parts();
        // 将远程连接的缓冲区内容写入到本地连接
        local_stream.write_all(&parts.read_buf).await?;
        proxy(local_stream, parts.io).await?;
        Ok(())
    }

}


/// 使用超时机制连接到指定地址和端口。
async fn connect_with_timeout(addr:SocketAddr) -> anyhow::Result<TcpStream> {
    //通过 TcpStream::connect() 函数来进行主动连接
    match timeout(NETWORK_TIMEOUT, TcpStream::connect(addr)).await {
        Ok(res) => res, // 连接成功，返回连接流
        Err(err) => Err(err.into()), // 连接超时或失败，返回错误
    }
    .with_context(|| format!("无法连接到 {addr}")) // 添加上下文信息
}
