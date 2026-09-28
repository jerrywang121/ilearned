use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, ModifyCommand, SearchQuery, TopicQuery,
};
use crate::domain::experience::Experience;
use crate::error::AppError;
use crate::storage::embeddings::VectorStore;
use crate::storage::repository::ExperienceRepo;
use crate::surfaces::http::rest::Shared;

fn err_page(status: StatusCode, msg: &str) -> Response {
    let t = ErrorTemplate {
        title: "ilearned",
        status: status.as_u16(),
        message: msg,
    };
    (status, Html(t.render().unwrap())).into_response()
}

/// Boxed response error so handler `Err` variants stay small (clippy::result_large_err).
pub struct WebErr(pub Box<Response>);

impl IntoResponse for WebErr {
    fn into_response(self) -> Response {
        *self.0
    }
}

fn tpl_err(e: askama::Error) -> WebErr {
    WebErr(Box::new(err_page(
        StatusCode::INTERNAL_SERVER_ERROR,
        &format!("template error: {e}"),
    )))
}

fn svc_err(e: AppError) -> Response {
    let status = match &e {
        AppError::InvalidInput(_) | AppError::InvalidFtsSyntax(_) => StatusCode::BAD_REQUEST,
        AppError::NotFound { .. } => StatusCode::NOT_FOUND,
        AppError::EmbeddingUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        AppError::Storage(_) | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    err_page(status, &e.to_string())
}

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>{{ title }}</title>
<style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}ul{list-style:none;padding:0}li{margin:.6em 0;border-bottom:1px solid #ddd;padding-bottom:.4em}.meta{color:#666;font-size:.9em}</style>
</head><body>
<h1>{{ title }}</h1>
{% block content %}{% endblock %}
</body></html>"#,
    ext = "html"
)]
struct BaseTemplate {
    title: String,
}

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ilearned</title>
<style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}ul{list-style:none;padding:0}li{margin:.6em 0;border-bottom:1px solid #ddd;padding-bottom:.4em}.meta{color:#666;font-size:.9em}</style>
</head><body>
<h1>ilearned</h1>
<form method="get" action="/">
<input name="q" value="{{ query }}" placeholder="search text">
<input name="topic" value="{{ topic }}" placeholder="topic">
<label><input type="checkbox" name="deep" value="true" {% if deep %}checked{% endif %}> deep</label>
<button type="submit">Search</button>
</form>
<p class="meta">{{ results.len() }} result(s) <a href="/experiences/new">add</a> · <a href="/topics">topics</a> · <a href="/clear">clear</a></p>
<ul>
{% for e in results %}
<li><a href="/experiences/{{ e.topic }}/{{ e.id }}">{{ e.when_text }} — {{ e.check_text }}</a>
<span class="meta">{{ e.topic }} · good {{ e.good_count }} · bad {{ e.bad_count }} · {{ e.state_str() }}</span></li>
{% endfor %}
</ul>
</body></html>"#,
    ext = "html"
)]
struct SearchTemplate {
    query: String,
    topic: String,
    deep: bool,
    results: Vec<Experience>,
}

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ilearned · record</title>
<style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}.meta{color:#666}</style>
</head><body>
<h1>ilearned</h1>
<h2>{{ e.topic }} / {{ e.id }}</h2>
<p class="meta">{{ e.state_str() }} · good {{ e.good_count }} · bad {{ e.bad_count }}</p>
<dl>
<dt>when</dt><dd>{{ e.when_text }}</dd>
<dt>if</dt><dd>{{ e.if_text }}</dd>
<dt>do</dt><dd>{{ e.do_text }}</dd>
<dt>check</dt><dd>{{ e.check_text }}</dd>
</dl>
<p>
<a href="/experiences/{{ e.topic }}/{{ e.id }}/edit">edit</a>
<form method="post" action="/experiences/{{ e.topic }}/{{ e.id }}/promote" style="display:inline"><button>promote</button></form>
<form method="post" action="/experiences/{{ e.topic }}/{{ e.id }}/downgrade" style="display:inline"><button>downgrade</button></form>
<form method="post" action="/experiences/{{ e.topic }}/{{ e.id }}/delete" style="display:inline"><input type="hidden" name="confirm" value="yes"><button>delete</button></form>
</p>
<p><a href="/">back</a></p>
</body></html>"#,
    ext = "html"
)]
struct DetailTemplate {
    e: Experience,
}

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ilearned · add</title>
<style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}label{display:block;margin:.5em 0}</style>
</head><body>
<h1>ilearned · add</h1>
<form method="post" action="/experiences">
<label>topic <input name="topic" required></label>
<label>when <input name="when_text" required></label>
<label>if <input name="if_text" required></label>
<label>do <input name="do_text" required></label>
<label>check <input name="check" required></label>
<button type="submit">Add</button>
</form>
<p><a href="/">back</a></p>
</body></html>"#,
    ext = "html"
)]
struct AddTemplate;

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ilearned · edit</title>
<style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}label{display:block;margin:.5em 0}</style>
</head><body>
<h1>ilearned · edit {{ e.topic }}/{{ e.id }}</h1>
<form method="post" action="/experiences/{{ e.topic }}/{{ e.id }}">
<label>when <input name="when_text" value="{{ e.when_text }}"></label>
<label>if <input name="if_text" value="{{ e.if_text }}"></label>
<label>do <input name="do_text" value="{{ e.do_text }}"></label>
<label>check <input name="check" value="{{ e.check_text }}"></label>
<button type="submit">Save</button>
</form>
<p><a href="/experiences/{{ e.topic }}/{{ e.id }}">back</a></p>
</body></html>"#,
    ext = "html"
)]
struct EditTemplate {
    e: Experience,
}

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ilearned · clear</title>
<style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}</style>
</head><body>
<h1>ilearned · clear</h1>
<form method="post" action="/clear">
<label>topic <input name="topic"></label>
<p>or</p>
<label><input type="checkbox" name="all" value="true"> all topics</label>
<label>destructive — type yes to confirm <input name="confirm" required></label>
<button type="submit">Clear</button>
</form>
<p><a href="/">back</a></p>
</body></html>"#,
    ext = "html"
)]
struct ClearTemplate;

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ilearned · error {{ status }}</title></head><body>
<h1>error {{ status }}</h1>
<p>{{ message }}</p>
<p><a href="/">back</a></p>
</body></html>"#,
    ext = "html"
)]
struct ErrorTemplate<'a> {
    #[allow(dead_code)]
    title: &'a str,
    status: u16,
    message: &'a str,
}

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ilearned · topics</title>
<style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}ul{list-style:none;padding:0}li{margin:.6em 0;border-bottom:1px solid #ddd;padding-bottom:.4em}.meta{color:#666;font-size:.9em}</style>
</head><body>
<h1>ilearned · topics</h1>
<form method="get" action="/topics">
<input name="q" value="{{ q }}" placeholder="search substring or # pattern">
<input name="level" value="{{ level }}" placeholder="level">
<label><input type="checkbox" name="deep" value="true" {% if deep %}checked{% endif %}> deep</label>
<button type="submit">Search</button>
</form>
<p class="meta">{{ results.len() }} topic(s)</p>
<ul>
{% for t in results %}
<li>{{ t }}</li>
{% endfor %}
</ul>
<p><a href="/">back</a></p>
</body></html>"#,
    ext = "html"
)]
struct TopicsTemplate {
    q: String,
    level: String,
    deep: bool,
    results: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct IndexParams {
    pub q: Option<String>,
    pub topic: Option<String>,
    pub deep: Option<bool>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ExperienceForm {
    pub topic: Option<String>,
    #[serde(rename = "when_text")]
    pub when_text: Option<String>,
    #[serde(rename = "if_text")]
    pub if_text: Option<String>,
    #[serde(rename = "do_text")]
    pub do_text: Option<String>,
    pub check: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ConfirmForm {
    pub confirm: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ClearForm {
    pub topic: Option<String>,
    pub all: Option<String>,
    pub confirm: Option<String>,
}

fn req(o: Option<String>, name: &str) -> Result<String, AppError> {
    o.filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::InvalidInput(format!("{name} is required")))
}

pub async fn index<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Query(p): Query<IndexParams>,
) -> Result<Html<String>, WebErr> {
    let results = svc
        .search(&SearchQuery {
            topic: p.topic.clone().filter(|s| !s.trim().is_empty()),
            text: p.q.clone().filter(|s| !s.trim().is_empty()),
            semantic: None,
            limit: p.limit.unwrap_or(20),
            offset: p.offset.unwrap_or(0),
            deep: p.deep.unwrap_or(false),
        })
        .map_err(|e| WebErr(Box::new(svc_err(e))))?;
    let t = SearchTemplate {
        query: p.q.unwrap_or_default(),
        topic: p.topic.unwrap_or_default(),
        deep: p.deep.unwrap_or(false),
        results,
    };
    Ok(Html(t.render().map_err(|e| {
        let WebErr(b) = tpl_err(e);
        WebErr(b)
    })?))
}

async fn detail<R: ExperienceRepo + VectorStore + 'static>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
) -> Result<Html<String>, WebErr> {
    // Reads go through MemoryService (sole entry): reconcile + deleted→404.
    let e = svc
        .get(&topic, &id)
        .map_err(|e| WebErr(Box::new(svc_err(e))))?;
    Ok(Html(DetailTemplate { e }.render().map_err(|e| {
        let WebErr(b) = tpl_err(e);
        WebErr(b)
    })?))
}

async fn add_form() -> Html<String> {
    Html(AddTemplate.render().unwrap())
}

async fn add_submit<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Form(f): Form<ExperienceForm>,
) -> Result<Redirect, WebErr> {
    let cmd = AddCommand {
        topic: req(f.topic, "topic").map_err(|e| WebErr(Box::new(svc_err(e))))?,
        when_text: req(f.when_text, "when").map_err(|e| WebErr(Box::new(svc_err(e))))?,
        if_text: req(f.if_text, "if").map_err(|e| WebErr(Box::new(svc_err(e))))?,
        do_text: req(f.do_text, "do").map_err(|e| WebErr(Box::new(svc_err(e))))?,
        check_text: req(f.check, "check").map_err(|e| WebErr(Box::new(svc_err(e))))?,
    };
    let e = svc.add(cmd).map_err(|e| WebErr(Box::new(svc_err(e))))?;
    Ok(Redirect::to(&format!("/experiences/{}/{}", e.topic, e.id)))
}

