use super::latest_chat_target;

#[test]
fn 库里没有会话时托盘打开对话窗新建页而不是设置窗() {
    assert_eq!(latest_chat_target(None).url(), "index.html?newSession=1");
}

#[test]
fn 有最近会话时托盘打开该会话() {
    assert_eq!(latest_chat_target(Some(7)).url(), "index.html?sessionId=7");
}
