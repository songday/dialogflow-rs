use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::vec::Vec;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::completion::Prompt;
use super::stream::{DeltaFormat, DeltaStream};
use crate::ai::huggingface::{HuggingFaceModel, LoadedHuggingFaceModel};
use crate::flow::rt::dto::StreamingResponseData;
use crate::man::settings;
use crate::result::{Error, Result};

static LOADED_MODELS: LazyLock<Mutex<HashMap<String, LoadedHuggingFaceModel>>> =
    LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));
// static HTTP_CLIENTS: LazyLock<Mutex<HashMap<String, LoadedHuggingFaceModel>>> =
//     LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));

/// The sink one answer's deltas go to.
pub(crate) struct SenderWrapper<D> {
    sender: UnboundedSender<D>,
    content_seq: usize,
}

impl SenderWrapper<StreamingResponseData> {
    pub(crate) fn new(sender: UnboundedSender<StreamingResponseData>, content_seq: usize) -> Self {
        Self {
            sender,
            content_seq,
        }
    }

    /// Returns `false` once the receiver is gone, which is how a client hanging
    /// up reaches the generation loops: they stop rather than keep producing
    /// into a channel nobody reads.
    ///
    /// The channel is unbounded, so this never blocks and never reorders — the
    /// old per-token `spawn_blocking` send could do both.
    pub(crate) fn send(&self, content: String) -> bool {
        self.sender
            .send(StreamingResponseData {
                content_seq: Some(self.content_seq),
                content,
            })
            .is_ok()
    }
}