async fn edit_form<R: ExperienceRepo + VectorStore + 'static>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
) -> Result<Html<String>, WebErr> {
    // Reads go through MemoryService (sole entry): reconcile + deleted→404.
    let e = svc
        .get(&topic, &id)
        .map_err(|e| WebErr(Box::new(svc_err(e))))?;
    Ok(Html(EditTemplate { e }.render().map_err(|e| {
        let WebErr(b) = tpl_err(e);
        WebErr(b)
    })?))
}

async fn edit_submit<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
    Form(f): Form<ExperienceForm>,
) -> Result<Redirect, WebErr> {
    let clean = |o: Option<String>| o.filter(|s| !s.trim().is_empty());
    let cmd = ModifyCommand {
        topic: topic.clone(),
        id: id.clone(),
        when_text: clean(f.when_text),
        if_text: clean(f.if_text),
        do_text: clean(f.do_text),
        check_text: clean(f.check),
    };
    if !cmd.has_updates() {
        return Err(WebErr(Box::new(err_page(
            StatusCode::BAD_REQUEST,
            "no fields to update",
        ))));
    }
    svc.modify(cmd).map_err(|e| WebErr(Box::new(svc_err(e))))?;
    Ok(Redirect::to(&format!("/experiences/{topic}/{id}")))
}

