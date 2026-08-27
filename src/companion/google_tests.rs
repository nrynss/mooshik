//! Pins for the Google (Vertex) companion posture.
//!
//! Nothing here reaches Google. The credential fixture is an authorized-user
//! ADC file whose `token_uri` points at an in-process loopback token endpoint,
//! and the completions endpoint is the existing SSE mock — so the refresh
//! behaviour that matters (a token expires in about an hour; a header built
//! once at construction 401s after that) is provable offline.
//!
//! An authorized-user fixture rather than a service-account one on purpose:
//! the `refresh_token` grant carries no key material at all, so this file
//! contains nothing shaped like a private key.

use std::time::Duration;

use crate::config::{vertex_base_url, CompanionAuth, CompanionConfig};

use super::cancel::Cancellation;
use super::client::{chat_completions_url, CompanionClient};
use super::mock::{Frame, MockServer, Script, TokenServer};
use super::types::Message;
use super::CompanionError;

/// The dummy credential file. `client_secret` and `refresh_token` are inert
/// fixture words: this grant type is exactly the one that needs no key.
fn credentials_file(token_uri: &str, label: &str) -> std::path::PathBuf {
    let path = crate::secure_path::canonical_temp_dir().join(format!(
        "mooshik-google-{label}-{}.json",
        std::process::id()
    ));
    let body = serde_json::json!({
        "type": "authorized_user",
        "client_id": "fixture-client-id",
        "client_secret": "fixture-client-secret",
        "refresh_token": "fixture-refresh-token",
        "quota_project_id": "fixture-project",
        "token_uri": token_uri,
    });
    std::fs::write(&path, body.to_string()).unwrap();
    path
}

/// A Google-posture config whose *completions* endpoint is the loopback mock.
///
/// `google_project` is deliberately unset so `resolved_base_url` falls back to
/// `base_url`: these tests are about the token, and one test cannot both talk
/// to the mock and talk to `*-aiplatform.googleapis.com`. The derivation is
/// pinned on its own, above.
fn google_config(base_url: &str, credentials: &std::path::Path) -> CompanionConfig {
    CompanionConfig {
        base_url: base_url.to_owned(),
        auth: CompanionAuth::Google,
        google_project: None,
        google_credentials: Some(credentials.to_path_buf()),
        ..CompanionConfig::default()
    }
}

fn stop_script(text: &str) -> Script {
    Script::sse(vec![
        Frame::content_openai(text),
        Frame::finish("stop"),
        Frame::done(),
    ])
}

/// The Vertex endpoint is a pure function of project and location, so an
/// operator supplies those and never a URL.
#[test]
fn the_vertex_base_url_is_derived_from_project_and_location() {
    assert_eq!(
        vertex_base_url("mooshik", "us-central1"),
        "https://us-central1-aiplatform.googleapis.com/v1beta1/projects/mooshik/locations/us-central1/endpoints/openapi"
    );
    // The region appears twice and must track together.
    assert_eq!(
        vertex_base_url("other-proj", "europe-west4"),
        "https://europe-west4-aiplatform.googleapis.com/v1beta1/projects/other-proj/locations/europe-west4/endpoints/openapi"
    );
    // And it drops straight into the completions path.
    assert_eq!(
        chat_completions_url(&vertex_base_url("mooshik", "us-central1")),
        "https://us-central1-aiplatform.googleapis.com/v1beta1/projects/mooshik/locations/us-central1/endpoints/openapi/chat/completions"
    );
}

#[test]
fn the_google_posture_derives_its_endpoint_and_the_local_default_is_untouched() {
    let local = CompanionConfig::default();
    assert_eq!(local.auth, CompanionAuth::Static);
    assert_eq!(local.resolved_base_url(), "http://127.0.0.1:8080/v1");

    let google = CompanionConfig {
        auth: CompanionAuth::Google,
        google_project: Some("mooshik".to_owned()),
        // A stale hand-pasted URL must not win over the derivation.
        base_url: "http://127.0.0.1:8080/v1".to_owned(),
        ..CompanionConfig::default()
    };
    assert_eq!(google.resolved_google_location(), "us-central1");
    assert_eq!(
        google.resolved_base_url(),
        vertex_base_url("mooshik", "us-central1")
    );
    let with_region = CompanionConfig {
        google_location: Some("europe-west4".to_owned()),
        ..google
    };
    assert_eq!(
        with_region.resolved_base_url(),
        vertex_base_url("mooshik", "europe-west4")
    );
}

