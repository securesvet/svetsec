use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, IssuerUrl, Nonce, PkceCodeVerifier, RedirectUrl,
    TokenResponse,
    core::{CoreClient, CoreProviderMetadata},
};
use sha2::{Digest, Sha256};

const TELEGRAM_ISSUER: &str = "https://oauth.telegram.org";

#[derive(Clone)]
pub struct TelegramAuth {
    client_id: String,
    client_secret: String,
    redirect_url: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TelegramProfile {
    pub id: String,
    pub username: Option<String>,
    pub verified_username: Option<String>,
    pub picture: Option<String>,
}

impl TelegramAuth {
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_url: String,
    ) -> anyhow::Result<Self> {
        if client_id.trim().is_empty() || client_secret.trim().is_empty() {
            anyhow::bail!("Telegram client ID and secret cannot be empty");
        }
        let redirect = url::Url::parse(&redirect_url)?;
        if !matches!(redirect.scheme(), "https" | "http") {
            anyhow::bail!("Telegram redirect URL must use HTTP or HTTPS");
        }
        Ok(Self {
            client_id,
            client_secret,
            redirect_url,
        })
    }

    #[must_use]
    pub fn authorization_url(&self, state: &str, nonce: &str, code_challenge: &str) -> String {
        let mut url = url::Url::parse("https://oauth.telegram.org/auth")
            .expect("Telegram authorization endpoint must be a valid URL");
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", &self.redirect_url)
            .append_pair("response_type", "code")
            .append_pair("scope", "openid profile")
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
        url.into()
    }

    pub async fn exchange(
        &self,
        code: String,
        code_verifier: String,
        nonce: String,
    ) -> anyhow::Result<TelegramProfile> {
        let http_client = openidconnect::reqwest::ClientBuilder::new()
            .redirect(openidconnect::reqwest::redirect::Policy::none())
            .build()?;
        let metadata = CoreProviderMetadata::discover_async(
            IssuerUrl::new(TELEGRAM_ISSUER.to_owned())?,
            &http_client,
        )
        .await?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.client_id.clone()),
            Some(ClientSecret::new(self.client_secret.clone())),
        )
        .set_redirect_uri(RedirectUrl::new(self.redirect_url.clone())?);
        let token = client
            .exchange_code(AuthorizationCode::new(code))?
            .set_pkce_verifier(PkceCodeVerifier::new(code_verifier))
            .request_async(&http_client)
            .await?;
        let id_token = token.id_token().ok_or_else(|| {
            anyhow::anyhow!("Telegram token response did not include an ID token")
        })?;
        let claims = id_token.claims(&client.id_token_verifier(), &Nonce::new(nonce))?;
        let picture = claims
            .picture()
            .and_then(|picture| picture.get(None))
            .map(|picture| picture.as_str().to_owned())
            .filter(|url| url.starts_with("https://"));
        let verified_username = claims
            .preferred_username()
            .map(|username| username.as_str().to_owned());
        let username = verified_username.clone().or_else(|| {
            claims
                .name()
                .and_then(|name| name.get(None))
                .map(|name| name.as_str().to_owned())
        });
        Ok(TelegramProfile {
            id: claims.subject().as_str().to_owned(),
            username,
            verified_username,
            picture,
        })
    }
}

#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::pkce_challenge;

    #[test]
    fn pkce_uses_url_safe_sha256_without_padding() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