async fn promote_action<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
) -> Result<Redirect, WebErr> {
    svc.promote(&FeedbackCommand {
        topic: topic.clone(),
        id: id.clone(),
    })
    .map_err(|e| WebErr(Box::new(svc_err(e))))?;
    Ok(Redirect::to(&format!("/experiences/{topic}/{id}")))
}

async fn downgrade_action<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
) -> Result<Redirect, WebErr> {
    svc.downgrade(&FeedbackCommand {
        topic: topic.clone(),
        id: id.clone(),
    })
    .map_err(|e| WebErr(Box::new(svc_err(e))))?;
    Ok(Redirect::to(&format!("/experiences/{topic}/{id}")))
}

async fn delete_action<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Path((topic, id)): Path<(String, String)>,
    Form(f): Form<ConfirmForm>,
) -> Result<Redirect, WebErr> {
    if f.confirm.as_deref() != Some("yes") {
        return Err(WebErr(Box::new(err_page(
            StatusCode::BAD_REQUEST,
            "delete requires confirm=yes",
        ))));
    }
    svc.delete(&topic, &id)
        .map_err(|e| WebErr(Box::new(svc_err(e))))?;
    Ok(Redirect::to("/"))
}

async fn clear_page() -> Html<String> {
    Html(ClearTemplate.render().unwrap())
}

