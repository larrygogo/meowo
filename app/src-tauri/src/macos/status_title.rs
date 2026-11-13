//! 菜单栏图标右边的富文本标题（用户 2026-11-05：「像 QQ 音乐那种」「能动，还有颜色」）。
//!
//! tauri 的托盘接口只给纯文本标题：系统默认的小号细字，▶●✓ 这些只能当普通字符，实拍很糙。
//! QQ 音乐那种效果靠直接给 NSStatusItem 的按钮设 attributedTitle：任意字体字重、按段着色、
//! 内嵌 SF Symbols。tray-icon 能拿到底层 NSStatusItem，这里用它设富文本。
//!
//! 内容由前端拼成一段段（见 [`Segment`]），这里只管画。颜色只用系统语义色（systemGreen 等、
//! labelColor），菜单栏深浅色自动适配。
//! 未经本机编译（Windows 开发机），靠 CI macOS 矩阵编译、真机验证。

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;
use serde::Deserialize;
use tauri::AppHandle;

/// 标题里的一段：文字或一个 SF Symbol。
#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Segment {
    Text { text: String, color: String },
    Symbol { name: String, color: String },
}

/// 语义色名 → NSColor（autoreleased）。认不出的用 labelColor。
unsafe fn color(name: &str) -> *mut AnyObject {
    match name {
        "green" => msg_send![class!(NSColor), systemGreenColor],
        "orange" => msg_send![class!(NSColor), systemOrangeColor],
        "red" => msg_send![class!(NSColor), systemRedColor],
        "blue" => msg_send![class!(NSColor), systemBlueColor],
        "secondary" => msg_send![class!(NSColor), secondaryLabelColor],
        _ => msg_send![class!(NSColor), labelColor],
    }
}

/// 拼出富文本（retained，调用方负责 release）。
unsafe fn build(segments: &[Segment]) -> *mut AnyObject {
    let out: *mut AnyObject = msg_send![class!(NSMutableAttributedString), new];
    // 等宽数字的系统字体，字重 Medium（NSFontWeightMedium = 0.23）：数字变化时宽度不跳。
    let font: *mut AnyObject = msg_send![class!(NSFont), monospacedDigitSystemFontOfSize: 12.0f64, weight: 0.23f64];
    let font_key = NSString::from_str("NSFont");
    let color_key = NSString::from_str("NSColor");
    for seg in segments {
        match seg {
            Segment::Text { text, color: c } => {
                let keys: [*const AnyObject; 2] = [
                    (&*font_key as *const NSString).cast(),
                    (&*color_key as *const NSString).cast(),
                ];
                let values: [*const AnyObject; 2] = [font.cast_const(), color(c).cast_const()];
                let attrs: *mut AnyObject = msg_send![
                    class!(NSDictionary),
                    dictionaryWithObjects: values.as_ptr(),
                    forKeys: keys.as_ptr(),
                    count: 2usize
                ];
                let s = NSString::from_str(text);
                let piece: *mut AnyObject = msg_send![class!(NSAttributedString), alloc];
                let piece: *mut AnyObject = msg_send![piece, initWithString: &*s, attributes: attrs];
                let _: () = msg_send![out, appendAttributedString: piece];
                let _: () = msg_send![piece, release];
            }
            Segment::Symbol { name, color: c } => {
                let sym = NSString::from_str(name);
                let image: *mut AnyObject = msg_send![
                    class!(NSImage),
                    imageWithSystemSymbolName: &*sym,
                    accessibilityDescription: std::ptr::null::<AnyObject>()
                ];
                if image.is_null() {
                    continue;
                }
                // 11pt、Semibold（0.3）；颜色用 hierarchical 配置烤进图里（macOS 12+）。
                let size: *mut AnyObject = msg_send![
                    class!(NSImageSymbolConfiguration),
                    configurationWithPointSize: 11.0f64,
                    weight: 0.3f64
                ];
                let tint: *mut AnyObject = msg_send![
                    class!(NSImageSymbolConfiguration),
                    configurationWithHierarchicalColor: color(c)
                ];
                let config: *mut AnyObject = msg_send![size, configurationByApplyingConfiguration: tint];
                let image: *mut AnyObject = msg_send![image, imageWithSymbolConfiguration: config];
                if image.is_null() {
                    continue;
                }
                let attachment: *mut AnyObject = msg_send![class!(NSTextAttachment), new];
                let _: () = msg_send![attachment, setImage: image];
                let piece: *mut AnyObject =
                    msg_send![class!(NSAttributedString), attributedStringWithAttachment: attachment];
                let _: () = msg_send![out, appendAttributedString: piece];
                let _: () = msg_send![attachment, release];
            }
        }
    }
    out
}

/// 设到菜单栏按钮上。必须在主线程调（set_menubar_status 是同步命令，在主线程）。
pub fn apply(app: &AppHandle, segments: &[Segment]) {
    let Some(tray) = app.tray_by_id("meowo-tray") else {
        return;
    };
    // with_inner_tray_icon 要求闭包 'static（可能被派到主线程上跑）：不能借用 segments，拷一份挪进去。
    let segments = segments.to_vec();
    let _ = tray.with_inner_tray_icon(move |inner| {
        let Some(item) = inner.ns_status_item() else {
            return;
        };
        // SAFETY：item 是 tray-icon 持有的 NSStatusItem，Retained 保证调用期间存活；button 与新建的
        // 富文本都只在主线程用。build 返回的对象是 +1，setAttributedTitle: 内部会 copy，之后 release。
        unsafe {
            let item: *const AnyObject = objc2::rc::Retained::as_ptr(&item).cast();
            let button: *mut AnyObject = msg_send![item, button];
            if button.is_null() {
                return;
            }
            let title = build(&segments);
            let _: () = msg_send![button, setAttributedTitle: title];
            let _: () = msg_send![title, release];
        }
    });
}
