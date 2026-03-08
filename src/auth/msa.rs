//! Microsoft OAuth2 Authorization Code Flow.
//!
//! Opens the user's browser to Microsoft's login page. A temporary localhost
//! HTTP server catches the redirect and extracts the authorization code, which
//! is then exchanged for an access token.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use serde::Deserialize;
use tracing::{debug, info};

const AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";

/// Tokens received from a successful Microsoft OAuth2 exchange.
#[derive(Debug, Clone)]
pub struct MsaTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    #[allow(dead_code)]
    token_type: String,
}

#[derive(Deserialize)]
struct TokenErrorResponse {
    error: String,
    error_description: Option<String>,
}

/// Run the full Authorization Code Flow:
/// 1. Start a localhost HTTP server on a random port
/// 2. Open the user's browser to Microsoft's login page
/// 3. Wait for the redirect with the auth code
/// 4. Exchange the code for tokens
pub async fn login(client_id: &str, http: &reqwest::Client) -> Result<MsaTokens> {
    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://localhost:{port}");

    info!("OAuth2 callback server listening on {redirect_uri}");

    // Build the authorization URL
    let auth_url = format!(
        "{AUTHORIZE_URL}?client_id={client_id}&response_type=code\
         &redirect_uri={redirect_uri}&scope={scope}&prompt=select_account",
        scope = urlencoded(SCOPE),
        redirect_uri = urlencoded(&redirect_uri),
    );

    // Open the browser
    info!("Opening browser for Microsoft login...");
    open::that(&auth_url)?;

    // Wait for the redirect (blocking, so we run on a blocking thread)
    let code = {
        let redirect_uri_clone = redirect_uri.clone();
        tokio::task::spawn_blocking(move || wait_for_code(listener, &redirect_uri_clone)).await??
    };

    debug!("Received authorization code");

    // Exchange the code for tokens
    exchange_code(http, client_id, &code, &redirect_uri).await
}

/// Refresh an existing MSA token using the refresh token.
pub async fn refresh(
    client_id: &str,
    refresh_token: &str,
    http: &reqwest::Client,
) -> Result<MsaTokens> {
    info!("Refreshing Microsoft access token...");

    let params = [
        ("client_id", client_id),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
        ("scope", SCOPE),
    ];

    let resp = http.post(TOKEN_URL).form(&params).send().await?;

    parse_token_response(resp).await
}

/// Wait for the browser redirect to our localhost server and extract the auth code.
fn wait_for_code(listener: TcpListener, _redirect_uri: &str) -> Result<String> {
    // Accept exactly one connection
    let (mut stream, _) = listener.accept()?;
    let mut reader = BufReader::new(stream.try_clone()?);

    // Read the HTTP request line
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    // Parse the code from the URL: GET /?code=...&... HTTP/1.1
    let code = request_line
        .split_whitespace()
        .nth(1) // The path
        .and_then(|path| {
            let parsed: url::Url = url::Url::parse(&format!("http://localhost{path}")).ok()?;
            parsed
                .query_pairs()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| v.to_string())
        })
        .ok_or_else(|| eyre!("No authorization code found in redirect URL: {request_line}"))?;

    // Send a nice response to the browser
    let body = "<!DOCTYPE html><html><body>\
        <h1>Login successful!</h1>\
        <p>You can close this tab and return to MUI.</p>\
        </body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;

    Ok(code)
}

/// Exchange an authorization code for access + refresh tokens.
async fn exchange_code(
    http: &reqwest::Client,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<MsaTokens> {
    let params = [
        ("client_id", client_id),
        ("code", code),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPE),
    ];

    let resp = http.post(TOKEN_URL).form(&params).send().await?;

    parse_token_response(resp).await
}

async fn parse_token_response(resp: reqwest::Response) -> Result<MsaTokens> {
    let status = resp.status();
    let body = resp.text().await?;

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<TokenErrorResponse>(&body) {
            return Err(eyre!(
                "MSA token error: {} - {}",
                err.error,
                err.error_description.unwrap_or_default()
            ));
        }
        return Err(eyre!("MSA token request failed ({}): {}", status, body));
    }

    let tokens: TokenResponse = serde_json::from_str(&body)?;
    Ok(MsaTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.unwrap_or_default(),
        expires_in: tokens.expires_in,
    })
}

/// Percent-encoding for URL query parameters.
fn urlencoded(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
