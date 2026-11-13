//! 只够和 codex app-server 说 JSON-RPC 的极简 WebSocket 客户端（RFC 6455，仅 ws://、仅文本帧）。
//!
//! 为什么手写而不引 tungstenite：宿主只在关卡答复时连一次本机回环口、发几条短消息，
//! 用不到压缩、TLS、分片发送；为这点用量给整个应用加一棵依赖树（http/sha1/rand…）不划算。
//! 实测要点（codex 0.167.2，2026-11-13 隔离探针）：
//! - 带 `Origin` 头的握手被拒 403（防浏览器跨站），所以这里**不发 Origin**；
//! - capability-token 鉴权走 `Authorization: Bearer <token>`，缺失即 401。
use base64::Engine;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// 单帧上限：thread/started 之类的通知会带上整段线程元数据，给足 16MB，超出按协议错误断开。
const MAX_FRAME: u64 = 16 * 1024 * 1024;

pub(crate) struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next_id: u64,
}

impl Client {
    pub(crate) fn connect(port: u16, token: &str, timeout: Duration) -> Result<Self, String> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|_| "lead/codex_server_unreachable")?;
        stream.set_read_timeout(Some(timeout)).ok();
        stream.set_write_timeout(Some(timeout)).ok();
        stream.set_nodelay(true).ok();
        let mut key = [0u8; 16];
        getrandom::fill(&mut key).map_err(|_| "lead/codex_server_unreachable")?;
        let key = base64::engine::general_purpose::STANDARD.encode(key);
        let request = format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nAuthorization: Bearer {token}\r\n\r\n"
        );
        let mut writer = stream
            .try_clone()
            .map_err(|_| "lead/codex_server_unreachable")?;
        writer
            .write_all(request.as_bytes())
            .map_err(|_| "lead/codex_server_unreachable")?;
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        reader
            .read_line(&mut status)
            .map_err(|_| "lead/codex_server_unreachable")?;
        // 101 之外（401 token 不对、403 等）一律视为连不上，不把响应体带进错误串。
        if !status
            .split_whitespace()
            .nth(1)
            .is_some_and(|code| code == "101")
        {
            return Err(if status.contains(" 401") {
                "lead/codex_server_auth".into()
            } else {
                "lead/codex_server_unreachable".into()
            });
        }
        loop {
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .map_err(|_| "lead/codex_server_unreachable")?;
            if n == 0 {
                return Err("lead/codex_server_unreachable".into());
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
        }
        Ok(Self {
            reader,
            writer,
            next_id: 1,
        })
    }

    pub(crate) fn notify(&mut self, method: &str) -> Result<(), String> {
        let body = serde_json::json!({ "jsonrpc": "2.0", "method": method });
        self.send_text(&body.to_string())
    }

    /// 发请求并等同 id 的响应；途中的通知丢弃（宿主不订阅线程事件）。
    pub(crate) fn call(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let body =
            serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.send_text(&body.to_string())?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let text = self.read_text()?;
            let Ok(message) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(result) = message.get("result") {
                return Ok(result.clone());
            }
            if message.get("error").is_some() {
                return Err("lead/codex_server_rejected".into());
            }
        }
        Err("lead/codex_server_timeout".into())
    }

    fn send_text(&mut self, text: &str) -> Result<(), String> {
        let mut mask = [0u8; 4];
        getrandom::fill(&mut mask).map_err(|_| "lead/codex_server_unreachable")?;
        let frame = encode_client_frame(0x1, text.as_bytes(), mask);
        self.writer
            .write_all(&frame)
            .map_err(|_| "lead/codex_server_unreachable".into())
    }

    /// 读到一条完整文本消息为止：拼接续帧、回 pong、遇 close 报断开。
    fn read_text(&mut self) -> Result<String, String> {
        let mut message = Vec::new();
        loop {
            let (fin, opcode, payload) = read_frame(&mut self.reader)?;
            match opcode {
                0x0..=0x2 => {
                    message.extend_from_slice(&payload);
                    if fin {
                        return String::from_utf8(message)
                            .map_err(|_| "lead/codex_server_protocol".into());
                    }
                }
                0x8 => return Err("lead/codex_server_closed".into()),
                0x9 => {
                    let mut mask = [0u8; 4];
                    getrandom::fill(&mut mask).map_err(|_| "lead/codex_server_unreachable")?;
                    let pong = encode_client_frame(0xA, &payload, mask);
                    self.writer
                        .write_all(&pong)
                        .map_err(|_| "lead/codex_server_unreachable")?;
                }
                _ => {}
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // 礼貌关闭：服务端据此立即释放连接；失败无所谓。
        let _ = self
            .writer
            .write_all(&encode_client_frame(0x8, &[], [0; 4]));
    }
}

/// 客户端帧必须加掩码（RFC 6455 §5.3）。
fn encode_client_frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | opcode);
    let len = payload.len();
    if len < 126 {
        frame.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ mask[i % 4]),
    );
    frame
}

fn read_frame(reader: &mut impl Read) -> Result<(bool, u8, Vec<u8>), String> {
    let err = |_| "lead/codex_server_closed".to_string();
    let mut head = [0u8; 2];
    reader.read_exact(&mut head).map_err(err)?;
    let fin = head[0] & 0x80 != 0;
    let opcode = head[0] & 0x0F;
    let masked = head[1] & 0x80 != 0;
    let mut len = u64::from(head[1] & 0x7F);
    if len == 126 {
        let mut ext = [0u8; 2];
        reader.read_exact(&mut ext).map_err(err)?;
        len = u64::from(u16::from_be_bytes(ext));
    } else if len == 127 {
        let mut ext = [0u8; 8];
        reader.read_exact(&mut ext).map_err(err)?;
        len = u64::from_be_bytes(ext);
    }
    if len > MAX_FRAME {
        return Err("lead/codex_server_protocol".into());
    }
    let mut mask = [0u8; 4];
    if masked {
        reader.read_exact(&mut mask).map_err(err)?;
    }
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).map_err(err)?;
    if masked {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }
    Ok((fin, opcode, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_roundtrip_across_all_length_encodings() {
        for len in [0usize, 5, 125, 126, 65_535, 65_536, 70_000] {
            let payload: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let frame = encode_client_frame(0x1, &payload, [1, 2, 3, 4]);
            let (fin, opcode, decoded) = read_frame(&mut frame.as_slice()).unwrap();
            assert!(fin);
            assert_eq!(opcode, 0x1);
            assert_eq!(decoded, payload, "len={len}");
        }
    }

    #[test]
    fn unmasked_server_frame_and_oversize_guard() {
        let mut frame = vec![0x81, 3];
        frame.extend_from_slice(b"abc");
        assert_eq!(read_frame(&mut frame.as_slice()).unwrap().2, b"abc");
        let mut huge = vec![0x81, 127];
        huge.extend_from_slice(&(MAX_FRAME + 1).to_be_bytes());
        assert_eq!(
            read_frame(&mut huge.as_slice()).unwrap_err(),
            "lead/codex_server_protocol"
        );
    }
}