/// Where a generated answer goes. Both variants accumulate the full text, so a
/// node still has the complete answer to reason about afterwards; they differ
/// only in whether each delta is also pushed to the client on arrival.
pub(crate) enum ResultSender<'r, D> {
    ChannelSender(SenderWrapper<D>, &'r mut String),
    StrBuf(&'r mut String),
}

impl ResultSender<'_, StreamingResponseData> {
    /// Whether the caller wants deltas as they are produced. This also decides
    /// whether a provider asks its backend to stream at all.
    pub(crate) fn is_streaming(&self) -> bool {
        matches!(self, Self::ChannelSender(..))
    }

    /// Appends a delta to the answer and, when streaming, pushes it out.
    /// Returns `false` once the client is gone and generation should stop.
    pub(crate) fn push_delta(&mut self, delta: String) -> bool {
        match self {
            Self::ChannelSender(sender, answer) => {
                answer.push_str(&delta);
                sender.send(delta)
            }
            Self::StrBuf(answer) => {
                answer.push_str(&delta);
                true
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "id", content = "model")]
pub(crate) enum ChatProvider {
    HuggingFace(HuggingFaceModel),
    OpenAI(String),
    Ollama(String),
}

pub(crate) fn replace_model_cache(robot_id: &str, m: &HuggingFaceModel) -> Result<()> {
    let m = LoadedHuggingFaceModel::load(m)?;
    let mut r = LOADED_MODELS.lock()?;
    r.insert(String::from(robot_id), m);
    Ok(())
}

pub(crate) async fn chat(
    robot_id: &str,
    // prompt: &str,
    chat_history: Option<Vec<Prompt>>,
    media: Option<&crate::ai::dto::UserMediaData>,
    connect_timeout: Option<u32>,
    read_timeout: Option<u32>,
    result_sender: ResultSender<'_, StreamingResponseData>,
) -> Result<()> {
    if let Some(settings) = settings::get_settings(robot_id).await? {
        // log::info!("{:?}", &settings.chat_provider.provider);
        match settings.chat_provider.provider {
            ChatProvider::HuggingFace(m) => {
                huggingface(
                    robot_id,
                    &m,
                    chat_history,
                    media,
                    settings.chat_provider.max_response_token_length as usize,
                    result_sender,
                )?;
                Ok(())
            }
            ChatProvider::OpenAI(m) => {
                open_ai(
                    &m,
                    chat_history,
                    media,
                    &settings.chat_provider.api_url,
                    &settings.chat_provider.api_key,
                    connect_timeout.unwrap_or(settings.chat_provider.connect_timeout_millis),
                    read_timeout.unwrap_or(settings.chat_provider.read_timeout_millis),
                    &settings.chat_provider.proxy_url,
                    result_sender,
                )
                .await?;
                Ok(())
            }
            ChatProvider::Ollama(m) => {
                ollama(
                    &settings.chat_provider.api_url,
                    &m,
                    chat_history,
                    media,
                    connect_timeout.unwrap_or(settings.chat_provider.connect_timeout_millis),
                    read_timeout.unwrap_or(settings.chat_provider.read_timeout_millis),
                    &settings.chat_provider.proxy_url,
                    settings.chat_provider.max_response_token_length,
                    result_sender,
                )
                .await?;
                Ok(())
            }
        }
    } else {
        Err(Error::WithMessage(format!(
            "Can NOT retrieve settings from robot_id: {robot_id}"
        )))
    }
}

fn huggingface(
    robot_id: &str,
    m: &HuggingFaceModel,
    chat_history: Option<Vec<Prompt>>,
    media: Option<&crate::ai::dto::UserMediaData>,
    sample_len: usize,
    mut result_sender: ResultSender<'_, StreamingResponseData>,
) -> Result<()> {
    let info = m.get_info();
    // log::info!("model_type={:?}", &info.model_type);
    let empty_media = crate::ai::dto::UserMediaData::default();
    let media = media.unwrap_or(&empty_media);
    if !media.is_empty() && !info.supports_vision() {
        return Err(Error::WithMessage(format!(
            "Model {:?} doesn't support image input, please configure a vision model (e.g. Moondream2, or OpenAI/Ollama vision models).",
            &info.model_type
        )));
    }
    let new_prompt = info.convert_prompt("", chat_history.clone())?;
    log::info!("Prompt: {}", &new_prompt);
    let mut model = LOADED_MODELS.lock().unwrap_or_else(|e| {
        log::warn!("{:#?}", &e);
        e.into_inner()
    });
    if !model.contains_key(robot_id) {
        let r = LoadedHuggingFaceModel::load(m)?;
        model.insert(String::from(robot_id), r);
    };
    let loaded_model = model.get_mut(robot_id).unwrap();
    match loaded_model {
        LoadedHuggingFaceModel::Gemma(m) => super::gemma::gen_text(
            &m.0,
            &m.1,
            &m.2,
            &new_prompt,
            sample_len,
            Some(0.5),
            &mut result_sender,
        ),
        LoadedHuggingFaceModel::Llama(m) => super::llama::gen_text(
            &m.0,
            &m.1,
            &m.2,
            &m.3,
            &m.4,
            &new_prompt,
            sample_len,
            Some(25),
            Some(0.5),
            &mut result_sender,
        ),
        LoadedHuggingFaceModel::Phi3(m) => super::phi3::gen_text(
            &m.0,
            &m.1,
            &m.2,
            &new_prompt,
            sample_len,
            Some(0.5),
            &mut result_sender,
        ),
        LoadedHuggingFaceModel::Moondream((device, model, tokenizer)) => {
            // Moondream's text backbone doesn't take a chat-formatted prompt,
            // the user question is the last user message.
            let question = chat_history
                .as_ref()
                .and_then(|h| {
                    h.iter()
                        .rev()
                        .find(|p| p.role.eq("user") && !p.content.is_empty())
                })
                .map(|p| p.content.clone())
                .unwrap_or_default();
            super::moondream::gen_text(
                device,
                model,
                tokenizer,
                super::moondream::MoondreamInput {
                    prompt: &question,
                    images_base64: &media.images,
                },
                sample_len,
                Some(0.5),
                &mut result_sender,
            )
        }
        LoadedHuggingFaceModel::Bert(_m) => Err(Error::WithMessage(format!(
            "Unsuported model type {:?}.",
            &info.model_type
        ))),
    }
    // Ok(())
}

async fn open_ai(
    m: &str,
    chat_history: Option<Vec<Prompt>>,
    media: Option<&crate::ai::dto::UserMediaData>,
    api_url: &str,
    api_key: &str,
    connect_timeout_millis: u32,
    read_timeout_millis: u32,
    proxy_url: &str,
    mut result_sender: ResultSender<'_, StreamingResponseData>,
) -> Result<()> {
    // let client = HTTP_CLIENTS.lock()?.entry(String::from("value")).or_insert(crate::external::http::get_client(connect_timeout_millis.into(), read_timeout_millis.into(), proxy_url)?);
    let client = crate::external::http::get_client(
        connect_timeout_millis.into(),
        read_timeout_millis.into(),
        proxy_url,
    )?;
    let mut req_body = Map::new();
    req_body.insert(String::from("model"), Value::from(m));

    let mut sys_message = Map::new();
    sys_message.insert(String::from("role"), Value::from("system"));
    sys_message.insert(
        String::from("content"),
        Value::from("You are a helpful assistant."),
    );
    let empty_media = crate::ai::dto::UserMediaData::default();
    let media = media.unwrap_or(&empty_media);
    // The system message is pushed rather than written into a pre-sized slot:
    // indexing a `Vec::with_capacity(1)` that was never pushed to panics, which
    // is what an empty or absent history used to do.
    let mut messages: Vec<Value> = Vec::with_capacity(1 + chat_history.as_ref().map_or(0, Vec::len));
    messages.push(Value::Object(sys_message));
    if let Some(h) = chat_history {
        for p in h.into_iter() {
            let mut map = Map::new();
            map.insert(p.role, Value::String(p.content));
            messages.push(Value::from(map));
        }
    }
    // OpenAI-compatible vision format: the last user message's content becomes
    // an array of typed parts (text + image_url) when images are present.
    // Works with OpenAI, vLLM and other OpenAI-compatible servers.
    if !media.is_empty() {
        let last_user_idx = messages.iter().rposition(|m| {
            m.get("role").and_then(|r| r.as_str()) == Some("user")
        });
        if let Some(idx) = last_user_idx {
            let msg = messages[idx].as_object().unwrap();
            let text = msg
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string();
            let mut parts: Vec<Value> = Vec::with_capacity(media.images.len() + 1);
            if !text.is_empty() {
                let mut text_part = Map::new();
                text_part.insert(String::from("type"), Value::from("text"));
                text_part.insert(String::from("text"), Value::String(text));
                parts.push(Value::Object(text_part));
            }
            for img in media.images.iter() {
                let mut image_url = Map::new();
                image_url.insert(String::from("url"), Value::String(img.clone()));
                let mut part = Map::new();
                part.insert(String::from("type"), Value::from("image_url"));
                part.insert(String::from("image_url"), Value::Object(image_url));
                parts.push(Value::Object(part));
            }
            let mut new_msg = Map::new();
            new_msg.insert(String::from("role"), Value::from("user"));
            new_msg.insert(String::from("content"), Value::Array(parts));
            messages[idx] = Value::Object(new_msg);
        }
    }
    let messages = Value::Array(messages);
    req_body.insert(String::from("messages"), messages);

    // The backend is asked to stream only when someone is reading frame by
    // frame; otherwise one document is simpler for both sides.
    req_body.insert(
        String::from("stream"),
        Value::Bool(result_sender.is_streaming()),
    );
    let obj = Value::Object(req_body);
    let u = if api_url.is_empty() {
        String::from("https://api.openai.com/v1/chat/completions")
    } else {
        String::from(api_url)
    };
    let req = client
        .post(u)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .body(serde_json::to_string(&obj)?);
    let res = req.send().await?;
    let status = res.status();
    if !status.is_success() {
        // Without this a rejected key or an unknown model shows up as a
        // silently empty answer.
        let body = res.text().await.unwrap_or_default();
        return Err(Error::WithMessage(format!(
            "OpenAI-compatible endpoint returned {status}: {body}"
        )));
    }
    if result_sender.is_streaming() {
        // `bytes_stream()` yields arbitrary fragments: several events, half an
        // event, or one ending mid-character. DeltaStream buffers them so only
        // complete payloads are dispatched.
        let mut deltas = DeltaStream::new(DeltaFormat::OpenAi);
        let mut stream = res.bytes_stream();
        while let Some(item) = stream.next().await {
            for delta in deltas.push(item?.as_ref())? {
                if !result_sender.push_delta(delta) {
                    log::warn!("OpenAI stream receiver is gone, stopping generation.");
                    return Ok(());
                }
            }
            if deltas.is_done() {
                break;
            }
        }
        for delta in deltas.finish()? {
            if !result_sender.push_delta(delta) {
                return Ok(());
            }
        }
    } else {
        let v: Value = serde_json::from_slice(res.bytes().await?.as_ref())?;
        if let Some(content) = v
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
        {
            log::info!("OpenAI returned {content}");
            result_sender.push_delta(String::from(content));
        }
    }
    Ok(())
}

async fn ollama(
    u: &str,
    m: &str,
    // s: &str,
    chat_history: Option<Vec<Prompt>>,
    media: Option<&crate::ai::dto::UserMediaData>,
    connect_timeout_millis: u32,
    read_timeout_millis: u32,
    proxy_url: &str,
    sample_len: u32,
    mut result_sender: ResultSender<'_, StreamingResponseData>,
) -> Result<()> {
    let client = crate::external::http::get_client(
        connect_timeout_millis.into(),
        read_timeout_millis.into(),
        proxy_url,
    )?;
    let mut req_body = Map::new();
    req_body.insert(String::from("model"), Value::String(String::from(m)));
    req_body.insert(
        String::from("stream"),
        Value::Bool(result_sender.is_streaming()),
    );

    let empty_media = crate::ai::dto::UserMediaData::default();
    let media = media.unwrap_or(&empty_media);
    let messages: Vec<Value> = match chat_history {
        Some(h) if !h.is_empty() => {
            let mut d = Vec::with_capacity(h.len() + 1);
            let mut seen_user = false;
            for p in h.into_iter() {
                let is_user = p.role.eq("user");
                if p.content.is_empty() && !(is_user && !media.is_empty() && !seen_user) {
                    continue;
                }
                let mut map = Map::new();
                map.insert("role".into(), Value::String(p.role.clone()));
                map.insert("content".into(), Value::String(p.content));
                // Ollama expects raw base64 (no data URI prefix) in "images",
                // attached to the last user message.
                if is_user && !media.is_empty() && !seen_user {
                    seen_user = true;
                    let images: Vec<Value> = media
                        .images
                        .iter()
                        .map(|img| {
                            Value::String(match img.split_once(",") {
                                Some((prefix, rest)) if prefix.starts_with("data:") => {
                                    String::from(rest)
                                }
                                _ => img.clone(),
                            })
                        })
                        .collect();
                    map.insert("images".into(), Value::Array(images));
                }
                d.push(Value::from(map));
            }
            d
        }
        _ => Vec::with_capacity(1),
    };
    req_body.insert(String::from("messages"), Value::Array(messages));

    let mut num_predict = Map::new();
    num_predict.insert(String::from("num_predict"), Value::from(sample_len));
    req_body.insert(String::from("options"), Value::from(num_predict));

    let obj = Value::Object(req_body);
    let body = serde_json::to_string(&obj)?;
    log::info!("Request Ollama body {}", &body);
    let req = client.post(u).body(body);
    let res = req.send().await?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(Error::WithMessage(format!(
            "Ollama endpoint returned {status}: {body}"
        )));
    }
    if result_sender.is_streaming() {
        let mut deltas = DeltaStream::new(DeltaFormat::OllamaChat);
        let mut stream = res.bytes_stream();
        while let Some(item) = stream.next().await {
            for delta in deltas.push(item?.as_ref())? {
                if !result_sender.push_delta(delta) {
                    log::warn!("Ollama stream receiver is gone, stopping generation.");
                    return Ok(());
                }
            }
            if deltas.is_done() {
                break;
            }
        }
        for delta in deltas.finish()? {
            if !result_sender.push_delta(delta) {
                return Ok(());
            }
        }
    } else {
        let v: Value = serde_json::from_slice(res.bytes().await?.as_ref())?;
        if let Some(content) = v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
        {
            log::info!("Ollama returned {content}");
            result_sender.push_delta(String::from(content));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serves one canned response, written in the given pieces so the write
    /// boundaries fall exactly where a test wants them — including in the
    /// middle of a multi-byte character. `cuts` are absolute byte offsets into
    /// `body`, so the client sees the body arrive in `cuts.len() + 1` reads.
    ///
    /// Each test uses its own read timeout, and that is part of the client
    /// cache key, so no two tests share a client or its connection pool.
    async fn serve(head: String, body: &str, cuts: &[usize]) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let body = body.as_bytes();
        let mut pieces: Vec<Vec<u8>> = vec![head.into_bytes()];
        let mut start = 0;
        for &cut in cuts {
            assert!(cut >= start && cut <= body.len(), "cut {cut} is out of order or past the end");
            pieces.push(body[start..cut].to_vec());
            start = cut;
        }
        pieces.push(body[start..].to_vec());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            // Read the request head so the client is not still writing when the
            // response arrives.
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).await;
            for piece in pieces {
                if socket.write_all(&piece).await.is_err() {
                    return;
                }
                tokio::task::yield_now().await;
            }
        });
        format!("http://{addr}/v1/chat/completions")
    }

    fn sse_head() -> String {
        String::from("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
    }

    fn sse_event(text: &str) -> String {
        format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}}}}]}}\n\n")
    }

    fn prompt() -> Option<Vec<Prompt>> {
        Some(vec![Prompt {
            role: String::from("user"),
            content: String::from("hi"),
        }])
    }

    /// Collects what the stream pushed, in order.
    fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<StreamingResponseData>) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(f) = rx.try_recv() {
            assert_eq!(f.content_seq, Some(0), "every delta belongs to answer 0");
            out.push(f.content);
        }
        out
    }

    /// The point of the streaming path: provider output arrives as arbitrary
    /// byte fragments and the answer still has to come out whole and in order.
    /// The cuts below land inside a multi-byte character and between an event's
    /// lines, which is where the old code gave up and lost the whole answer.
    #[tokio::test]
    async fn open_ai_streams_deltas_whatever_the_chunk_boundaries() {
        let body = format!(
            ": OPENROUTER PROCESSING\n\n{}{}{}data: [DONE]\n\n{}",
            sse_event("Hello, "),
            sse_event("世"),
            sse_event("界"),
            sse_event("NEVER"),
        );
        // A cut in the middle of an event's payload, then one and two bytes into
        // 世, so a multi-byte character is split across three reads and the
        // fragments in between are not valid UTF-8 on their own.
        let hello = body.find("Hello").expect("the body contains Hello") + 2;
        let world = body.find('世').expect("the body contains 世") + 1;
        let cuts = [0, hello, world, world + 1];
        let url = serve(sse_head(), &body, &cuts).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut answer = String::new();
        open_ai(
            "test-model",
            prompt(),
            None,
            &url,
            "test-key",
            3_311,
            9_913,
            "",
            ResultSender::ChannelSender(SenderWrapper::new(tx, 0), &mut answer),
        )
        .await
        .unwrap();
        assert_eq!(answer, "Hello, 世界", "text after [DONE] must not appear");
        assert_eq!(drain(&mut rx), vec!["Hello, ", "世", "界"]);
    }

    /// A client that hangs up stops the generation, and stopping is not an
    /// error: the run just ends.
    #[tokio::test]
    async fn open_ai_stops_when_the_client_is_gone() {
        let body = format!("{}{}{}", sse_event("a"), sse_event("b"), sse_event("c"));
        let url = serve(sse_head(), &body, &[]).await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        let mut answer = String::new();
        open_ai(
            "test-model",
            prompt(),
            None,
            &url,
            "test-key",
            3_313,
            9_917,
            "",
            ResultSender::ChannelSender(SenderWrapper::new(tx, 0), &mut answer),
        )
        .await
        .unwrap();
        assert_eq!(answer, "a", "generation should stop at the first delta");
    }

    /// A rejection has to surface. It used to look like an empty answer, which
    /// a flow reports as a successful response with nothing in it.
    #[tokio::test]
    async fn open_ai_reports_a_http_error() {
        let body = String::from("{\"error\":{\"message\":\"bad key\"}}");
        let head = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let url = serve(head, &body, &[]).await;
        let mut answer = String::new();
        let e = open_ai(
            "test-model",
            prompt(),
            None,
            &url,
            "test-key",
            3_317,
            9_919,
            "",
            ResultSender::StrBuf(&mut answer),
        )
        .await
        .unwrap_err();
        let e = format!("{e:?}");
        assert!(e.contains("401"), "the error should name the status: {e}");
        assert!(answer.is_empty(), "nothing should be read from a rejection");
    }
}
