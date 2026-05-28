use crate::settings::{load_settings, ProxyMode, ProxyProtocol, ProxySettings};

pub fn proxy_url_from_settings(settings: &ProxySettings) -> Result<Option<String>, String> {
    if settings.mode != ProxyMode::Custom {
        return Ok(None);
    }

    let host = settings.host.trim().trim_end_matches('/');
    if host.is_empty() {
        return Err("Proxy host is required for custom proxy mode".to_string());
    }

    let without_scheme = host.split_once("://").map(|(_, rest)| rest).unwrap_or(host);
    let has_port = without_scheme
        .rsplit('/')
        .next()
        .unwrap_or(without_scheme)
        .contains(':');
    let authority = match (settings.port, has_port) {
        (Some(port), false) => format!("{}:{}", without_scheme, port),
        (_, true) => without_scheme.to_string(),
        (None, false) => {
            return Err("Proxy host and port are required for custom proxy mode".to_string())
        }
    };

    let scheme = match settings.protocol {
        ProxyProtocol::Http => "http",
        ProxyProtocol::Socks5 => "socks5h",
    };
    let username = settings.username.trim();
    let user_info = if username.is_empty() {
        String::new()
    } else {
        format!(
            "{}:{}@",
            urlencoding::encode(username),
            urlencoding::encode(&settings.password)
        )
    };

    Ok(Some(format!("{}://{}{}", scheme, user_info, authority)))
}

fn redact_proxy_password(message: String, password: &str) -> String {
    if password.is_empty() {
        message
    } else {
        message.replace(password, "<redacted>")
    }
}

pub fn custom_proxy_bypass_list() -> &'static str {
    "localhost,127.0.0.1,::1"
}

pub fn client_builder_with_proxy_settings(
    settings: &ProxySettings,
) -> Result<reqwest::ClientBuilder, String> {
    let builder = reqwest::Client::builder();
    match settings.mode {
        ProxyMode::System => Ok(builder),
        ProxyMode::Disabled => Ok(builder.no_proxy()),
        ProxyMode::Custom => {
            let url = proxy_url_from_settings(settings)?
                .ok_or_else(|| "Custom proxy URL was not resolved".to_string())?;
            let proxy = reqwest::Proxy::all(&url)
                .map_err(|error| {
                    redact_proxy_password(
                        format!("Invalid proxy settings: {}", error),
                        &settings.password,
                    )
                })?
                .no_proxy(reqwest::NoProxy::from_string(custom_proxy_bypass_list()));

            Ok(builder.proxy(proxy))
        }
    }
}

pub fn http_client_builder() -> Result<reqwest::ClientBuilder, String> {
    let settings = load_settings();
    client_builder_with_proxy_settings(&settings.proxy)
}

pub fn build_http_client() -> Result<reqwest::Client, String> {
    http_client_builder()?
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

#[cfg(test)]
pub fn build_client_with_proxy_settings(
    settings: &ProxySettings,
) -> Result<reqwest::Client, String> {
    client_builder_with_proxy_settings(settings)?
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

#[cfg(test)]
mod tests {
    use crate::http_client::{
        build_client_with_proxy_settings, custom_proxy_bypass_list, proxy_url_from_settings,
    };
    use crate::settings::{ProxyMode, ProxyProtocol, ProxySettings};

    #[test]
    fn custom_http_proxy_url_is_normalized_from_host_and_port() {
        let settings = ProxySettings {
            mode: ProxyMode::Custom,
            protocol: ProxyProtocol::Http,
            host: "proxy.corp.local".to_string(),
            port: Some(8080),
            username: String::new(),
            password: String::new(),
        };

        let proxy_url = proxy_url_from_settings(&settings).expect("proxy url should build");

        assert_eq!(proxy_url, Some("http://proxy.corp.local:8080".to_string()));
    }

    #[test]
    fn custom_socks_proxy_url_uses_remote_dns_scheme() {
        let settings = ProxySettings {
            mode: ProxyMode::Custom,
            protocol: ProxyProtocol::Socks5,
            host: "127.0.0.1".to_string(),
            port: Some(1080),
            username: String::new(),
            password: String::new(),
        };

        let proxy_url = proxy_url_from_settings(&settings).expect("proxy url should build");

        assert_eq!(proxy_url, Some("socks5h://127.0.0.1:1080".to_string()));
    }

    #[test]
    fn custom_proxy_with_credentials_percent_encodes_user_info() {
        let settings = ProxySettings {
            mode: ProxyMode::Custom,
            protocol: ProxyProtocol::Http,
            host: "proxy.corp.local".to_string(),
            port: Some(8080),
            username: "domain\\user@example.com".to_string(),
            password: "pa:ss@word".to_string(),
        };

        let proxy_url = proxy_url_from_settings(&settings).expect("proxy url should build");

        assert_eq!(
            proxy_url,
            Some(
                "http://domain%5Cuser%40example.com:pa%3Ass%40word@proxy.corp.local:8080"
                    .to_string()
            )
        );
    }

    #[test]
    fn custom_proxy_without_host_or_port_is_rejected() {
        let settings = ProxySettings {
            mode: ProxyMode::Custom,
            protocol: ProxyProtocol::Http,
            host: " ".to_string(),
            port: None,
            username: String::new(),
            password: String::new(),
        };

        let err = proxy_url_from_settings(&settings).expect_err("missing endpoint should fail");

        assert!(err.contains("host"));
    }

    #[test]
    fn disabled_proxy_builds_client_with_proxy_disabled() {
        let settings = ProxySettings {
            mode: ProxyMode::Disabled,
            ..ProxySettings::default()
        };

        let client = build_client_with_proxy_settings(&settings);

        assert!(client.is_ok());
    }

    #[test]
    fn custom_proxy_bypass_list_keeps_loopback_direct() {
        let bypass = custom_proxy_bypass_list();

        assert!(bypass.contains("localhost"));
        assert!(bypass.contains("127.0.0.1"));
        assert!(bypass.contains("::1"));
        assert!(reqwest::NoProxy::from_string(bypass).is_some());
    }
}
