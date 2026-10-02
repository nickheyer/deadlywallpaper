//! The one HTTP client configuration for everything that talks to the web: Steam's
//! Workshop pages and image CDN, and album art URLs from media players.

use std::time::Duration;
use ureq::tls::{TlsConfig, TlsProvider};

/// An agent that identifies as this application and speaks TLS through the platform's
/// native library; `timeout` bounds every request as a whole.
pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(format!(
            "{}/{}",
            crate::paths::APP_ID,
            env!("CARGO_PKG_VERSION")
        ))
        .http_status_as_error(false)
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .build(),
        )
        .build()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_uses_the_native_tls_provider() {
        let agent = agent(Duration::from_secs(5));
        assert_eq!(
            agent.config().tls_config().provider(),
            TlsProvider::NativeTls
        );
        assert_eq!(
            agent.config().timeouts().global,
            Some(Duration::from_secs(5))
        );
    }
}