/// A Google access token expires in about an hour. The old client turned
/// `api_key` into a header once at construction, so the Google path would work
/// for one hour and then 401 — on a companion whose premise is running beside
/// you all day.
///
/// The token endpoint hands out a token that expires almost immediately, then
/// a second one. Two completions across that boundary must carry two DIFFERENT
/// bearer tokens, which is only true if the client asks for a token per
/// request instead of memoizing one.
#[tokio::test]
async fn an_expired_token_is_refreshed_rather_than_reused() {
    // `expires_in` is reduced by a 60s safety margin and floored at 1s, so 60
    // is the shortest cache lambo will keep.
    let tokens = TokenServer::spawn(vec![
        (
            200,
            r#"{"access_token":"token-first","expires_in":60}"#.to_owned(),
        ),
        (
            200,
            r#"{"access_token":"token-second","expires_in":3600}"#.to_owned(),
        ),
    ])
    .await;
    let credentials = credentials_file(&tokens.url, "refresh");
    let server = MockServer::spawn(vec![stop_script("one"), stop_script("two")]).await;
    let client =
        CompanionClient::from_config(&google_config(&server.base_url, &credentials)).unwrap();

    client
        .complete(&[Message::user("hi")], &[], &Cancellation::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(tokens.mint_count(), 1);
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    client
        .complete(&[Message::user("again")], &[], &Cancellation::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(
        tokens.mint_count(),
        2,
        "the expired token must be re-minted"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer token-first")
    );
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer token-second"),
        "the second request must carry the REFRESHED token, not the expired one"
    );
    let _ = std::fs::remove_file(credentials);
}

/// The equivalent of `api_key_never_appears_in_client_errors`, for the minted
/// token.
///
/// Lambo formats the token endpoint's response body into
/// `GoogleAuthError::Backend`, and a token response body is not something this
/// crate gets to promise is free of credential material — so the mapping drops
/// the message and the terminal sees `en.toml` only.
#[tokio::test]
async fn a_minted_token_never_appears_in_client_errors() {
    let marker = "s3cret-minted-token-value";
    // A refusal whose body echoes the credential — the shape that would leak
    // if the mapping passed lambo's message through.
    let tokens = TokenServer::spawn(vec![(
        400,
        format!(r#"{{"error":"invalid_grant","access_token":"{marker}"}}"#),
    )])
    .await;
    let credentials = credentials_file(&tokens.url, "leak");
    let server = MockServer::spawn(vec![stop_script("unused")]).await;
    let client =
        CompanionClient::from_config(&google_config(&server.base_url, &credentials)).unwrap();
    let error = client
        .complete(&[Message::user("hi")], &[], &Cancellation::new(), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(error, CompanionError::AuthRefused), "{error:?}");
    assert!(!error.to_string().contains(marker), "{error}");
    assert!(!format!("{error:?}").contains(marker));
    assert_eq!(
        error.to_string(),
        crate::text::get("companion.auth_refused")
    );
    // Nothing was sent to the model, so no request can carry it either.
    assert!(server.requests().is_empty());
    let _ = std::fs::remove_file(credentials);
}

/// A token endpoint that cannot be reached is transient; a refusal is not.
/// Both are the operator's to fix, and both exit 2.
#[tokio::test]
async fn google_auth_failures_keep_the_transient_and_permanent_split() {
    let credentials =
        crate::secure_path::canonical_temp_dir().join("mooshik-google-absent-credentials.json");
    let _ = std::fs::remove_file(&credentials);
    let built = CompanionClient::from_config(&CompanionConfig {
        auth: CompanionAuth::Google,
        google_project: Some("proj".to_owned()),
        google_credentials: Some(credentials),
        ..CompanionConfig::default()
    });
    let Err(error) = built else {
        panic!("an unreadable credential file must not build a client");
    };
    assert!(
        matches!(error, CompanionError::AuthUnavailable),
        "{error:?}"
    );
    assert!(error.to_string().contains("google_credentials"));

    // Both classify as user error: the operator supplies the credential.
    for error in [CompanionError::AuthUnavailable, CompanionError::AuthRefused] {
        let failure = crate::cli::Failure::from(anyhow::Error::new(error));
        assert_eq!(failure.exit_code(), 2);
    }
}

/// The static path is untouched: a local endpoint with no key still sends no
/// Authorization header, and a configured key still sends exactly one.
#[tokio::test]
async fn the_static_posture_still_works_for_local_and_generic_endpoints() {
    let server = MockServer::spawn(vec![stop_script("ok")]).await;
    let client = CompanionClient::from_config(&CompanionConfig {
        base_url: server.base_url.clone(),
        ..CompanionConfig::default()
    })
    .unwrap();
    client
        .complete(&[Message::user("hi")], &[], &Cancellation::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(server.requests()[0].authorization, None);
    server.assert_all_streaming();
}
