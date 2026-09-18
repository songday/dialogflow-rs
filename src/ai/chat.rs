use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::vec::Vec;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::stream::{DeltaFormat, DeltaStream};
use crate::ai::huggingface::{HuggingFaceModel, LoadedHuggingFaceModel};
use crate::flow::rt::dto::StreamingResponseData;
use crate::man::settings;
use crate::result::{Error, Result};

/// 一条对话消息。历史、单个提示词、以及 `/ai/text/generation` 收上来的
/// JSON 提示词数组都是这个形状。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Prompt {
    pub(crate) role: String,
    pub(crate) content: String,
}

/// 本地 HuggingFace 模型的采样参数。原来住在 `completion.rs`——那个模块是
/// 给"文本生成"用的第二套 provider 配置，现在并进了这里，所以这几个常量也
/// 跟着搬（`gemma` / `llama` / `phi3` / `moondream` 都在用）。
pub(crate) const TEMPERATURE: f64 = 0.7;
pub(crate) const REPEAT_PENALTY: f32 = 1.1;
pub(crate) const REPEAT_LAST_N: usize = 64;

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
    /// 兼容老记录的两个名字：
    /// - `OpenAI`：2026-09-17 之前这个变体叫这个名字；
    /// - `Ollama`：UI 里曾经有第三个"Ollama"选项，它对应的是**原生**端点，
    ///   那条路现在由 `api_url` 选中（`is_ollama_chat_url`），不再需要一个
    ///   单独的变体。老记录的 `apiUrl` 就是 `http://localhost:11434/api/chat`，
    ///   所以别名过来之后照样走原生实现。
    ///
    /// 两个别名都可以在所有实例保存过一次设置后删掉。
    #[serde(alias = "OpenAI", alias = "Ollama")]
    OpenAICompatible(String),
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
            ChatProvider::OpenAICompatible(m) => {
                let connect_timeout =
                    connect_timeout.unwrap_or(settings.chat_provider.connect_timeout_millis);
                let read_timeout =
                    read_timeout.unwrap_or(settings.chat_provider.read_timeout_millis);
                // Ollama 的原生对话端点和 OpenAI 协议不是一套东西：token 上限在
                // `options.num_predict` 而不是 `max_tokens`，图片是裸 base64 而不是
                // content parts，回来的是 NDJSON 而不是 SSE。所以按**地址**决定走哪条
                // 实现——UI 里的厂商本来就是从地址反查的，两者不可能不一致，老记录
                // 里指向 `.../v1/chat/completions` 的 Ollama 也照旧走 OpenAI 兼容分支。
                if is_ollama_chat_url(&settings.chat_provider.api_url) {
                    ollama(
                        &settings.chat_provider.api_url,
                        &m,
                        chat_history,
                        media,
                        &settings.chat_provider.api_key,
                        connect_timeout,
                        read_timeout,
                        &settings.chat_provider.proxy_url,
                        settings.chat_provider.max_response_token_length,
                        result_sender,
                    )
                    .await?;
                } else {
                    open_ai_compatible(
                        &m,
                        chat_history,
                        media,
                        &settings.chat_provider.api_url,
                        &settings.chat_provider.api_key,
                        settings.chat_provider.max_response_token_length,
                        connect_timeout,
                        read_timeout,
                        &settings.chat_provider.proxy_url,
                        result_sender,
                    )
                    .await?;
                }
                Ok(())
            }
        }
    } else {
        Err(Error::WithMessage(format!(
            "Can NOT retrieve settings from robot_id: {robot_id}"
        )))
    }
}

/// `/ai/text/generation` 的实现：对话节点里那个"生成文本"按钮。
///
/// 它原来走 `completion.rs`——一份独立的 provider 配置加一套独立的请求构造，
/// 于是同一个机器人要在设置页配两遍模型；而在线模型那条分支把调用方给过来的
/// JSON 提示词数组当成纯文本塞进一条 user 消息，界面里填的 system 提示词根本
/// 到不了模型。现在它和对话节点走**完全相同**的路径、用同一份配置。
pub(crate) async fn gen_text(
    robot_id: &str,
    prompt: &str,
    sender: UnboundedSender<StreamingResponseData>,
) -> Result<()> {
    let history = parse_prompt(prompt);
    // 这里的应答只往通道里推，没人读累积的文本，但 `ResultSender` 两种变体都
    // 需要一块缓冲，就随它留着。
    let mut answer = String::with_capacity(1024);
    chat(
        robot_id,
        Some(history),
        None,
        None,
        None,
        ResultSender::ChannelSender(SenderWrapper::new(sender, 0), &mut answer),
    )
    .await
}

