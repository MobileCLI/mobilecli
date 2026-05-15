use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub const AUTH_VERSION: u8 = 2;
const TOKEN_BYTES: usize = 32;
const VERIFIER_DOMAIN: &[u8] = b"mobilecli-auth-v2-verifier\0";
const TRANSCRIPT_DOMAIN: &str = "mobilecli-auth-v2";
const LOCAL_PTY_DOMAIN: &str = "mobilecli-local-pty-v1";
const LOCAL_PTY_KEY_DOMAIN: &[u8] = b"mobilecli-local-pty-v1-key\0";

pub const LOCAL_PTY_AUTH_VERSION: u8 = 1;

pub const SCOPE_SESSION_READ: &str = "session:read";
pub const SCOPE_SESSION_CONTROL: &str = "session:control";
pub const SCOPE_SESSION_SPAWN: &str = "session:spawn";
pub const SCOPE_FS_READ: &str = "fs:read";
pub const SCOPE_FS_WRITE: &str = "fs:write";
pub const SCOPE_FS_DELETE: &str = "fs:delete";
pub const SCOPE_FS_WATCH: &str = "fs:watch";
pub const SCOPE_FS_UPLOAD: &str = "fs:upload";
pub const SCOPE_PUSH_REGISTER: &str = "push:register";

pub fn default_scopes() -> Vec<String> {
    [
        SCOPE_SESSION_READ,
        SCOPE_SESSION_CONTROL,
        SCOPE_SESSION_SPAWN,
        SCOPE_FS_READ,
        SCOPE_FS_WRITE,
        SCOPE_FS_DELETE,
        SCOPE_FS_WATCH,
        SCOPE_FS_UPLOAD,
        SCOPE_PUSH_REGISTER,
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCredential {
    pub credential_id: String,
    pub verifier: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

impl AuthCredential {
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }

    pub fn revoke(&mut self) {
        if self.revoked_at.is_none() {
            self.revoked_at = Some(chrono::Utc::now().to_rfc3339());
        }
    }
}

#[derive(Debug, Clone)]
pub struct PairingCredential {
    pub credential: AuthCredential,
    pub auth_token: String,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedClient {
    pub credential_id: String,
    pub mobile_installation_id: String,
    pub sender_id: Option<String>,
    pub client_version: String,
    pub client_capabilities: Option<u32>,
    pub scopes: Vec<String>,
}

impl AuthenticatedClient {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

pub fn generate_pairing_credential(name: impl Into<String>) -> PairingCredential {
    let mut token_bytes = [0u8; TOKEN_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut token_bytes);
    let auth_token = URL_SAFE_NO_PAD.encode(token_bytes);
    let verifier = verifier_from_token(&auth_token);
    let now = chrono::Utc::now().to_rfc3339();
    PairingCredential {
        credential: AuthCredential {
            credential_id: uuid::Uuid::new_v4().to_string(),
            verifier,
            name: name.into(),
            scopes: default_scopes(),
            created_at: now,
            last_used_at: None,
            revoked_at: None,
        },
        auth_token,
    }
}

pub fn verifier_from_token(auth_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(VERIFIER_DOMAIN);
    hasher.update(auth_token.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

pub fn build_auth_transcript(
    server_id: &str,
    credential_id: &str,
    client_nonce: &str,
    server_nonce: &str,
    mobile_installation_id: &str,
) -> String {
    [
        TRANSCRIPT_DOMAIN,
        server_id,
        credential_id,
        client_nonce,
        server_nonce,
        mobile_installation_id,
    ]
    .join("\n")
}

pub fn generate_nonce() -> String {
    let mut nonce = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    URL_SAFE_NO_PAD.encode(nonce)
}

pub fn proof_from_verifier(verifier: &str, transcript: &str) -> Option<String> {
    let key = URL_SAFE_NO_PAD.decode(verifier.as_bytes()).ok()?;
    let mut mac = HmacSha256::new_from_slice(&key).ok()?;
    mac.update(transcript.as_bytes());
    Some(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

pub fn verify_proof(verifier: &str, transcript: &str, proof: &str) -> bool {
    let Some(expected) = proof_from_verifier(verifier, transcript) else {
        return false;
    };
    expected.as_bytes().ct_eq(proof.as_bytes()).into()
}

pub fn build_pty_registration_transcript(
    session_id: &str,
    name: &str,
    command: &str,
    project_path: &str,
    runtime: &str,
    desktop: bool,
) -> String {
    [
        LOCAL_PTY_DOMAIN,
        session_id,
        name,
        command,
        project_path,
        runtime,
        if desktop { "desktop" } else { "headless" },
    ]
    .join("\n")
}

pub fn local_pty_proof_from_token(token: &str, transcript: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(LOCAL_PTY_KEY_DOMAIN);
    hasher.update(token.as_bytes());
    let key = hasher.finalize();
    let mut mac = HmacSha256::new_from_slice(&key).expect("sha256 key is valid hmac key");
    mac.update(transcript.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

pub fn verify_local_pty_proof(token: &str, transcript: &str, proof: &str) -> bool {
    let expected = local_pty_proof_from_token(token, transcript);
    expected.as_bytes().ct_eq(proof.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_credential_verifies_challenge_response() {
        let pairing = generate_pairing_credential("phone");
        let transcript = build_auth_transcript(
            "server",
            &pairing.credential.credential_id,
            "client_nonce",
            "server_nonce",
            "mobile_installation",
        );
        let proof = proof_from_verifier(&pairing.credential.verifier, &transcript).unwrap();

        assert!(verify_proof(
            &pairing.credential.verifier,
            &transcript,
            &proof
        ));
        assert!(!verify_proof(
            &pairing.credential.verifier,
            &transcript,
            "wrong-proof"
        ));
    }

    #[test]
    fn verifier_derivation_is_deterministic_and_token_bound() {
        let token = "sample-token";
        assert_eq!(verifier_from_token(token), verifier_from_token(token));
        assert_ne!(
            verifier_from_token(token),
            verifier_from_token("other-token")
        );
    }

    #[test]
    fn local_pty_proof_is_bound_to_registration_transcript() {
        let token = generate_nonce();
        let transcript = build_pty_registration_transcript(
            "session",
            "name",
            "claude",
            "/tmp/project",
            "pty",
            true,
        );
        let proof = local_pty_proof_from_token(&token, &transcript);
        assert!(verify_local_pty_proof(&token, &transcript, &proof));

        let other_transcript =
            build_pty_registration_transcript("session", "name", "claude", "/", "pty", true);
        assert!(!verify_local_pty_proof(&token, &other_transcript, &proof));
    }
}
