//! `meowo-reporter mcp`：给 agent 用的 stdio MCP 服务器，提供「操作 Meowo 对话窗面板里的
//! 浏览器」这组工具。调用经 broker 的 TCP 通道转给 Meowo 执行（app 侧 browser_agent.rs）。
//!
//! 只在 Meowo 托管的会话里有用：那里的 agent 环境带着 `MEOWO_PTY_ENDPOINT` 和按会话签发的
//! `MEOWO_PTY_TOKEN`，Meowo 凭它知道是哪个会话、该操作哪个面板浏览器。别处（外部终端、
//! Kimi Code 桌面版也读同一份 kimi `mcp.json`）没有这两个变量，工具列表就是空的，不添乱。
//!
//! 协议是 MCP 的 stdio 传输：每行一条 JSON-RPC 2.0 消息。只实现用得到的几个方法。

use std::io::{BufRead, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use meowo_protocol::broker::{write_v2_handshake, BrokerRequest};
use serde_json::{json, Value};

/// 一次浏览器操作最多等多久（页面加载 + 脚本执行，app 侧各自另有上限）。
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

pub fn run() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let channel = broker();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(reply) = respond(&message, channel.as_ref()) {
            let _ = writeln!(stdout, "{reply}");
            let _ = stdout.flush();
        }
    }
}

fn broker() -> Option<(String, String)> {
    let endpoint = std::env::var("MEOWO_PTY_ENDPOINT")
        .ok()
        .filter(|v| !v.is_empty())?;
    let token = std::env::var("MEOWO_PTY_TOKEN")
        .ok()
        .filter(|v| !v.is_empty())?;
    Some((endpoint, token))
}

/// 处理一条消息；通知（没有 id）不回。
fn respond(message: &Value, channel: Option<&(String, String)>) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "meowo", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "这是 Meowo 对话窗右侧面板里的浏览器，用户就在旁边看着。凡是要打开网页、在网站上操作（填表、点按钮、查信息）、看本地 dev server 效果的，都优先用这组工具，不用找别的浏览器扩展或桌面控制。需要登录的网站，打开后让用户直接在面板里登录，再继续。用法：先 browser_open 或 browser_snapshot 拿到元素编号（如 e12），再按编号 browser_click / browser_type。提交、付款这类不可撤销的操作，先跟用户确认。",
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": if channel.is_some() { tools() } else { Vec::new() } }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (content, is_error) = match channel {
                Some(channel) => match call(channel, name, args) {
                    Ok(content) => (content, false),
                    Err(error) => (vec![json!({ "type": "text", "text": error })], true),
                },
                None => (
                    vec![
                        json!({ "type": "text", "text": "只有在 Meowo 里托管的会话才能用面板浏览器" }),
                    ],
                    true,
                ),
            };
            json!({ "content": content, "isError": is_error })
        }
        _ => {
            return Some(
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("不支持的方法：{method}") } }),
            );
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// 转给 Meowo 执行，回来的是 MCP 的 content 列表：文字一段，截图另加一张图。
fn call(
    (endpoint, token): &(String, String),
    tool: &str,
    args: Value,
) -> Result<Vec<Value>, String> {
    let addr = endpoint
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .ok_or("连不上 Meowo（地址无效）")?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .map_err(|_| "连不上 Meowo，它可能已经退出了".to_string())?;
    let _ = stream.set_read_timeout(Some(CALL_TIMEOUT));
    write_v2_handshake(
        &mut stream,
        &BrokerRequest::Browser {
            token: token.clone(),
            tool: tool.to_string(),
            args,
        },
    )
    .map_err(|_| "向 Meowo 发送请求失败".to_string())?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .map_err(|_| "等 Meowo 回应超时".to_string())?;
    if reply.trim().is_empty() {
        // 旧版 Meowo 不认识这种请求，会直接断开。
        return Err("Meowo 没有回应：可能版本太旧，不支持面板浏览器".into());
    }
    let value: Value =
        serde_json::from_str(&reply).map_err(|_| "Meowo 的回应无法解析".to_string())?;
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(content_of(&value))
    } else {
        Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("操作失败")
            .to_string())
    }
}

