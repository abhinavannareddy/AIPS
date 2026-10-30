//! Authentication and role-based access (NFR-07, CMD-04).
//!
//! Production: Keycloak-issued RS256 JWTs validated against the realm JWKS (issuer, audience,
//! expiry). Development: an explicit `AIPS_AUTH_MODE=dev` header-based mode that refuses to
//! start when `AIPS_ENV=production`.
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub mod roles {
    pub const ENGINEER: &str = "engineer";
    pub const MCC: &str = "mcc_controller";
    pub const OPS: &str = "flight_ops";
    pub const SAFETY: &str = "safety_manager";
    pub const CABIN_CREW: &str = "cabin_crew";
    pub const LESSOR: &str = "lessor";
    pub const ADMIN: &str = "admin";
    pub const ALL: [&str; 7] = [ENGINEER, MCC, OPS, SAFETY, CABIN_CREW, LESSOR, ADMIN];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub subject: String,
    pub name: String,
    pub roles: Vec<String>,
    pub org: Option<String>,
    pub auth_mode: String,
}

impl Principal {
    /// Attribution for audit records and human decisions (P7): always the unique subject, with
    /// the display name for readability. The name claim alone is neither unique nor fixed.
    pub fn audit_actor(&self) -> String {
        if self.name == self.subject {
            self.subject.clone()
        } else {
            format!("{} <{}>", self.name, self.subject)
        }
    }
    pub fn has(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role || r == roles::ADMIN)
    }
    pub fn any(&self, wanted: &[&str]) -> bool {
        wanted.iter().any(|r| self.has(r))
    }
    pub fn require(&self, wanted: &[&str]) -> Result<(), ApiError> {
        if self.any(wanted) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!(
                "requires one of roles: {}",
                wanted.join(", ")
            )))
        }
    }
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self {
            status,
            message: msg.into(),
        }
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, msg)
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            axum::Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    realm_access: Option<RealmAccess>,
    #[serde(default)]
    org: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RealmAccess {
    #[serde(default)]
    roles: Vec<String>,
}

enum Mode {
    Keycloak {
        issuer: String,
        /// Realm URL used to fetch the JWKS; differs from `issuer` when the public hostname in
        /// tokens is not reachable from the service (e.g. docker compose).
        jwks_base: String,
        audience: String,
        jwks: RwLock<(JwkSet, Instant)>,
        http: reqwest::Client,
    },
    Dev,
}

pub struct Authenticator {
    mode: Mode,
}

/// The only AIPS_ENV values under which header-based dev authentication may run.
const DEV_AUTH_ENVS: [&str; 4] = ["development", "local", "test", "ci"];

