use std::net::Ipv4Addr;
use std::env;
use anyhow::Context;
use clap::{Parser, Subcommand};
use log::info;
use port_proxy::server::*;
use port_proxy::client::*;
// use port_proxy::server::Server; // 导入 port_proxy::server::Server
// use port_proxy::client::Client; // 导入 port_proxy::client::Client
use env_logger;
#[derive(Parser)]
#[command(
    name = "port_proxy",
    version = "1.0",
)]
struct Cli {
    /// must to specify a subcommand
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Subcommand)]
enum SubCommand {
    #[command(
        name = "server",
        about = "start a server to bind a port",
        override_usage = "port_proxy server -k [KEY] -b [BIND_SERVER:PORT]" // 自定义 server 子命令的用法
    )]
    Server {
        /// const bind ip
        #[clap(short= 'b' ,long, default_value = "127.0.0.1:7000")]
        bind: String,
        /// you'd better specify a secret
        #[clap(short = 'k', long)]
        key: Option<String>,
    },
    #[command(
        name = "client",
        about = "start a client to connect to a server",
        override_usage = "port_proxy client --connect <SERVER_IP:PORT> --forward <FORWARD_PORT:REMOTE_PORT> -key <KEY>"
    )]
    Client {
        /// specify the remote server bind port
        #[clap(short='c',long)]
        connect: String,
        /// specify the local port u want to trans to remote server
        #[clap(short='f',long)]
        forward: String,
        /// specify the key
        #[clap(short = 'k', long)]
        key: Option<String>,
    },
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.subcmd {
        SubCommand::Server {
            bind,
            key
        } => {
            // 检查bind输入是否合法
            let bind = bind.parse::<std::net::SocketAddr>().with_context(|| format!("invalid bind address: {}", bind))?;
            // 检查key是否合法
            
            // 检查端口是否被可用
            if let Err(_) = std::net::TcpListener::bind(bind) {
                anyhow::bail!("port {} is already in use", bind.port());
            }
            info!("server is running on {}", bind);
            Server::new(key.as_deref(), bind).bind().await?;

            





        },
        SubCommand::Client {
            connect,
            forward,
            key

        } => {
            // 检查connect输入是否合法
            let connect = connect.parse::<std::net::SocketAddr>().with_context(|| format!("invalid connect address: {}", connect))?;
            // 检查forward输入是否合法
            let (local_port,remote_port) = validate_forward(&forward)?;
            // 检查key是否合法
            // let key = key.ok_or_else(|| anyhow::anyhow!("key is required")).with_context(||format!("invalid key"))?;
            // 检查本地端口是否被占用
            if let Err(_) = std::net::TcpListener::bind(format!("127.0.0.1:{:?}",local_port)) {
                anyhow::bail!("port {} is already in use", local_port);
            }
            info!("client is trying to connect to {} and forward {} to {}", connect, local_port, remote_port);
            let local_ip:Ipv4Addr = "127.0.0.1".parse().unwrap();
            Client::new(connect, local_ip, local_port, remote_port,key.as_deref()).await?.listen_local().await?;

        },
    }
    Ok(())
}

#[tokio::main] // 让 main 函数支持异步操作
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", "info"); // 设置日志级别
    env_logger::init(); // 初始化日志
    let cli = Cli::parse(); // 解析命令行参数
    run(cli).await
}

fn validate_forward(forward: &str) -> anyhow::Result<(u16, u16)> {
    let mut iter = forward.split(':');
    let local = iter.next().ok_or_else(|| anyhow::anyhow!("local port is required"))?;
    let remote = iter.next().ok_or_else(|| anyhow::anyhow!("remote port is required"))?;
    let local = local.parse::<u16>().with_context(|| format!("invalid local port: {}", local))?;
    let remote = remote.parse::<u16>().with_context(|| format!("invalid remote port: {}", remote))?;
    Ok((local, remote))
}