/// 调用方发来的提示词：`[{"role":"user","content":"…"}]`。不是 JSON 就当成
/// 一条 user 消息，纯文本的老调用方也不至于直接失败。
///
/// system 消息统一提到最前。调用方是按 `[user, system]` 拼的（`DialogNode.vue`
/// 先 push user 再 push system），而 system 落在 user 之后对部分端点是非法顺序；
/// 本地 HF 那条路径本来也是把它拆出来放最前的，这样三个后端行为才一致。
fn parse_prompt(s: &str) -> Vec<Prompt> {
    let mut prompts: Vec<Prompt> = serde_json::from_str(s).unwrap_or_else(|_| {
        vec![Prompt {
            role: String::from("user"),
            content: String::from(s),
        }]
    });
    // `sort_by_key` 是稳定排序，system 之间的相对顺序不变。
    prompts.sort_by_key(|p| !p.role.eq("system"));
    prompts
}

/// 地址是不是 Ollama 的原生对话端点。
///
/// 只看路径不看主机：远程 Ollama（`http://192.168.x.x:11434/api/chat`）和本机
/// 一样常见，而 `/api/chat` 这个路径是 Ollama 特有的。
fn is_ollama_chat_url(u: &str) -> bool {
    u.trim().trim_end_matches('/').ends_with("/api/chat")
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

async fn open_ai_compatible(
    m: &str,
    chat_history: Option<Vec<Prompt>>,
    media: Option<&crate::ai::dto::UserMediaData>,
    api_url: &str,
    api_key: &str,
    max_response_token_length: u32,
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
            // 键必须是 `role` 和 `content`。这里原来写的是
            // `map.insert(p.role, Value::String(p.content))`，即把角色名当成了
            // 键（`{"user":"hi"}`），任何端点都会以"缺少 role"驳回；视觉那段
            // 靠 `get("role")` 找最后一条 user 消息，也因此永远找不到。
            let mut map = Map::new();
            map.insert(String::from("role"), Value::String(p.role));
            map.insert(String::from("content"), Value::String(p.content));
            messages.push(Value::Object(map));
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
    // 这个上限原来在本路径上被完全忽略（只有本地和 Ollama 分支用了），
    // 所以界面上那个"最大响应 token 长度"对任何在线模型都是摆设。
    req_body.insert(
        String::from("max_tokens"),
        Value::from(max_response_token_length),
    );
    let obj = Value::Object(req_body);
    // 不再回退到 api.openai.com：用户填了别家的 key 却漏了地址时，
    // 静默把请求（和 key）打向 OpenAI 不是我们该做的选择。
    if api_url.is_empty() {
        return Err(Error::WithMessage(String::from(
            "OpenAI-compatible API URL is empty, please configure it in settings.",
        )));
    }
    let u = String::from(api_url);
    let req = client
        .post(&u)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .body(serde_json::to_string(&obj)?);
    let res = req.send().await?;
    let status = res.status();
    if !status.is_success() {
        // Without this a rejected key or an unknown model shows up as a
        // silently empty answer.
        let body = res.text().await.unwrap_or_default();
        // 报错里带上地址：这是排查任意第三方端点的唯一线索，
        // 而用户往往同时配了好几个 provider。
        return Err(Error::WithMessage(format!(
            "OpenAI-compatible endpoint {u} returned {status}: {body}"
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
    api_key: &str,
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
    let mut req = client.post(u).header("Content-Type", "application/json");
    // Ollama 自己不看这个头，但把模型放在网关/鉴权代理后面的（以及 Ollama 云）
    // 需要它；界面上的 API Key 输入框就在这一项下面，配了却不发等于骗人。
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {api_key}"));
    }
    let res = req.body(body).send().await?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(Error::WithMessage(format!(
            "Ollama endpoint {u} returned {status}: {body}"
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
    use crate::ai::test_support::{drain, prompt, serve, serve_capturing, sse_event, sse_head};

    use super::*;

    /// What actually goes on the wire: the configured URL, the key, and the
    /// token cap. The cap used to be dropped entirely on this path, and an
    /// empty URL used to silently become OpenAI's.
    #[tokio::test]
    async fn open_ai_compatible_sends_url_key_and_token_cap() {
        let body = format!("{}data: [DONE]\n\n", sse_event("hi"));
        let (url, request) =
            serve_capturing("/custom/llm/chat/completions", sse_head(), &body).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut answer = String::new();
        open_ai_compatible(
            "deepseek-chat",
            prompt(),
            None,
            &url,
            "test-key",
            4_242,
            3_323,
            9_923,
            "",
            ResultSender::ChannelSender(SenderWrapper::new(tx, 0), &mut answer),
        )
        .await
        .unwrap();
        assert_eq!(answer, "hi");
        assert_eq!(drain(&mut rx), vec!["hi"]);

        let request = request.await.unwrap();
        assert!(
            request.starts_with("POST /custom/llm/chat/completions "),
            "the configured path must be used, not a hardcoded one: {request}"
        );
        // Header names arrive lowercased over HTTP/1.1.
        assert!(
            request.to_lowercase().contains("authorization: bearer test-key"),
            "the configured key must be sent: {request}"
        );
        let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
        assert!(
            body.contains("\"max_tokens\":4242"),
            "the configured token cap must be sent: {body}"
        );
        // 消息必须是 `{"role":…,"content":…}`。这里曾经写成 `{"user":"hi"}`——
        // 把角色名当成了键，任何端点都会以"缺少 role"驳回，视觉那段靠
        // `get("role")` 找最后一条 user 消息也因此永远找不到。
        assert!(
            body.contains("\"role\":\"user\"") && body.contains("\"content\":\"hi\""),
            "every message needs role and content keys: {body}"
        );
        assert!(
            !body.contains("\"hi\":") && !body.contains("\"user\":\"hi\""),
            "the role must not be used as the key: {body}"
        );
    }

    /// `/ai/text/generation` 收到的提示词是 JSON 数组，且调用方按
    /// `[user, system]` 的顺序拼（`DialogNode.vue`）。system 必须被提到最前，
    /// 否则部分端点会直接拒绝这个顺序；纯文本则退化成一条 user 消息。
    #[test]
    fn parse_prompt_puts_system_first_and_accepts_plain_text() {
        let p = parse_prompt(
            r#"[{"role":"user","content":"写一句问候"},{"role":"system","content":"你是客服"}]"#,
        );
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].role, "system");
        assert_eq!(p[0].content, "你是客服");
        assert_eq!(p[1].role, "user");
        assert_eq!(p[1].content, "写一句问候");

        let p = parse_prompt("hello");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].role, "user");
        assert_eq!(p[0].content, "hello");
    }

    /// The point of the streaming path: provider output arrives as arbitrary
    /// byte fragments and the answer still has to come out whole and in order.
    /// The cuts below land inside a multi-byte character and between an event's
    /// lines, which is where the old code gave up and lost the whole answer.
    #[tokio::test]
    async fn open_ai_compatible_streams_deltas_whatever_the_chunk_boundaries() {
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
        open_ai_compatible(
            "test-model",
            prompt(),
            None,
            &url,
            "test-key",
            1_000,
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
    async fn open_ai_compatible_stops_when_the_client_is_gone() {
        let body = format!("{}{}{}", sse_event("a"), sse_event("b"), sse_event("c"));
        let url = serve(sse_head(), &body, &[]).await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        let mut answer = String::new();
        open_ai_compatible(
            "test-model",
            prompt(),
            None,
            &url,
            "test-key",
            1_000,
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
    async fn open_ai_compatible_reports_a_http_error() {
        let body = String::from("{\"error\":{\"message\":\"bad key\"}}");
        let head = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let url = serve(head, &body, &[]).await;
        let mut answer = String::new();
        let e = open_ai_compatible(
            "test-model",
            prompt(),
            None,
            &url,
            "test-key",
            1_000,
            3_317,
            9_919,
            "",
            ResultSender::StrBuf(&mut answer),
        )
        .await
        .unwrap_err();
        let e = format!("{e:?}");
        assert!(e.contains("401"), "the error should name the status: {e}");
        assert!(e.contains(&url), "the error should name the URL: {e}");
        assert!(answer.is_empty(), "nothing should be read from a rejection");
    }

    /// An empty URL is a configuration mistake, not a reason to quietly send
    /// the user's key to OpenAI.
    #[tokio::test]
    async fn open_ai_compatible_refuses_an_empty_url() {
        let mut answer = String::new();
        let e = open_ai_compatible(
            "test-model",
            prompt(),
            None,
            "",
            "test-key",
            1_000,
            3_321,
            9_921,
            "",
            ResultSender::StrBuf(&mut answer),
        )
        .await
        .unwrap_err();
        assert!(
            format!("{e:?}").contains("URL is empty"),
            "the error should say what is missing: {e:?}"
        );
    }

    /// 走 Ollama 原生端点时线上的形状和 OpenAI 兼容那条完全不同：token 上限在
    /// `options.num_predict`，图片是裸 base64。这条实现由地址（`/api/chat`）选中，
    /// 所以这个测试同时锁住了"为什么值得为它分一条路"。
    #[tokio::test]
    async fn ollama_native_endpoint_sends_options_and_raw_base64_images() {
        let body = "{\"message\":{\"content\":\"hi\"},\"done\":false}\n\
                    {\"message\":{\"content\":\"\"},\"done\":true}\n";
        let head = String::from(
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
        );
        let (url, request) = serve_capturing("/api/chat", head, body).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut answer = String::new();
        // `UserMediaData` 存的可能是 data URI，Ollama 只吃逗号后面那段。
        let media = crate::ai::dto::UserMediaData {
            images: vec![String::from("data:image/png;base64,QUJD")],
        };
        ollama(
            &url,
            "llava",
            prompt(),
            Some(&media),
            "test-key",
            3_331,
            9_991,
            "",
            4_242,
            ResultSender::ChannelSender(SenderWrapper::new(tx, 0), &mut answer),
        )
        .await
        .unwrap();
        assert_eq!(answer, "hi");
        assert_eq!(drain(&mut rx), vec!["hi"]);

        let request = request.await.unwrap();
        assert!(
            request.starts_with("POST /api/chat "),
            "the configured path must be used: {request}"
        );
        // Ollama 本身不校验这个头，但网关/鉴权代理和 Ollama 云需要它。
        assert!(
            request.to_lowercase().contains("authorization: bearer test-key"),
            "the configured key must be sent: {request}"
        );
        let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
        assert!(
            body.contains("\"num_predict\":4242"),
            "Ollama takes the cap in options.num_predict: {body}"
        );
        assert!(
            !body.contains("max_tokens"),
            "max_tokens is an OpenAI field and Ollama ignores it: {body}"
        );
        assert!(
            body.contains("\"images\":[\"QUJD\"]"),
            "Ollama wants bare base64 without the data URI prefix: {body}"
        );
    }

    /// 只有 Ollama 的原生对话地址才选中那条实现；`/v1/chat/completions`
    /// （Ollama 的兼容端点，以及所有别的厂商）继续走 OpenAI 兼容分支。
    #[test]
    fn only_ollama_native_urls_select_the_ollama_path() {
        assert!(is_ollama_chat_url("http://localhost:11434/api/chat"));
        assert!(is_ollama_chat_url("http://192.168.1.9:11434/api/chat/"));
        assert!(is_ollama_chat_url("  http://localhost:11434/api/chat  "));
        assert!(!is_ollama_chat_url(
            "http://localhost:11434/v1/chat/completions"
        ));
        assert!(!is_ollama_chat_url("https://api.deepseek.com/v1/chat/completions"));
        assert!(!is_ollama_chat_url(""));
    }

    /// 老记录必须还能读进来：反序列化失败不是"少一个选项"，而是 `get_settings`
    /// 直接返回 Err、设置页整个打不开（老 `OpenAI` 那个坑就是这么来的）。
    ///
    /// `Ollama` 这条尤其要紧：UI 里已经没有这个选项，但历史记录存的是
    /// `{"id":"Ollama"}` 加上 `apiUrl = http://localhost:11434/api/chat`，
    /// 别名接过来之后由**地址**选中原生实现，行为不变。
    #[test]
    fn legacy_provider_ids_still_deserialize() {
        let p: ChatProvider = serde_json::from_str(r#"{"id":"Ollama","model":"llama3"}"#).unwrap();
        assert!(
            matches!(p, ChatProvider::OpenAICompatible(ref m) if m == "llama3"),
            "the old Ollama id must land on the URL-selected variant"
        );
        let p: ChatProvider = serde_json::from_str(r#"{"id":"OpenAI","model":"gpt-4o"}"#).unwrap();
        assert!(matches!(p, ChatProvider::OpenAICompatible(ref m) if m == "gpt-4o"));

        // 别名只影响读；再存一次就写成新名字，这是别名将来可以删掉的前提。
        let p = ChatProvider::OpenAICompatible(String::from("x"));
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"id":"OpenAICompatible","model":"x"}"#
        );
    }
}
