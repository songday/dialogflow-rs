use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::vec::Vec;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::mpsc::Sender;

use super::completion::Prompt;
use crate::ai::huggingface::{HuggingFaceModel, LoadedHuggingFaceModel};
use crate::flow::rt::dto::StreamingResponseData;
use crate::man::settings;
use crate::result::{Error, Result};

static LOADED_MODELS: LazyLock<Mutex<HashMap<String, LoadedHuggingFaceModel>>> =
    LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));
// static HTTP_CLIENTS: LazyLock<Mutex<HashMap<String, LoadedHuggingFaceModel>>> =
//     LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));

pub(crate) struct SenderWrapper<D> {
    pub(crate) sender: Sender<D>,
    pub(crate) content_seq: usize,
}

impl SenderWrapper<StreamingResponseData> {
    pub(crate) fn send(&self, content: String) {
        let data = StreamingResponseData {
            content_seq: Some(self.content_seq),
            content,
        };
        crate::sse_send!(self.sender, data);
    }
    pub(crate) fn try_send(&self, content: String) -> Result<()> {
        let data = StreamingResponseData {
            content_seq: Some(self.content_seq),
            content,
        };
        self.sender
            .try_send(data)
            .map_err(|e| Error::WithMessage(format!("{:#?}", &e)))
    }
}

pub(crate) enum ResultSender<'r, D> {
    ChannelSender(SenderWrapper<D>),
    StrBuf(&'r mut String),
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
    if let Some(settings) = settings::get_settings(robot_id)? {
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
    result_sender: ResultSender<'_, StreamingResponseData>,
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
    let mut messages: Vec<Value> = match chat_history {
        Some(h) if !h.is_empty() => {
            let mut d = Vec::with_capacity(h.len() + 1);
            d.push(Value::Null);
            for p in h.into_iter() {
                let mut map = Map::new();
                map.insert(p.role, Value::String(p.content));
                d.push(Value::from(map));
            }
            d
        }
        _ => Vec::with_capacity(1),
    };
    messages[0] = Value::Object(sys_message);
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

    let stream = match result_sender {
        ResultSender::ChannelSender(_) => true,
        ResultSender::StrBuf(_) => false,
    };
    req_body.insert(String::from("stream"), Value::Bool(stream));
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
    match result_sender {
        ResultSender::ChannelSender(_sender_wrapper) => {
            let mut stream = res.bytes_stream();
            while let Some(item) = stream.next().await {
                let chunk = item?;
                let v: Value = serde_json::from_slice(chunk.as_ref())?;
                if let Some(choices) = v.get("choices")
                    && choices.is_array()
                {
                    if let Some(choices) = choices.as_array()
                        && !choices.is_empty()
                    {
                        if let Some(item) = choices.first() {
                            if let Some(delta) = item.get("delta") {
                                if let Some(content) = delta.get("content")
                                    && content.is_string()
                                {
                                    if let Some(s) = content.as_str() {
                                        let m = String::from(s);
                                        log::info!("OpenAI push {}", &m);
                                        // crate::sse_send!(sender, m);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        ResultSender::StrBuf(sb) => {
            let v: Value = serde_json::from_slice(res.text().await?.as_ref())?;
            if let Some(choices) = v.get("choices")
                && choices.is_array()
            {
                if let Some(choices) = choices.as_array()
                    && !choices.is_empty()
                {
                    if let Some(item) = choices.first() {
                        if let Some(message) = item.get("message") {
                            if let Some(content) = message.get("content")
                                && content.is_string()
                            {
                                if let Some(s) = content.as_str() {
                                    log::info!("OpenAI push {s}");
                                    sb.push_str(s);
                                }
                            }
                        }
                    }
                }
            }
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
    result_sender: ResultSender<'_, StreamingResponseData>,
) -> Result<()> {
    let client = crate::external::http::get_client(
        connect_timeout_millis.into(),
        read_timeout_millis.into(),
        proxy_url,
    )?;
    let mut req_body = Map::new();
    req_body.insert(String::from("model"), Value::String(String::from(m)));
    let stream = match result_sender {
        ResultSender::ChannelSender(_) => true,
        ResultSender::StrBuf(_) => false,
    };
    req_body.insert(String::from("stream"), Value::Bool(stream));

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
    match result_sender {
        ResultSender::ChannelSender(sender_wrapper) => {
            let mut stream = res.bytes_stream();
            while let Some(item) = stream.next().await {
                let chunk = item?;
                let v: Value = serde_json::from_slice(chunk.as_ref())?;
                if let Some(message) = v.get("message")
                    && message.is_object()
                {
                    if let Some(content) = message.get("content")
                        && content.is_string()
                    {
                        if let Some(s) = content.as_str() {
                            if sender_wrapper.sender.is_closed() {
                                log::warn!("Ollama channel sender is closed");
                                break;
                            }
                            let m = String::from(s);
                            log::info!("Ollama push {}", &m);
                            sender_wrapper.send(m);
                        }
                    }
                }
            }
        }
        ResultSender::StrBuf(sb) => {
            let v: Value = serde_json::from_slice(res.bytes().await?.as_ref())?;
            if let Some(message) = v.get("message") {
                if message.is_object() {
                    if let Some(content) = message.get("content")
                        && content.is_string()
                    {
                        if let Some(s) = content.as_str() {
                            log::info!("Ollama returned {}", s);
                            sb.push_str(s);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
