use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

// use crossbeam_channel::Sender;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::chat::{ResultSender, SenderWrapper};
use super::stream::{DeltaFormat, DeltaStream};
use crate::ai::huggingface::{HuggingFaceModel, LoadedHuggingFaceModel};
use crate::man::settings;
use crate::result::{Error, Result};

pub(crate) const TEMPERATURE: f64 = 0.7;
pub(crate) const REPEAT_PENALTY: f32 = 1.1;
pub(crate) const REPEAT_LAST_N: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "id", content = "model")]
pub(crate) enum TextGenerationProvider {
    HuggingFace(HuggingFaceModel),
    /// 兼容 2026-09-17 之前存的记录（那时这个变体叫 `OpenAI`）。
    /// 等所有实例都保存过一次设置后即可删掉这行。
    #[serde(alias = "OpenAI")]
    OpenAICompatible(String),
    Ollama(String),
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Prompt {
    pub(crate) role: String,
    pub(crate) content: String,
}

static LOADED_MODELS: LazyLock<Mutex<HashMap<String, LoadedHuggingFaceModel>>> =
    LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));

// pub(crate) fn replace_model_cache(robot_id: &str, m: &HuggingFaceModel) -> Result<()> {
//     let info = m.get_info();
//     match info.model_type {
//         HuggingFaceModelType::Llama => super::llama::replace_model_cache(robot_id, &info),
//         HuggingFaceModelType::Gemma => super::gemma::replace_model_cache(robot_id, &info),
//         HuggingFaceModelType::Phi3 => super::phi3::replace_model_cache(robot_id, &info),
//         HuggingFaceModelType::Bert => Err(Error::ErrorWithMessage(format!(
//             "Unsuported model type {:?}.",
//             &info.model_type
//         ))),
//     }
// }

pub(crate) fn replace_model_cache(robot_id: &str, m: &HuggingFaceModel) -> Result<()> {
    let m = LoadedHuggingFaceModel::load(m)?;
    let mut r = LOADED_MODELS.lock()?;
    r.insert(String::from(robot_id), m);
    Ok(())
}

