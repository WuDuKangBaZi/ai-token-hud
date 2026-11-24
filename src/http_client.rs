use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;

const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub is_https: bool,
    pub host: String,
    pub port: u16,
    pub path_and_query: String,
}

/// 解析标准 HTTP/HTTPS URL 为主机、端口与请求路径
pub fn parse_http_url(url: &str) -> Result<ParsedUrl, String> {
    let trimmed = url.trim();
    let (is_https, default_port, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
        (true, 443u16, r)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        (false, 80u16, r)
    } else {
        return Err(format!("不支持的 URL 协议: {}", url));
    };

    let without_fragment = rest.split('#').next().unwrap_or(rest);
    let (authority, path_and_query) = match without_fragment.find('/') {
        Some(idx) => (&without_fragment[..idx], without_fragment[idx..].to_string()),
        None => match without_fragment.find('?') {
            Some(q_idx) => (&without_fragment[..q_idx], format!("/{}", &without_fragment[q_idx..])),
            None => (without_fragment, "/".to_string()),
        },
    };

    if authority.is_empty() {
        return Err(format!("URL 缺少主机名: {}", url));
    }

    let (host, port) = if let Some(bracket_end) = authority.strip_prefix('[').and_then(|s| s.find(']')) {
        let h = &authority[1..1 + bracket_end];
        let after = &authority[2 + bracket_end..];
        let p = if let Some(port_str) = after.strip_prefix(':') {
            port_str.parse::<u16>().map_err(|_| format!("无效端口号: {}", url))?
        } else if after.is_empty() {
            default_port
        } else {
            return Err(format!("无效主机格式: {}", url));
        };
        (h.to_string(), p)
    } else if let Some((h, port_str)) = authority.rsplit_once(':') {
        let p = port_str.parse::<u16>().map_err(|_| format!("无效端口号: {}", url))?;
        (h.to_string(), p)
    } else {
        (authority.to_string(), default_port)
    };

    if host.is_empty() {
        return Err(format!("URL 主机名为空: {}", url));
    }

    Ok(ParsedUrl {
        is_https,
        host,
        port,
        path_and_query,
    })
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
struct WinHttpHandle(*mut core::ffi::c_void);

#[cfg(target_os = "windows")]
impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = windows_sys::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// 通过 Windows 原生 WinHTTP 接口执行 HTTPS/HTTP GET 请求，不启动任何外部子进程
#[cfg(target_os = "windows")]
pub fn get_bytes(url: &str, timeout_secs: u32) -> Result<Vec<u8>, String> {
    use core::ffi::c_void;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
        WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
        WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_DECOMPRESSION_FLAG_DEFLATE,
        WINHTTP_DECOMPRESSION_FLAG_GZIP, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DECOMPRESSION,
        WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    };

    let parsed = parse_http_url(url)?;
    let agent = to_wide(concat!("AI-Token-HUD/", env!("CARGO_PKG_VERSION")));
    let host_w = to_wide(&parsed.host);
    let method_w = to_wide("GET");
    let path_w = to_wide(&parsed.path_and_query);

    unsafe {
        let mut h_session = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        );
        if h_session.is_null() {
            h_session = WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                null(),
                null(),
                0,
            );
        }
        if h_session.is_null() {
            return Err(format!("WinHttpOpen 失败 (错误码 {})", GetLastError()));
        }
        let session = WinHttpHandle(h_session);

        let timeout_ms = (timeout_secs.clamp(1, 120) as i32) * 1000;
        let _ = WinHttpSetTimeouts(session.0, timeout_ms, timeout_ms, timeout_ms, timeout_ms);

        let h_connect = WinHttpConnect(session.0, host_w.as_ptr(), parsed.port, 0);
        if h_connect.is_null() {
            return Err(format!("连接主机失败 ({}): 错误码 {}", parsed.host, GetLastError()));
        }
        let connect = WinHttpHandle(h_connect);

        let flags = if parsed.is_https { WINHTTP_FLAG_SECURE } else { 0 };
        let h_request = WinHttpOpenRequest(
            connect.0,
            method_w.as_ptr(),
            path_w.as_ptr(),
            null(),
            null(),
            null(),
            flags,
        );
        if h_request.is_null() {
            return Err(format!("创建 HTTP 请求失败: 错误码 {}", GetLastError()));
        }
        let request = WinHttpHandle(h_request);

        let decomp = WINHTTP_DECOMPRESSION_FLAG_GZIP | WINHTTP_DECOMPRESSION_FLAG_DEFLATE;
        let _ = WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_DECOMPRESSION,
            (&decomp as *const u32).cast::<c_void>(),
            std::mem::size_of::<u32>() as u32,
        );
        let redirect_policy = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP;
        let _ = WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            (&redirect_policy as *const u32).cast::<c_void>(),
            std::mem::size_of::<u32>() as u32,
        );

        if WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) == 0 {
            return Err(format!("发送请求失败 ({}): 错误码 {}", url, GetLastError()));
        }
        if WinHttpReceiveResponse(request.0, null_mut()) == 0 {
            return Err(format!("接收响应超时或失败 ({}): 错误码 {}", url, GetLastError()));
        }

        let mut status_code: u32 = 0;
        let mut status_size: u32 = std::mem::size_of::<u32>() as u32;
        if WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut status_code as *mut u32).cast::<c_void>(),
            &mut status_size,
            null_mut(),
        ) == 0
        {
            return Err(format!("读取 HTTP 状态码失败 ({}): 错误码 {}", url, GetLastError()));
        }
        if !(200..300).contains(&status_code) {
            return Err(format!("HTTP 请求返回状态码 {}: {}", status_code, url));
        }

        let mut body = Vec::new();
        loop {
            let mut available: u32 = 0;
            if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                return Err(format!("读取响应大小失败 ({}): 错误码 {}", url, GetLastError()));
            }
            if available == 0 {
                break;
            }
            let chunk_len = available as usize;
            if body.len().saturating_add(chunk_len) > MAX_RESPONSE_BYTES {
                return Err(format!("响应体超过 {} MB 上限: {}", MAX_RESPONSE_BYTES / (1024 * 1024), url));
            }
            let mut buf = vec![0u8; chunk_len];
            let mut read: u32 = 0;
            if WinHttpReadData(
                request.0,
                buf.as_mut_ptr().cast::<c_void>(),
                available,
                &mut read,
            ) == 0
            {
                return Err(format!("读取响应体失败 ({}): 错误码 {}", url, GetLastError()));
            }
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
        }

        if body.is_empty() {
            return Err(format!("服务端返回空响应: {}", url));
        }
        Ok(body)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_bytes(_url: &str, _timeout_secs: u32) -> Result<Vec<u8>, String> {
    let _ = MAX_RESPONSE_BYTES;
    Err("HTTP client only implemented on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_http_url_variants() {
        assert_eq!(
            parse_http_url("https://models.dev/api.json").unwrap(),
            ParsedUrl {
                is_https: true,
                host: "models.dev".into(),
                port: 443,
                path_and_query: "/api.json".into(),
            }
        );
        assert_eq!(
            parse_http_url("http://127.0.0.1:8080/v1/rates?cur=CNY#top").unwrap(),
            ParsedUrl {
                is_https: false,
                host: "127.0.0.1".into(),
                port: 8080,
                path_and_query: "/v1/rates?cur=CNY".into(),
            }
        );
        assert_eq!(
            parse_http_url("https://openrouter.ai").unwrap(),
            ParsedUrl {
                is_https: true,
                host: "openrouter.ai".into(),
                port: 443,
                path_and_query: "/".into(),
            }
        );
        assert_eq!(
            parse_http_url("https://example.com?foo=bar").unwrap(),
            ParsedUrl {
                is_https: true,
                host: "example.com".into(),
                port: 443,
                path_and_query: "/?foo=bar".into(),
            }
        );
        assert!(parse_http_url("ftp://example.com/a").is_err());
        assert!(parse_http_url("https:///no-host").is_err());
        assert!(parse_http_url("https://bad:port/test").is_err());
    }
}
