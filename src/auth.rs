use anyhow::{bail, ensure};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;
use crate::shared::{ClientMessage, DelimitedStream, ServerMessage};
// use crate::shared::{ClientMessage, Delimited, ServerMessage};
//Challenge-Response 质询响应验证
/*
服务端 生成一个随机的 challenge 发送给客户端。
客户端 用它的 key 对该 challenge 进行加密或者生成 HMAC 并发送回服务端。
服务端 使用相同的 key 对 challenge 进行相同的操作，验证返回的结果。 */
pub struct Authenticator(Hmac<Sha256>);

impl Authenticator {
    pub fn new(secret: &str) -> Self {
        //创建一个 SHA-256 的哈希实例，然后将 secret 更新到哈希中，最后生成哈希值,用作 HMAC 的密钥
        let hashed_secret = Sha256::new().chain_update(secret).finalize();
        //生成的哈希值来创建 HMAC 实例
        Self(Hmac::new_from_slice(&hashed_secret).expect("HMAC can take key of any size"))
    }

    pub fn encrypt(&self, challenge: &Uuid) -> String {
        let mut hmac = self.0.clone();
        //将challenge的字节序列更新到 HMAC 中
        hmac.update(challenge.as_bytes());
        //字节数组编码为十六进制字符串，并返回这个字符串
        hex::encode(hmac.finalize().into_bytes())
    }

    pub fn validate(&self, challenge: &Uuid, tag: &str) -> bool {
        if let Ok(decoded_tag) = hex::decode(tag) {
            let mut hmac = self.0.clone();
            hmac.update(challenge.as_bytes());
            //使用 HMAC 的 verify_slice 方法验证生成的 HMAC 是否与解码后的标签匹配
            hmac.verify_slice(&decoded_tag).is_ok()
        } else {
            false
        }
    }
    pub async fn server_handshake<S>(
        &self,
        stream: &mut DelimitedStream<S>,
    ) -> anyhow::Result<()> 
    where S: AsyncRead + AsyncWrite + Unpin {
        let challenge = Uuid::new_v4();
        //发送 Challenge 消息
        stream
            .send(&ServerMessage::Challenge(challenge))
            .await?;
        match stream.recv_with_timeout().await?{
            Some(ClientMessage::ChallengeResponse(tag)) => {
                //验证客户端返回的 ChallengeResponse
                ensure!(self.validate(&challenge, &tag), "invalid challenge response");
                Ok(())
            }
            _ => bail!("unexpected message"),
        }
    }
    pub async fn client_handshake<S>(
        &self,
        stream: &mut DelimitedStream<S>,
    ) -> anyhow::Result<()> 
    where S: AsyncRead + AsyncWrite + Unpin {
        match stream.recv_with_timeout().await? {
            Some(ServerMessage::Challenge(challenge)) => {
                //生成 ChallengeResponse
                let tag = self.encrypt(&challenge);
                //发送 ChallengeResponse
                stream
                    .send(&ClientMessage::ChallengeResponse(tag))
                    .await?;
                Ok(())
            }
            _ => bail!("unexpected message"),
        }
    }

}