fn content_of(value: &Value) -> Vec<Value> {
    let mut content = Vec::new();
    if let (Some(data), Some(mime)) = (
        value.get("image").and_then(Value::as_str),
        value.get("mime").and_then(Value::as_str),
    ) {
        content.push(json!({ "type": "image", "data": data, "mimeType": mime }));
    }
    let text = value.get("text").and_then(Value::as_str).unwrap_or("");
    content.push(json!({ "type": "text", "text": text }));
    content
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

fn tools() -> Vec<Value> {
    let r = json!({ "type": "string", "description": "元素编号，来自 browser_snapshot，如 e12" });
    vec![
        tool(
            "browser_open",
            "打开面板浏览器（没有就新建一个页签），给了 url 就跳过去。返回页面快照。",
            json!({ "url": { "type": "string", "description": "可选。网址，localhost:5173 这类会自动补 http://" } }),
            &[],
        ),
        tool(
            "browser_navigate",
            "在面板浏览器里打开网址，等加载完返回页面快照。",
            json!({ "url": { "type": "string" } }),
            &["url"],
        ),
        tool(
            "browser_snapshot",
            "读取当前页面：标题、网址、文字，以及可交互元素和它们的编号。",
            json!({}),
            &[],
        ),
        tool(
            "browser_click",
            "按编号点击元素，返回点击后的页面快照。",
            json!({ "ref": r }),
            &["ref"],
        ),
        tool(
            "browser_type",
            "往输入框里填文字（会替换原有内容），可选同时提交表单。返回页面快照。",
            json!({ "ref": r, "text": { "type": "string" }, "submit": { "type": "boolean", "description": "填完后提交（相当于按回车）" } }),
            &["ref", "text"],
        ),
        tool(
            "browser_select",
            "在下拉框里选一项（按选项文字或值）。",
            json!({ "ref": r, "value": { "type": "string" } }),
            &["ref", "value"],
        ),
        tool(
            "browser_press",
            "向当前焦点元素按一个键，如 Enter、Escape、ArrowDown、Tab。",
            json!({ "key": { "type": "string" } }),
            &["key"],
        ),
        tool(
            "browser_scroll",
            "滚动页面：给 ref 就滚到那个元素，否则按 dy 像素向下滚（负数向上）。",
            json!({ "ref": r, "dy": { "type": "number" } }),
            &[],
        ),
        tool(
            "browser_wait_for",
            "等页面上出现某段文字（最多 seconds 秒）；不给 text 就单纯等待。",
            json!({ "text": { "type": "string" }, "seconds": { "type": "number" } }),
            &[],
        ),
        tool("browser_screenshot", "截下面板浏览器当前的画面（图片）。要看排版、样式、颜色、图表这些快照文字表达不了的东西时用。", json!({}), &[]),
        tool("browser_back", "后退。", json!({}), &[]),
        tool("browser_forward", "前进。", json!({}), &[]),
        tool("browser_reload", "刷新页面。", json!({}), &[]),
        tool(
            "browser_evaluate",
            "在页面里执行一段 JavaScript 表达式，返回结果（会转成 JSON）。可以用 await（如 await fetch(...).then(r => r.json())），最多等 15 秒。读不到快照里的内容时用。",
            json!({ "expression": { "type": "string" } }),
            &["expression"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_and_tool_list() {
        let init = respond(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), None).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "meowo");
        // 通知不回。
        assert!(respond(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            None
        )
        .is_none());
        // 不在 Meowo 托管的会话里：没有工具，免得在别处添乱。
        let list = respond(
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            None,
        )
        .unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 0);
        let channel = ("127.0.0.1:1".to_string(), "t".to_string());
        let list = respond(
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
            Some(&channel),
        )
        .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"browser_snapshot") && names.contains(&"browser_click"));
        let unknown = respond(
            &json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
            None,
        )
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn screenshot_reply_becomes_image_content() {
        let content = content_of(
            &json!({ "ok": true, "text": "截图", "image": "QUJD", "mime": "image/jpeg" }),
        );
        assert_eq!(
            content[0],
            json!({ "type": "image", "data": "QUJD", "mimeType": "image/jpeg" })
        );
        assert_eq!(content[1]["text"], "截图");
        assert_eq!(content_of(&json!({ "ok": true, "text": "t" })).len(), 1);
    }

    #[test]
    fn call_outside_meowo_explains_why() {
        let reply = respond(&json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "browser_snapshot" } }), None).unwrap();
        assert_eq!(reply["result"]["isError"], true);
    }
}
