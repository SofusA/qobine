use crate::AppState;
use axum::{
    Form, Router,
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth", get(index))
        .route("/auth/login", post(login))
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    request: Request,
    next: axum::middleware::Next,
) -> (CookieJar, Response<Body>) {
    let Some(state_secret) = state.web_secret.clone() else {
        return (jar, next.run(request).await);
    };

    let redirect_response = (
        StatusCode::FOUND,
        [
            (
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static(mime::TEXT_HTML_UTF_8.as_ref()),
            ),
            (
                axum::http::header::LOCATION,
                axum::http::HeaderValue::from_static("/auth"),
            ),
        ],
    )
        .into_response();

    let secret = jar.get("secret");

    match secret {
        Some(result_secret) => {
            if state_secret != result_secret.value() {
                return (jar, redirect_response);
            }
        }
        None => return (jar, redirect_response),
    }

    (set_auth_cookie(jar, state_secret), next.run(request).await)
}

fn set_auth_cookie(jar: CookieJar, secret: String) -> CookieJar {
    let mut cookie = Cookie::new("secret", secret);
    cookie.set_same_site(SameSite::Strict);
    cookie.set_path("/");
    cookie.set_max_age(time::Duration::weeks(1));
    jar.add(cookie)
}

async fn index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.render("unauthorized-page.html", &json!({}))
}

#[derive(Deserialize)]
struct LoginParameters {
    secret: String,
}

async fn login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(parameters): Form<LoginParameters>,
) -> (CookieJar, Response<Body>) {
    let response = (
        StatusCode::FOUND,
        [
            (
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static(mime::TEXT_HTML_UTF_8.as_ref()),
            ),
            (
                axum::http::header::LOCATION,
                axum::http::HeaderValue::from_static("/"),
            ),
        ],
        "Success",
    )
        .into_response();

    match state.web_secret.clone() {
        None => return (jar, response),
        Some(secret) => {
            if secret == parameters.secret {
                return (set_auth_cookie(jar, secret), response);
            };
        }
    }

    let response = (StatusCode::UNAUTHORIZED, "Bad credentials").into_response();
    (jar, response)
}
