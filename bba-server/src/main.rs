//! BBA Server — Bridge Bidding Analyzer API
//!
//! Rust port of the ASP.NET bba-server, using Axum + epbot-core.
//! Generates bridge auctions for the BBOAlert browser extension.

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

mod config;
mod models;
mod routes;
mod services;

use config::Config;
use services::audit_log::AuditLogService;
use services::convention_service::ConventionService;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub convention_service: Arc<ConventionService>,
    pub audit_log: Arc<AuditLogService>,
    pub semaphore: Arc<Semaphore>,
    pub epbot_version: i32,
}

#[tokio::main]
async fn main() {
    // Load .env if present
    let _ = dotenvy::dotenv();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let config = Config::from_env();

    // Get EPBot version
    let epbot_version = match epbot_core::version() {
        Ok(v) => {
            info!("EPBot version: {}", v);
            v
        }
        Err(e) => {
            warn!("Failed to get EPBot version: {}. Auction generation may fail.", e);
            0
        }
    };

    let convention_service = ConventionService::new(
        &config.github_raw_base_url,
        &config.default_ns_card,
        &config.default_ew_card,
    );

    // From Cargo.toml, not a literal — this value lands in the audit CSV's
    // Version column, and a hardcoded copy silently kept reporting 2.0.2
    // after the crate was bumped to 2.1.0.
    let audit_log = AuditLogService::new(&config.log_path, env!("CARGO_PKG_VERSION"));

    let state = AppState {
        semaphore: Arc::new(Semaphore::new(config.max_concurrency)),
        config: Arc::new(config.clone()),
        convention_service: Arc::new(convention_service),
        audit_log: Arc::new(audit_log),
        epbot_version,
    };

    // CORS — allow BridgeBase.com, Bridge Classroom, and localhost dev servers.
    //
    // The localhost entries are a RANGE (5173-5199 vite dev, 4173-4199 vite
    // preview), not the two default ports. Vite takes 5173 and silently
    // INCREMENTS when it's busy, and there are seven Vite repos in this
    // workspace, none pinning a port — so a second dev server lands on 5174,
    // falls outside a two-port list, and every auction request is CORS-rejected
    // in ~20ms. That reads as "BBA stalls mid-auction" and cost real debugging
    // time twice (2026-07-30). The Bridge-Classroom lane2 worktree sits on 5174
    // permanently and had never worked against this service.
    //
    // Widening is safe: a localhost origin is only reachable from the
    // developer's own machine. Kept as an exact-match LIST rather than a
    // predicate so the production origins keep their strict equality.
    let mut allowed: Vec<HeaderValue> = vec![
        "https://www.bridgebase.com".parse().unwrap(),
        "http://www.bridgebase.com".parse().unwrap(),
        "https://bridgebase.com".parse().unwrap(),
        "https://bridge-classroom.com".parse().unwrap(),
        "https://www.bridge-classroom.com".parse().unwrap(),
        "https://bridge-classroom.org".parse().unwrap(),
        "https://www.bridge-classroom.org".parse().unwrap(),
        "https://game-analysis.bridge-classroom.com".parse().unwrap(),
        "https://game-analysis.bridge-classroom.org".parse().unwrap(),
        "http://localhost:3000".parse().unwrap(),
        "http://localhost:3001".parse().unwrap(),
    ];
    for host in ["localhost", "127.0.0.1"] {
        for port in (5173..=5199).chain(4173..=4199) {
            allowed.push(format!("http://{host}:{port}").parse().unwrap());
        }
    }

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            "content-type".parse().unwrap(),
            "x-client-version".parse().unwrap(),
            "x-client-info".parse().unwrap(),
            "x-api-key".parse().unwrap(),
        ]);

    let app = Router::new()
        // Public endpoints
        .route("/health", get(health))
        .route("/api/auction/generate", post(routes::api::generate_auction))
        .route("/api/scenario/select", post(routes::api::select_scenario))
        .route("/api/scenarios", get(routes::api::list_scenarios))
        // Admin endpoints
        .route("/admin", get(routes::admin::admin_root))
        .route("/admin/dashboard", get(routes::admin::dashboard))
        .route("/admin/whoami", get(routes::admin::whoami))
        .route("/admin/api/logs", get(routes::admin::list_logs))
        .route("/admin/api/logs/:filename", get(routes::admin::get_log))
        .route("/admin/api/stats", get(routes::admin::stats))
        .route("/admin/api/scenario-stats", get(routes::admin::scenario_stats))
        // Middleware
        .layer(middleware::from_fn_with_state(state.clone(), api_key_middleware))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let addr = format!("{}:{}", state.config.host, state.config.port);
    info!("BBA Server v{} starting on {}", env!("CARGO_PKG_VERSION"), addr);
    info!(
        "EPBot version: {}, max concurrency: {}",
        epbot_version, state.config.max_concurrency
    );

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap();
}

/// Health check endpoint.
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// API key validation middleware.
async fn api_key_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let path = request.uri().path();

    // Skip API key check for health, admin, and scenarios
    if path.starts_with("/health")
        || path.starts_with("/admin")
    {
        return next.run(request).await;
    }

    // Check API key if configured
    if !state.config.api_key.is_empty() {
        let provided_key = headers
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            .or_else(|| {
                request
                    .uri()
                    .query()
                    .and_then(|q| {
                        q.split('&')
                            .find(|p| p.starts_with("apiKey="))
                            .map(|p| &p[7..])
                    })
            });

        if provided_key != Some(&state.config.api_key) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "success": false,
                    "error": "Invalid or missing API key"
                })),
            )
                .into_response();
        }
    }

    next.run(request).await
}