#[derive(Debug, Deserialize, Default)]
pub struct TopicsParams {
    pub q: Option<String>,
    pub level: Option<String>,
    pub deep: Option<bool>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Topic list/search page: `q` substring or `#` pattern, `level` depth
/// truncation, `deep` includes inactive-only topics. Askama auto-escape ON.
async fn topics_page<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Query(p): Query<TopicsParams>,
) -> Result<Html<String>, WebErr> {
    let level: Option<u32> = match p.level.clone().filter(|s| !s.trim().is_empty()) {
        None => None,
        Some(s) => Some(s.parse::<u32>().map_err(|_| {
            WebErr(Box::new(err_page(
                StatusCode::BAD_REQUEST,
                "level must be a positive integer",
            )))
        })?),
    };
    let results = svc
        .list_topics(&TopicQuery {
            query: p.q.clone().filter(|s| !s.trim().is_empty()),
            level,
            limit: p.limit.unwrap_or(20),
            offset: p.offset.unwrap_or(0),
            deep: p.deep.unwrap_or(false),
        })
        .map_err(|e| WebErr(Box::new(svc_err(e))))?;
    let t = TopicsTemplate {
        q: p.q.unwrap_or_default(),
        level: p
            .level
            .unwrap_or_default(),
        deep: p.deep.unwrap_or(false),
        results,
    };
    Ok(Html(t.render().map_err(|e| {
        let WebErr(b) = tpl_err(e);
        WebErr(b)
    })?))
}

async fn clear_submit<R: ExperienceRepo + VectorStore>(
    State(svc): State<Shared<R>>,
    Form(f): Form<ClearForm>,
) -> Result<Redirect, WebErr> {
    if f.confirm.as_deref() != Some("yes") {
        return Err(WebErr(Box::new(err_page(
            StatusCode::BAD_REQUEST,
            "clear requires confirm=yes",
        ))));
    }
    let cmd = match (f.topic.filter(|s| !s.trim().is_empty()), f.all.as_deref()) {
        (Some(t), _) => ClearCommand::Topic(t),
        (None, Some("true")) => ClearCommand::All,
        _ => {
            return Err(WebErr(Box::new(err_page(
                StatusCode::BAD_REQUEST,
                "clear requires a topic or all=true",
            ))));
        }
    };
    svc.clear(&cmd).map_err(|e| WebErr(Box::new(svc_err(e))))?;
    Ok(Redirect::to("/"))
}

pub fn web_routes<R: ExperienceRepo + VectorStore + 'static>() -> Router<Shared<R>> {
    Router::new()
        .route("/", get(index::<R>))
        .route("/experiences/new", get(add_form))
        .route("/experiences", post(add_submit::<R>))
        .route("/experiences/:topic/:id", get(detail::<R>))
        .route("/experiences/:topic/:id/edit", get(edit_form::<R>))
        .route("/experiences/:topic/:id", post(edit_submit::<R>))
        .route("/experiences/:topic/:id/promote", post(promote_action::<R>))
        .route(
            "/experiences/:topic/:id/downgrade",
            post(downgrade_action::<R>),
        )
        .route("/experiences/:topic/:id/delete", post(delete_action::<R>))
        .route("/clear", get(clear_page).post(clear_submit::<R>))
        .route("/topics", get(topics_page::<R>))
}

#[allow(dead_code)]
fn _base_shape() -> BaseTemplate {
    BaseTemplate {
        title: String::new(),
    }
}
