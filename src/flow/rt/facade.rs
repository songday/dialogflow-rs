use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use axum::Json;
use axum::extract::Multipart;
use axum::response::IntoResponse;

use tokio::sync::mpsc::Sender;

use super::dto::Request;
use super::executor;
use crate::ai::dto::Attachment;
use crate::result::Result;
use crate::web::server::to_res2;

static ANSWER_SSE_SESSIONS: LazyLock<Mutex<HashMap<String, Sender<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::with_capacity(128)));

pub(crate) async fn answer(Json(mut req): Json<Request>) -> impl IntoResponse {
    let now = std::time::Instant::now();
    let r = executor::process(&mut req).await;
    // println!("exec used time:{:?}", now.elapsed());
    let res = to_res2(r);
    log::info!("Response used time:{:?}", now.elapsed());
    res
}

/// Like `answer` but accepts image attachments via multipart/form-data.
/// Fields:
/// - `request`: the JSON body, same schema as `/flow/answer`.
/// - `images` (repeatable): binary image files.
pub(crate) async fn answer_multipart(mut multipart: Multipart) -> impl IntoResponse {
    let now = std::time::Instant::now();
    let mut op_req: Option<Request> = None;
    let mut attachments: Vec<Attachment> = Vec::with_capacity(4);
    let r = loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                if field.name().is_some_and(|n| n.eq("request")) {
                    match field.bytes().await {
                        Ok(b) => match serde_json::from_slice::<Request>(b.as_ref()) {
                            Ok(req) => op_req = Some(req),
                            Err(e) => {
                                break Err(crate::result::Error::InvalidJsonStructure(Box::new(e)))
                            }
                        },
                        Err(e) => break Err(e.into()),
                    }
                } else if field.name().is_some_and(|n| n.eq("images")) {
                    match field.bytes().await {
                        Ok(b) => {
                            let mime_type = field
                                .content_type()
                                .map(|t| t.essence_str().to_string())
                                .unwrap_or(String::from("image/jpeg"));
                            let data =
                                base64::engine::general_purpose::STANDARD.encode(b.as_ref());
                            attachments.push(Attachment { mime_type, data });
                        }
                        Err(e) => break Err(e.into()),
                    }
                }
            }
            Ok(None) => break Ok(op_req),
            Err(e) => break Err(e.into()),
        }
    };
    let res = match (r, op_req) {
        (Ok(Some(mut req)), _) => {
            if !attachments.is_empty() {
                req.attachments.extend(attachments);
            }
            to_res2(executor::process(&mut req).await)
        }
        (Ok(None), _) => to_res2(Err(crate::result::Error::WithMessage(String::from(
            "Field `request` is missing.",
        )))),
        (Err(e), _) => to_res2(Err(e)),
    };
    log::info!("Response used time:{:?}", now.elapsed());
    res
}

pub(crate) async fn answer_sse(Json(req): Json<Request>) -> impl IntoResponse {
    let now = std::time::Instant::now();
    let (s, r) = tokio::sync::mpsc::channel::<String>(1);
    let mut l = ANSWER_SSE_SESSIONS.lock().unwrap();
    l.insert(String::new(), s);
    log::info!("Response used time:{:?}", now.elapsed());
    ""
}

pub(super) fn get_sender(session_id: &str) -> Result<Option<Sender<String>>> {
    let l = ANSWER_SSE_SESSIONS.lock()?;
    if l.contains_key(session_id) {
        let s = l.get(session_id).unwrap();
        return Ok(Some(s.clone()));
    }
    Ok(None)
}
