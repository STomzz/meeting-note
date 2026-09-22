//! WebView 媒体权限（Windows / WebView2）。
//!
//! `getUserMedia` 除了系统级麦克风权限，还要宿主应答 WebView 引擎自己的权限请求：
//! WebView2 在没人处理 `PermissionRequested` 时走它自己的默认行为（旧版本直接静默拒绝），
//! 而 Tauri 2.11 还没暴露跨平台权限 API，所以这里直接挂 WebView2 事件。
//!
//! 只放行**本应用页面**的麦克风/摄像头；定位、通知等其它类别保持默认行为不变。
//! Linux(WebKitGTK) 与 Android 走各自平台的默认路径（Android 由系统运行时权限弹窗处理）。

/// 请求是否来自本应用自己的页面：
/// - Windows 打包后：`http(s)://tauri.localhost`
/// - 开发模式（`tauri dev`）：`http://localhost:1420`
/// - 其它平台的 Tauri 自定义协议：`tauri://localhost`
#[cfg_attr(not(windows), allow(dead_code))] // 非 Windows 平台目前只在单测里使用
pub fn is_app_origin(uri: &str) -> bool {
    if let Some(rest) = uri.strip_prefix("tauri://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        return host == "localhost";
    }
    let Some((scheme, rest)) = uri.split_once("://") else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host_port.split(':').next().unwrap_or("");
    host == "localhost" || host == "127.0.0.1" || host == "tauri.localhost"
}

/// 给窗口注册 WebView2 媒体权限处理器（只对 Windows 生效）。
#[cfg(windows)]
pub fn grant_media_capture(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2PermissionRequestedEventArgs, COREWEBVIEW2_PERMISSION_KIND_CAMERA,
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION,
        COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    };
    use webview2_com::{take_pwstr, PermissionRequestedEventHandler};
    use windows::core::PWSTR;

    window.with_webview(|webview| {
        let core = match unsafe { webview.controller().CoreWebView2() } {
            Ok(core) => core,
            Err(e) => {
                eprintln!("[bnu-notes] WebView2 尚未就绪，媒体权限处理器未注册: {e}");
                return;
            }
        };
        let handler = PermissionRequestedEventHandler::create(Box::new(
            |_sender, args: Option<ICoreWebView2PermissionRequestedEventArgs>| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
                unsafe { args.PermissionKind(&mut kind)? };
                if kind != COREWEBVIEW2_PERMISSION_KIND_MICROPHONE
                    && kind != COREWEBVIEW2_PERMISSION_KIND_CAMERA
                {
                    return Ok(());
                }
                let mut uri = PWSTR::null();
                unsafe { args.Uri(&mut uri)? };
                let uri = take_pwstr(uri);
                if is_app_origin(&uri) {
                    unsafe { args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)? };
                }
                Ok(())
            },
        ));
        let mut token = 0i64;
        if let Err(e) = unsafe { core.add_PermissionRequested(&handler, &mut token) } {
            eprintln!("[bnu-notes] 注册 WebView2 媒体权限处理器失败: {e}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::is_app_origin;

    #[test]
    fn only_app_origin_allowed() {
        // 打包后的自定义协议（Windows 用 http(s)://，其它平台用 tauri://）
        assert!(is_app_origin("http://tauri.localhost/"));
        assert!(is_app_origin("http://tauri.localhost/index.html?a=1"));
        assert!(is_app_origin("https://tauri.localhost/"));
        assert!(is_app_origin("tauri://localhost/"));
        // 开发服务器
        assert!(is_app_origin("http://localhost:1420/"));
        assert!(is_app_origin("http://127.0.0.1:1420/"));
        // 外部页面一律不放行
        assert!(!is_app_origin("https://example.com/"));
        assert!(!is_app_origin("http://localhost.evil.com/"));
        assert!(!is_app_origin("http://127.0.0.1.evil.com/"));
        assert!(!is_app_origin("http://tauri.localhost.evil.com/"));
        assert!(!is_app_origin("file:///tmp/index.html"));
        assert!(!is_app_origin(""));
    }
}
