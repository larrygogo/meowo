//! GitHub 企业版：复用 github.com 的内置实现（forge/builtin.rs），基址换成 `https://<主机>/api/v3` 与
//! `/api/graphql`（GitHub 文档「Getting started with the REST API」/「Forming calls with GraphQL」，Enterprise Server 版），
//! 令牌是用户添加账号时给的个人访问令牌（经典令牌要 `repo` 权限）。HTTP 走多主机的 forge/http.rs，只发往这台主机。

use super::api::GhForge;
use super::builtin::Endpoints;
use super::http::{Auth, ForgeHttp, Req};
use super::GhState;
use crate::github::{Http, Request, Response, Token, GRAPHQL_ACCEPT};
use std::path::PathBuf;

/// 把内置实现发出的 GitHub 请求转给多主机 HTTP：带上令牌与 GitHub 的 Accept 头。
pub(crate) struct GheHttp<'a> {
    pub(crate) http: Box<dyn ForgeHttp + 'a>,
}

impl Http for GheHttp<'_> {
    fn send(&self, req: &Request) -> Result<Response, String> {
        let auth = req.token.map(|t| Auth::Bearer(t.clone()));
        let accept = if req.url.ends_with("/api/graphql") {
            GRAPHQL_ACCEPT
        } else {
            "application/vnd.github+json"
        };
        self.http.send(&Req {
            method: req.method,
            url: req.url,
            auth: auth.as_ref(),
            body: req.body.clone(),
            accept: Some(accept),
        })
    }
}

/// 企业版主机上的平台接口：内置实现 + 这台主机的端点 + 账号令牌。
pub(crate) fn forge<'a>(
    http: Box<dyn ForgeHttp + 'a>,
    host: &str,
    token: Option<Token>,
    state: GhState,
    prefs: PathBuf,
    on_auth: Box<dyn Fn(bool) + Sync + 'a>,
) -> GhForge<'a> {
    super::builtin::forge(Box::new(GheHttp { http }), Endpoints::enterprise(host), token, state, prefs, on_auth)
}

/// 令牌好不好用、是谁：`GET https://<主机>/api/v3/user`。
pub(crate) fn current_user(http: &dyn ForgeHttp, host: &str, token: &Token) -> Result<(String, Option<String>), String> {
    let auth = Auth::Bearer(token.clone());
    let url = format!("https://{host}/api/v3/user");
    let resp = http
        .send(&Req {
            method: "GET",
            url: &url,
            auth: Some(&auth),
            body: None,
            accept: Some("application/vnd.github+json"),
        })
        .map_err(|e| super::http::network_error(&e))?;
    if !super::http::ok(&resp) {
        return Err(super::http::http_error(&resp));
    }
    let v = super::http::json(&resp.body);
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string);
    Ok((s("login").ok_or("gh/failed: no login")?, s("name")))
}
