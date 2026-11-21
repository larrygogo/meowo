//! `meowo-reporter mcp`：给 agent 用的 stdio MCP 服务器，提供「操作 Meowo 对话窗面板里的
//! 浏览器」这组工具。调用经 broker 的 TCP 通道转给 Meowo 执行（app 侧 browser_agent.rs）。
//!
//! 只在 Meowo 托管的会话里有用：那里的 agent 环境带着 `MEOWO_PTY_ENDPOINT` 和按会话签发的
//! `MEOWO_PTY_TOKEN`，Meowo 凭它知道是哪个会话、该操作哪个面板浏览器。别处（外部终端、
//! Kimi Code 桌面版也读同一份 kimi `mcp.json`）没有这两个变量，工具列表就是空的，不添乱。
//!
//! 另有一个与浏览器无关的工具 `send_image`：agent 想让用户看到一张图时调用，Meowo 把图复制进
//! 自己的持久目录（`~/.meowo/images/sent`），对话里显示成 agent 发来的图片消息（app 侧
//! chat_images.rs）。浏览器开关关掉时以 `mcp --no-browser` 启动，只列这一个工具。
//!
//! 协议是 MCP 的 stdio 传输：每行一条 JSON-RPC 2.0 消息。只实现用得到的几个方法。

use std::io::{BufRead, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use meowo_protocol::broker::{write_v2_handshake, BrokerRequest};
use serde_json::{json, Value};

/// 一次浏览器操作最多等多久（页面加载 + 脚本执行，app 侧各自另有上限）。
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// 只给发图工具、不给浏览器工具的启动参数（用户关了「允许 agent 操作面板浏览器」时）。
pub const NO_BROWSER_FLAG: &str = "--no-browser";

/// 发图工具名。app 侧 pty.rs 按这个名字分流，不走浏览器。
const SEND_IMAGE: &str = "send_image";
/// 改前改后对照工具名。和发图一样不走浏览器、不受浏览器开关约束。
const COMPARE_IMAGES: &str = "compare_images";

fn is_image_tool(name: &str) -> bool {
    name == SEND_IMAGE || name == COMPARE_IMAGES
}

#[derive(Clone, Copy)]
struct Options {
    browser: bool,
}

pub fn run(args: &[String]) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let channel = broker();
    let options = Options {
        browser: !args.iter().any(|a| a == NO_BROWSER_FLAG),
    };
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(reply) = respond(&message, channel.as_ref(), options) {
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
fn respond(message: &Value, channel: Option<&(String, String)>, options: Options) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let mut result = json!({
                "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "meowo", "version": env!("CARGO_PKG_VERSION") },
            });
            // 没有 Meowo 会话令牌（外部终端、Kimi 桌面版、用户自己起的 gemini 读到同一份配置）时
            // 工具列表是空的，也不给说明：否则这段话会进那边模型的系统提示，教它用一组根本不存在的工具。
            if channel.is_some() {
                result["instructions"] = json!(if options.browser {
                    INSTRUCTIONS_WITH_BROWSER
                } else {
                    INSTRUCTIONS_IMAGES_ONLY
                });
            }
            result
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": if channel.is_some() { tools(options) } else { Vec::new() } }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (content, is_error) = match channel {
                Some(_) if !is_image_tool(name) && !options.browser => (
                    vec![json!({ "type": "text", "text": "用户在 Meowo 设置里关掉了「允许 agent 操作面板浏览器」，这组工具现在不能用。" })],
                    true,
                ),
                Some(channel) => match call(channel, name, prepare_args(name, args)) {
                    Ok(content) => (content, false),
                    Err(error) => (vec![json!({ "type": "text", "text": error_text(name, error) })], true),
                },
                None => (
                    vec![
                        json!({ "type": "text", "text": error_text(name, "只有在 Meowo 里托管的会话才能用这组工具".into()) }),
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

const SEND_IMAGE_HINT: &str = concat!(
    "想让用户看到一张图（你截的图、画的图表、改过的图标、找到的参考图）时，用 send_image 把它发给用户：给本地图片文件的路径，可带一句说明。网络上的图先用你自己的下载工具存到本地再发。图片会直接显示在用户的对话里；只在路径里提到图片，用户是看不到的。",
    "\n\n",
    "改界面（样式、布局、组件）时，动手前先截一张当前画面，改完在同样的页面和窗口大小下再截一张，用 compare_images 把两张发给用户：Meowo 会算出哪里变了，给用户看改前、改后和差异图，你也会拿到变化比例和变化范围。范围超出你想改的地方，说明顺手改坏了别处，先查清楚再汇报。",
);

const INSTRUCTIONS_IMAGES_ONLY: &str = SEND_IMAGE_HINT;

const INSTRUCTIONS_WITH_BROWSER: &str = concat!(
    "这是 Meowo 对话窗右侧面板里的浏览器，用户就在旁边看着。凡是要打开网页、在网站上操作（填表、点按钮、查信息）、看本地 dev server 效果的，都优先用这组工具，不用找别的浏览器扩展或桌面控制。需要登录的网站，打开后让用户直接在面板里登录，再继续。用法：先 browser_open 或 browser_snapshot 拿到元素编号（如 e12），再按编号 browser_click / browser_type。提交、付款这类不可撤销的操作，先跟用户确认。面板通常比桌面浏览器窄，快照开头会写当前视口：测响应式布局或按 UA 出页面的 SSR 网站前先看一眼，需要桌面版、手机版或特定请求头时先用 browser_emulate 设好再测。",
    "

",
    "想让用户看到一张图（你截的图、画的图表、改过的图标、找到的参考图）时，用 send_image 把它发给用户：给本地图片文件的路径，可带一句说明。网络上的图先用你自己的下载工具存到本地再发。图片会直接显示在用户的对话里；只在路径里提到图片，用户是看不到的。",
    "

",
    "改界面（样式、布局、组件）时，动手前先截一张当前画面，改完在同样的页面和窗口大小下再截一张，用 compare_images 把两张发给用户：Meowo 会算出哪里变了，给用户看改前、改后和差异图，你也会拿到变化比例和变化范围。范围超出你想改的地方，说明顺手改坏了别处，先查清楚再汇报。browser_screenshot 的结果里有截图存下的本地路径，直接拿来用。",
);

/// 发图失败的回执固定以 `[Image error: <code>] ` 开头（contract.md）：app 侧的失败已带前缀，
/// 这里补上连不上 Meowo 这类通道层失败，前端只认一种形态。
fn error_text(tool: &str, error: String) -> String {
    if !is_image_tool(tool) || error.starts_with(meowo_agent::images::ERROR_PREFIX) {
        return error;
    }
    meowo_agent::images::ImageError::new(meowo_agent::images::ImageErrorCode::Forbidden, error)
        .receipt()
}

/// 发图的本地相对路径按本进程的工作目录补全：本服务器由 agent 拉起，工作目录就是 agent 的；
/// Meowo 那边不知道 agent 在哪个目录。`~/` 也在这里展开。网址原样转发。
fn prepare_args(tool: &str, mut args: Value) -> Value {
    let keys: &[&str] = match tool {
        SEND_IMAGE => &["source"],
        COMPARE_IMAGES => &["before", "after"],
        _ => return args,
    };
    let cwd = std::env::current_dir().ok();
    for key in keys {
        let Some(source) = args.get(*key).and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if let Some(path) = absolute_source(source, cwd.as_deref()) {
            args[*key] = json!(path.to_string_lossy());
        }
    }
    args
}

fn absolute_source(source: &str, cwd: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    if source.is_empty() {
        return None;
    }
    // 网址（含 file:）原样转发；Windows 盘符 `C:` 不算网址方案。
    if let Some((scheme, _)) = source.split_once(':') {
        if scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return None;
        }
    }
    if let Some(rest) = source.strip_prefix("~/").or_else(|| source.strip_prefix("~\\")) {
        return meowo_agent::home_dir().map(|home| home.join(rest));
    }
    let path = std::path::Path::new(source);
    if path.is_absolute() {
        return None;
    }
    cwd.map(|cwd| cwd.join(path))
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

fn compare_images_tool() -> Value {
    tool(
        COMPARE_IMAGES,
        "把改前、改后两张本地截图发给用户对照：Meowo 逐像素比较，生成差异图（变了的地方标红），在用户的对话里显示成一张对照卡（可拖动滑块比较、看差异），手机上也能看。改界面时用它证明改了什么、没改坏别处。两张图最好是同一页面、同样窗口大小下截的；尺寸不同也能比，多出来的部分算作变化。支持 PNG、JPEG、GIF、WebP，单张不超过 20MB，不接受网址。你会收到变化比例和变化范围（像素坐标），图片不会回传给你。",
        json!({
            "before": { "type": "string", "description": "改之前的截图：本地路径（绝对路径，或相对当前工作目录）" },
            "after": { "type": "string", "description": "改之后的截图：本地路径（绝对路径，或相对当前工作目录）" },
            "caption": { "type": "string", "description": "可选。随对照卡显示给用户的一句说明，比如改了什么" }
        }),
        &["before", "after"],
    )
}

fn send_image_tool() -> Value {
    tool(
        SEND_IMAGE,
        "把一张本地图片发给用户看：图片会直接显示在用户的 Meowo 对话里（手机上也能看），用户可以点开看大图、保存。想让用户看到一张图时就用它——你截的图、画的图表、生成或改过的图标/图片文件。只在回复里写图片路径，用户是看不到图的。只接受本地文件，不接受网址：网络上的图先用你自己的下载工具保存到本地，再把本地路径发过来。支持 PNG、JPEG、GIF、WebP、SVG，单张不超过 20MB。调用后你只会收到一句确认，图片不会回传给你。",
        json!({
            "source": { "type": "string", "description": "图片文件的本地路径（绝对路径，或相对当前工作目录）。不接受网址" },
            "caption": { "type": "string", "description": "可选。随图显示给用户的一句说明" }
        }),
        &["source"],
    )
}

fn tools(options: Options) -> Vec<Value> {
    let mut list = vec![send_image_tool(), compare_images_tool()];
    if options.browser {
        list.extend(browser_tools());
    }
    list
}

fn browser_tools() -> Vec<Value> {
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
        tool(
            "browser_emulate",
            "模拟设备：设视口尺寸、UA、附加请求头，跳转后仍保持，直到 reset 或关页签。面板窄时响应式网站会显示手机版，要测桌面版用 device: desktop。每次调用整体替换之前的设置：没给的项回到默认（不模拟），要同时保持手机尺寸并加请求头就在一次调用里一起给。附加请求头会随这个页签发出的所有请求发送（包括第三方域名），别放不该外传的令牌。改了 UA 或请求头会自动刷新页面（SSR 才拿得到新的那一版），只改尺寸不刷新。返回新的页面快照。目前只有 Windows 支持。",
            json!({
                "device": { "type": "string", "enum": ["desktop", "tablet", "mobile", "reset"], "description": "desktop=1280×800；tablet=820×1180 触屏、iPad UA；mobile=390×844 触屏、iPhone UA；reset=全部还原成面板实际大小和默认 UA" },
                "width": { "type": "integer", "description": "视口宽（CSS 像素，200～4096），覆盖预设" },
                "height": { "type": "integer", "description": "视口高（CSS 像素，200～4096），覆盖预设" },
                "user_agent": { "type": "string", "description": "自定义 UA，覆盖预设的 UA" },
                "headers": { "type": "object", "additionalProperties": { "type": "string" }, "description": "附加请求头，如 {\"Accept-Language\": \"en-US\"}" }
            }),
            &[],
        ),
        tool("browser_back", "后退。", json!({}), &[]),
        tool("browser_forward", "前进。", json!({}), &[]),
        tool("browser_reload", "刷新页面。", json!({}), &[]),
        tool(
            "browser_close",
            "关闭你正在用的面板浏览器页签（用户自己开的其他页签不动）。网页事情做完、页签不再需要时用；之后再 browser_open 会新开一个。",
            json!({}),
            &[],
        ),
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

    const ALL: Options = Options { browser: true };
    const IMAGES_ONLY: Options = Options { browser: false };

    fn names(list: &Value) -> Vec<String> {
        list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn handshake_and_tool_list() {
        let init = respond(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), None, ALL).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "meowo");
        // 没有令牌（不在 Meowo 托管的会话里）：不给说明，免得教别处的模型用不存在的工具。
        assert!(init["result"].get("instructions").is_none(), "{init}");
        let channel = ("127.0.0.1:1".to_string(), "t".to_string());
        let init = respond(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }), Some(&channel), ALL).unwrap();
        assert!(init["result"]["instructions"].as_str().unwrap().contains("send_image"));
        // 通知不回。
        assert!(respond(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            None,
            ALL
        )
        .is_none());
        // 不在 Meowo 托管的会话里：没有工具，免得在别处添乱。
        let list = respond(
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            None,
            ALL,
        )
        .unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 0);
        let channel = ("127.0.0.1:1".to_string(), "t".to_string());
        let list = respond(
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
            Some(&channel),
            ALL,
        )
        .unwrap();
        let all = names(&list);
        assert!(all.iter().any(|n| n == "browser_snapshot") && all.iter().any(|n| n == "browser_click"));
        assert!(all.iter().any(|n| n == "send_image"));
        let unknown = respond(
            &json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
            None,
            ALL,
        )
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    /// 浏览器开关关掉（`mcp --no-browser`）：只列发图工具，浏览器工具调用被拒。
    #[test]
    fn no_browser_lists_only_send_image() {
        let channel = ("127.0.0.1:1".to_string(), "t".to_string());
        let list = respond(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            Some(&channel),
            IMAGES_ONLY,
        )
        .unwrap();
        assert_eq!(names(&list), vec!["send_image".to_string(), "compare_images".to_string()]);
        let reply = respond(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "browser_snapshot" } }), Some(&channel), IMAGES_ONLY).unwrap();
        assert_eq!(reply["result"]["isError"], true);
        let init = respond(&json!({ "jsonrpc": "2.0", "id": 3, "method": "initialize", "params": {} }), Some(&channel), IMAGES_ONLY).unwrap();
        assert!(!init["result"]["instructions"].as_str().unwrap().contains("browser_open"));
    }

    #[test]
    fn send_image_schema_and_description() {
        let tool = send_image_tool();
        assert_eq!(tool["inputSchema"]["required"], json!(["source"]));
        assert!(tool["inputSchema"]["properties"]["caption"].is_object());
        assert!(tool["description"].as_str().unwrap().contains("想让用户看到一张图"));
    }

    /// 发图只收本地文件（做空第一轮）：工具描述、参数说明、服务器说明都不再说「可以给网址」，
    /// 并告诉 agent 网络图先下载到本地。
    #[test]
    fn send_image_is_described_as_local_only() {
        let tool = send_image_tool();
        let description = tool["description"].as_str().unwrap();
        let source = tool["inputSchema"]["properties"]["source"]["description"].as_str().unwrap();
        for text in [description, source, INSTRUCTIONS_IMAGES_ONLY, INSTRUCTIONS_WITH_BROWSER] {
            assert!(!text.contains("http(s)"), "{text}");
        }
        assert!(description.contains("不接受网址") && description.contains("保存到本地"));
        assert!(source.contains("不接受网址"));
        assert!(INSTRUCTIONS_IMAGES_ONLY.contains("存到本地"));
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

    /// 发图成功回执：app 回的只有文字（一句话 + 引用行），不会出现 image 块。
    #[test]
    fn send_image_reply_is_text_only() {
        let content = content_of(&json!({ "ok": true, "text": "图片已发给用户。\n[Image: source: C:/x.png]" }));
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn call_outside_meowo_explains_why() {
        let reply = respond(&json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "browser_snapshot" } }), None, ALL).unwrap();
        assert_eq!(reply["result"]["isError"], true);
        // 发图失败一律带 `[Image error: <code>] ` 前缀（contract.md）。
        let reply = respond(&json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": { "name": "send_image", "arguments": { "source": "a.png" } } }), None, ALL).unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"].as_str().unwrap().starts_with("[Image error: forbidden] "));
        // 连不上 Meowo（端口 1 没人听）：同样带前缀。
        let channel = ("127.0.0.1:1".to_string(), "t".to_string());
        let reply = respond(&json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": "send_image", "arguments": { "source": "C:/a.png" } } }), Some(&channel), ALL).unwrap();
        assert!(reply["result"]["content"][0]["text"].as_str().unwrap().starts_with("[Image error: "));
    }

    #[test]
    fn relative_source_resolves_against_cwd() {
        let cwd = std::path::Path::new(if cfg!(windows) { "C:/proj" } else { "/proj" });
        assert_eq!(
            absolute_source("out/chart.png", Some(cwd)),
            Some(cwd.join("out/chart.png"))
        );
        // 绝对路径、网址、file: 网址原样转发（None = 不改）。
        let abs = if cfg!(windows) { "C:/a/b.png" } else { "/a/b.png" };
        assert_eq!(absolute_source(abs, Some(cwd)), None);
        assert_eq!(absolute_source("https://x.com/a.png", Some(cwd)), None);
        assert_eq!(absolute_source("file:///C:/a.png", Some(cwd)), None);
        assert_eq!(absolute_source("", Some(cwd)), None);
        let args = prepare_args("browser_open", json!({ "url": "a" }));
        assert_eq!(args, json!({ "url": "a" }));
    }
}