impl Authenticator {
    pub async fn from_env() -> anyhow::Result<Arc<Self>> {
        let mode = std::env::var("AIPS_AUTH_MODE").unwrap_or_else(|_| "keycloak".into());
        let env = std::env::var("AIPS_ENV").unwrap_or_else(|_| "development".into());
        if mode == "dev" {
            // Allow-list, not deny-list: "prod", "Production" or "staging" must not enable dev auth.
            anyhow::ensure!(
                DEV_AUTH_ENVS.contains(&env.as_str()),
                "AIPS_AUTH_MODE=dev is only allowed when AIPS_ENV is one of {DEV_AUTH_ENVS:?} (got {env:?})"
            );
            tracing::warn!(
                "authentication in DEV mode: role headers accepted; never use in production"
            );
            return Ok(Arc::new(Self { mode: Mode::Dev }));
        }
        let issuer = std::env::var("AIPS_OIDC_ISSUER")
            .unwrap_or_else(|_| "http://keycloak:8080/realms/aips".into());
        let jwks_base = std::env::var("AIPS_OIDC_JWKS_BASE").unwrap_or_else(|_| issuer.clone());
        let audience = std::env::var("AIPS_OIDC_AUDIENCE").unwrap_or_else(|_| "aips-api".into());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        let jwks = fetch_jwks(&http, &jwks_base).await.unwrap_or_else(|e| {
            tracing::error!(error = %e, "could not fetch JWKS at startup; will retry on demand");
            JwkSet { keys: vec![] }
        });
        Ok(Arc::new(Self {
            mode: Mode::Keycloak {
                issuer,
                jwks_base,
                audience,
                jwks: RwLock::new((jwks, Instant::now())),
                http,
            },
        }))
    }

    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Dev => "dev",
            Mode::Keycloak { .. } => "keycloak",
        }
    }

    pub async fn authenticate(
        &self,
        headers: &HeaderMap,
        query: Option<&str>,
    ) -> Result<Principal, ApiError> {
        let q = |key: &str| -> Option<String> {
            query?.split('&').find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                (k == key).then(|| v.replace("%2C", ",").replace("%20", " "))
            })
        };
        match &self.mode {
            Mode::Dev => {
                let role_hdr = headers
                    .get("x-aips-role")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
                    .or_else(|| q("role"))
                    .ok_or_else(|| {
                        ApiError::new(StatusCode::UNAUTHORIZED, "missing x-aips-role (dev mode)")
                    })?;
                let user = headers
                    .get("x-aips-user")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
                    .or_else(|| q("user"))
                    .unwrap_or_else(|| "dev.user".into());
                let roles: Vec<String> = role_hdr
                    .split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| roles::ALL.contains(&r.as_str()))
                    .collect();
                if roles.is_empty() {
                    return Err(ApiError::new(StatusCode::UNAUTHORIZED, "unknown role"));
                }
                let org = headers
                    .get("x-aips-org")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                Ok(Principal {
                    subject: user.clone(),
                    name: user,
                    roles,
                    org,
                    auth_mode: "dev".into(),
                })
            }
            Mode::Keycloak {
                issuer,
                jwks_base,
                audience,
                jwks,
                http,
            } => {
                let token = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("Bearer "))
                    .map(str::to_string)
                    .or_else(|| q("access_token"))
                    .ok_or_else(|| {
                        ApiError::new(StatusCode::UNAUTHORIZED, "missing bearer token")
                    })?;
                let header = decode_header(&token)
                    .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "malformed token"))?;
                let kid = header
                    .kid
                    .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "token has no kid"))?;
                let mut key = jwks.read().0.find(&kid).cloned();
                if key.is_none() {
                    let stale = jwks.read().1.elapsed() > Duration::from_secs(60)
                        || jwks.read().0.keys.is_empty();
                    if stale {
                        if let Ok(fresh) = fetch_jwks(http, jwks_base).await {
                            *jwks.write() = (fresh, Instant::now());
                        }
                    }
                    key = jwks.read().0.find(&kid).cloned();
                }
                let jwk = key.ok_or_else(|| {
                    ApiError::new(StatusCode::UNAUTHORIZED, "unknown signing key")
                })?;
                let dk = DecodingKey::from_jwk(&jwk)
                    .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "unsupported key"))?;
                let mut v = Validation::new(Algorithm::RS256);
                v.set_issuer(&[issuer.as_str()]);
                v.set_audience(&[audience.as_str()]);
                let data = decode::<Claims>(&token, &dk, &v).map_err(|e| {
                    ApiError::new(StatusCode::UNAUTHORIZED, format!("invalid token: {e}"))
                })?;
                let c = data.claims;
                let roles = c
                    .realm_access
                    .map(|r| {
                        r.roles
                            .into_iter()
                            .filter(|x| roles::ALL.contains(&x.as_str()))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(Principal {
                    name: c
                        .name
                        .or(c.preferred_username)
                        .unwrap_or_else(|| c.sub.clone()),
                    subject: c.sub,
                    roles,
                    org: c.org,
                    auth_mode: "keycloak".into(),
                })
            }
        }
    }
}

async fn fetch_jwks(http: &reqwest::Client, issuer: &str) -> anyhow::Result<JwkSet> {
    let url = format!("{issuer}/protocol/openid-connect/certs");
    Ok(http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<JwkSet>()
        .await?)
}

/// Axum middleware: authenticate and attach the Principal as a request extension.
pub async fn require_auth(
    State(auth): State<Arc<Authenticator>>,
    mut req: Request,
    next: Next,
) -> Response {
    let query = req.uri().query().map(str::to_string);
    match auth.authenticate(req.headers(), query.as_deref()).await {
        Ok(p) => {
            req.extensions_mut().insert(p);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}