pub(crate) async fn completion(
    robot_id: &str,
    prompt: &str,
    sender: UnboundedSender<crate::flow::rt::dto::StreamingResponseData>,
) -> Result<()> {
    if let Some(settings) = settings::get_settings(robot_id).await? {
        log::info!("{:?}", &settings.text_generation_provider.provider);
        match settings.text_generation_provider.provider {
            TextGenerationProvider::HuggingFace(m) => {
                huggingface(
                    robot_id,
                    &m,
                    prompt,
                    settings.text_generation_provider.max_response_token_length as usize,
                    sender,
                )
                .await?;
                Ok(())
            }
            TextGenerationProvider::OpenAICompatible(m) => {
                open_ai_compatible(
                    &m,
                    prompt,
                    &settings.text_generation_provider.api_url,
                    &settings.text_generation_provider.api_key,
                    settings.text_generation_provider.max_response_token_length,
                    settings.text_generation_provider.connect_timeout_millis,
                    settings.text_generation_provider.read_timeout_millis,
                    &settings.text_generation_provider.proxy_url,
                    sender,
                )
                .await?;
                Ok(())
            }
            TextGenerationProvider::Ollama(m) => {
                ollama(
                    &settings.text_generation_provider.api_url,
                    &m,
                    prompt,
                    settings.text_generation_provider.connect_timeout_millis,
                    settings.text_generation_provider.read_timeout_millis,
                    &settings.text_generation_provider.proxy_url,
                    settings.text_generation_provider.max_response_token_length,
                    sender,
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

// pub(in crate::ai) fn send(sender: &Sender<String>, message: String) -> Result<()> {
//     let sender = sender.clone();
//     if let Err(e) = sender.try_send(message) {
//         match e {
//             tokio::sync::mpsc::error::TrySendError::Full(m) => Ok(sender.blocking_send(m)?),
//             tokio::sync::mpsc::error::TrySendError::Closed(_) => Err(e.into()),
//         }
//     } else {
//         Ok(())
//     }
// }

// async fn huggingface(
//     robot_id: &str,
//     m: &HuggingFaceModel,
//     prompt: &str,
//     sample_len: usize,
//     sender: &Sender<String>,
// ) -> Result<()> {
//     let info = m.get_info();
//     // log::info!("model_type={:?}", &info.model_type);
//     let new_prompt = info.convert_prompt(prompt)?;
//     match info.model_type {
//         HuggingFaceModelType::Gemma => {
//             super::gemma::gen_text(robot_id, &info, prompt, sample_len, Some(0.5), sender)
//         }
//         HuggingFaceModelType::Llama => super::llama::gen_text(
//             robot_id,
//             &info,
//             &new_prompt,
//             sample_len,
//             Some(25),
//             Some(0.5),
//             sender,
//         ),
//         HuggingFaceModelType::Phi3 => {
//             super::phi3::gen_text(robot_id, &info, prompt, sample_len, Some(0.5), sender)
//         }
//         HuggingFaceModelType::Bert => Err(Error::ErrorWithMessage(format!(
//             "Unsuported model type {:?}.",
//             &info.model_type
//         ))),
//     }
//     // Ok(())
// }

async fn huggingface(
    robot_id: &str,
    m: &HuggingFaceModel,
    prompt: &str,
    sample_len: usize,
    sender: UnboundedSender<crate::flow::rt::dto::StreamingResponseData>,
) -> Result<()> {
    let info = m.get_info();
    // log::info!("model_type={:?}", &info.model_type);
    let new_prompt = info.convert_prompt(prompt, None)?;
    let mut model = LOADED_MODELS.lock().unwrap_or_else(|e| {
        log::warn!("{:#?}", &e);
        e.into_inner()
    });
    if !model.contains_key(robot_id) {
        let r = LoadedHuggingFaceModel::load(m)?;
        model.insert(String::from(robot_id), r);
    };
    let loaded_model = model.get(robot_id).unwrap();
    // This endpoint streams everything it generates, so the accumulated answer
    // is never needed; `push_delta` just keeps it for the sake of the shared
    // interface.
    let mut collected = String::new();
    let mut result_sender =
        ResultSender::ChannelSender(SenderWrapper::new(sender, 0), &mut collected);
    match loaded_model {
        LoadedHuggingFaceModel::Gemma(m) => super::gemma::gen_text(
            &m.0,
            &m.1,
            &m.2,
            prompt,
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
            prompt,
            sample_len,
            Some(0.5),
            &mut result_sender,
        ),
        LoadedHuggingFaceModel::Bert(_m) => Err(Error::WithMessage(format!(
            "Unsuported model type {:?}.",
            &info.model_type
        ))),
        &LoadedHuggingFaceModel::Moondream(_) => todo!(),
    }
    // Ok(())
}

async fn open_ai_compatible(
    m: &str,
    s: &str,
    api_url: &str,
    api_key: &str,
    max_response_token_length: u32,
    connect_timeout_millis: u32,
    read_timeout_millis: u32,
    proxy_url: &str,
    sender: UnboundedSender<crate::flow::rt::dto::StreamingResponseData>,
) -> Result<()> {
    let client = crate::external::http::get_client(
        connect_timeout_millis.into(),
        read_timeout_millis.into(),
        proxy_url,
    )?;
    // 只发 user 消息。这里原来还塞了一条内容为字面量 "system_hint" 的
    // system 消息（一个从没被替换掉的占位串），等于每次请求都在告诉模型
    // 它的系统提示词是 "system_hint"。文本生成节点的 prompt 是调用方自己
    // 拼的、自包含的，不需要额外指令。
    let mut message = Map::new();
    message.insert(String::from("role"), Value::from("user"));
    message.insert(String::from("content"), Value::from(s));
    let messages = Value::Array(vec![message.into()]);
    let mut map = Map::new();
    map.insert(String::from("model"), Value::from(m));
    map.insert(String::from("messages"), messages);
    map.insert(String::from("stream"), Value::Bool(true));
    map.insert(
        String::from("max_tokens"),
        Value::from(max_response_token_length),
    );
    let obj = Value::Object(map);
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
        let body = res.text().await.unwrap_or_default();
        return Err(Error::WithMessage(format!(
            "OpenAI-compatible endpoint {u} returned {status}: {body}"
        )));
    }
    let sender_wrapper = SenderWrapper::new(sender, 0);
    let mut deltas = DeltaStream::new(DeltaFormat::OpenAi);
    let mut stream = res.bytes_stream();
    while let Some(item) = stream.next().await {
        for delta in deltas.push(item?.as_ref())? {
            if !sender_wrapper.send(delta) {
                log::warn!("OpenAI stream receiver is gone, stopping generation.");
                return Ok(());
            }
        }
        if deltas.is_done() {
            break;
        }
    }
    for delta in deltas.finish()? {
        if !sender_wrapper.send(delta) {
            return Ok(());
        }
    }
    Ok(())
}

async fn ollama(
    u: &str,
    m: &str,
    s: &str,
    connect_timeout_millis: u32,
    read_timeout_millis: u32,
    proxy_url: &str,
    sample_len: u32,
    sender: UnboundedSender<crate::flow::rt::dto::StreamingResponseData>,
) -> Result<()> {
    let prompts: Vec<Prompt> = serde_json::from_str(s)?;
    let mut prompt = String::with_capacity(32);
    for p in prompts.iter() {
        if p.role.eq("user") {
            prompt.push_str(&p.content);
            break;
        }
    }
    if prompt.is_empty() {
        return Ok(());
    }
    let client = crate::external::http::get_client(
        connect_timeout_millis.into(),
        read_timeout_millis.into(),
        proxy_url,
    )?;
    let mut map = Map::new();
    map.insert(String::from("prompt"), Value::String(prompt));
    map.insert(String::from("model"), Value::String(String::from(m)));
    map.insert(String::from("stream"), Value::Bool(true));

    let mut num_predict = Map::new();
    num_predict.insert(String::from("num_predict"), Value::from(sample_len));

    map.insert(String::from("options"), Value::from(num_predict));
    let obj = Value::Object(map);
    let body = serde_json::to_string(&obj)?;
    // log::info!("Request Ollama body {} to {}", &body, u);
    let req = client.post(u).body(body);
    let res = req.send().await?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(Error::WithMessage(format!(
            "Ollama endpoint returned {status}: {body}"
        )));
    }
    let sender_wrapper = SenderWrapper::new(sender, 0);
    let mut deltas = DeltaStream::new(DeltaFormat::OllamaGenerate);
    let mut stream = res.bytes_stream();
    while let Some(item) = stream.next().await {
        for delta in deltas.push(item?.as_ref())? {
            if !sender_wrapper.send(delta) {
                log::warn!("Ollama stream receiver is gone, stopping generation.");
                return Ok(());
            }
        }
        if deltas.is_done() {
            break;
        }
    }
    for delta in deltas.finish()? {
        if !sender_wrapper.send(delta) {
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ai::test_support::{drain, serve_capturing, sse_event, sse_head};

    use super::*;

    /// This path used to be wrong in four ways at once: the URL was hardcoded
    /// to OpenAI's, the key was an empty `Bearer ` (so it could never succeed
    /// against a real endpoint), a literal `system_hint` placeholder was sent
    /// as the system message, and the configured token cap was dropped.
    #[tokio::test]
    async fn open_ai_compatible_sends_url_key_and_token_cap() {
        let body = format!("{}data: [DONE]\n\n", sse_event("hi"));
        let (url, request) =
            serve_capturing("/custom/llm/chat/completions", sse_head(), &body).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        open_ai_compatible(
            "deepseek-chat",
            "hi",
            &url,
            "test-key",
            4_242,
            3_325,
            9_925,
            "",
            tx,
        )
        .await
        .unwrap();
        assert_eq!(drain(&mut rx), vec!["hi"]);

        let request = request.await.unwrap();
        assert!(
            request.starts_with("POST /custom/llm/chat/completions "),
            "the configured path must be used, not a hardcoded one: {request}"
        );
        // Header names arrive lowercased over HTTP/1.1.
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer test-key"),
            "the configured key must be sent: {request}"
        );
        let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
        assert!(
            body.contains("\"max_tokens\":4242"),
            "the configured token cap must be sent: {body}"
        );
        assert!(
            !body.contains("system_hint"),
            "the placeholder system message must be gone: {body}"
        );
    }
}
