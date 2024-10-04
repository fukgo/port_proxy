use std::io;
use anyhow::Context;
//使用 tokio_util::codec 自定义编解码器
/*两个 trait：Decoder 和 Encoder。
Decoder：负责从输入流中解析数据，通常是将原始字节转换为更高级别的 Rust 类型。
Encoder：负责将 Rust 类型转换为原始字节，以便写入输出流。 */
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::time::Duration;
use uuid::Uuid;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::{net::TcpStream, time::timeout};
use tokio_util::codec::{AnyDelimiterCodec, Framed, FramedParts};
/// 流中 JSON 帧的最大字节长度，用于限制单个消息的大小。
pub const MAX_FRAME_LENGTH: usize = 256;
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(3);
#[derive(Debug, Serialize, Deserialize)]
pub enum ServerMessage{
    Challenge(Uuid),
    Hello(u16),
    HeartBeat,
    Connection(Uuid),
    Error(String),
}
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage{ 
    ChallengeResponse(String),
    HeartBeat,
    Accept(Uuid),
    Hello(u16),
    
}
pub struct DelimitedStream<S>(Framed<S,AnyDelimiterCodec>);
impl<S> DelimitedStream<S>
where S: AsyncRead + AsyncWrite + Unpin{
    pub fn new(stream:S)->Self{
        let codec = AnyDelimiterCodec::new_with_max_length(vec![0], vec![0], MAX_FRAME_LENGTH);
        Self(Framed::new(stream,codec))
    }
    pub async fn recv<T:DeserializeOwned>(&mut self)->anyhow::Result<Option<T>>{
        if let Some(next_message) = self.0.next().await {
            let byte_message = next_message.context("帧错误，无效的字节长度")?;
            let serialized_obj =
                serde_json::from_slice(&byte_message).context("无法解析消息")?;
            Ok(serialized_obj)
        } else {
            Ok(None)
        }
    }
    pub async fn send<T:Serialize>(&mut self, message: T) -> anyhow::Result<()> {
        let serialized_message = serde_json::to_string(&message).context("无法序列化消息")?;
        self.0.send(serialized_message).await?;

        Ok(())

    }
    pub fn into_parts(self)->FramedParts<S,AnyDelimiterCodec>{
        self.0.into_parts()
    }
    pub async fn recv_with_timeout<T:DeserializeOwned>(&mut self)->anyhow::Result<Option<T>>{
        timeout(NETWORK_TIMEOUT, self.recv()).await.with_context(|| "网络超时")?
    }

}

pub async fn proxy(stream1:TcpStream,stream2:TcpStream)->io::Result<()>{
    let (mut reader1,mut writer1) = stream1.into_split();
    let (mut reader2,mut writer2) = stream2.into_split();
    let copy1 = tokio::io::copy(&mut reader1,&mut writer2);
    let copy2 = tokio::io::copy(&mut reader2,&mut writer1);
    tokio::try_join!(copy1,copy2)?;
    Ok(())
}