use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::vec::Vec;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::stream::DeltaStream;
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
    /// - `Ollama`：UI 里曾经有第三个"Ollama"选项，它指向 Ollama 的原生端点
    ///   （`/api/chat`）。原生路径已经并进这条统一的 OpenAI 兼容路径，老记录里
    ///   的那种地址由 `settings::unify_legacy_api_url` 在读写设置时挪到同一个
    ///   host 上的 `/v1/chat/completions`，所以请求形状仍然正确。
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
                open_ai_compatible(
                    &m,
                    chat_history,
                    media,
                    &settings.chat_provider.api_url,
                    &settings.chat_provider.api_key,
                    settings.chat_provider.max_response_token_length,
                    connect_timeout.unwrap_or(settings.chat_provider.connect_timeout_millis),
                    read_timeout.unwrap_or(settings.chat_provider.read_timeout_millis),
                    &settings.chat_provider.proxy_url,
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
        LoadedHuggingFaceModel::Qwen3((device, model, tokenizer)) => super::qwen3::gen_text(
            device,
            model,
            tokenizer,
            &new_prompt,
            sample_len,
            Some(0.5),
            &mut result_sender,
        ),
        LoadedHuggingFaceModel::Qwen3Moe((device, model, tokenizer)) => {
            super::qwen3::gen_text_moe(
                device,
                model,
                tokenizer,
                &new_prompt,
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
        let mut deltas = DeltaStream::new();
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

    /// Qwen3 用 ChatML。这里锁住几个容易写错的细节：
    /// - 回合结束标记是 `<|im_end|>`，不是 llama 的 `</s>`；
    /// - 每个回合都必须闭合（漏掉 `<|im_end|>` 会让模型把下一段当成同一回合）；
    /// - 最后必须以 `<|im_start|>assistant\n` 结尾，模型才知道该它说话了。
    ///
    /// 期望值对照 HF 上 `Qwen/Qwen3-0.6B` 的 `chat_template` 输出。history 就是
    /// 完整的消息列表（含最新那条 user）—— `chat()` 与 `gen_text()` 都是这么传的
    /// （`s` 为空串）。
    #[test]
    fn qwen3_prompt_is_chatml_and_stays_closed() {
        let info = HuggingFaceModel::Qwen3_0_6B.get_info();
        let history = vec![
            Prompt {
                role: String::from("system"),
                content: String::from("你是客服"),
            },
            Prompt {
                role: String::from("user"),
                content: String::from("你好"),
            },
            Prompt {
                role: String::from("assistant"),
                content: String::from("你好，有什么可以帮你？"),
            },
            Prompt {
                role: String::from("user"),
                content: String::from("退货运费谁出"),
            },
        ];
        assert_eq!(
            info.convert_prompt("", Some(history)).unwrap(),
            "<|im_start|>system\n你是客服<|im_end|>\n\
             <|im_start|>user\n你好<|im_end|>\n\
             <|im_start|>assistant\n你好，有什么可以帮你？<|im_end|>\n\
             <|im_start|>user\n退货运费谁出<|im_end|>\n\
             <|im_start|>assistant\n"
        );
    }

    /// 没有历史时，`user` 为空必须**跳过**整个 user 回合：`<|im_start|>user\n
    /// <|im_end|>` 不是 Qwen3 模板会产出的形状，而 `chat()` 的第一轮就是
    /// "history 里有 user + `s` 为空"，不注意就会多送一个空回合。
    #[test]
    fn qwen3_prompt_skips_an_empty_user_turn() {
        let info = HuggingFaceModel::Qwen3_8B.get_info();
        assert_eq!(
            info.convert_prompt("", None).unwrap(),
            "<|im_start|>assistant\n"
        );
        // 只有 system、没有 user 时同理，也不能留下空回合。
        let history = vec![Prompt {
            role: String::from("system"),
            content: String::from("你是客服"),
        }];
        assert_eq!(
            info.convert_prompt("", Some(history)).unwrap(),
            "<|im_start|>system\n你是客服<|im_end|>\n<|im_start|>assistant\n"
        );
    }

    /// MoE 档位走同一套 ChatML；两条分支不能有一条漏掉。
    #[test]
    fn qwen3_moe_uses_the_same_chatml() {
        let dense = HuggingFaceModel::Qwen3_0_6B.get_info();
        let moe = HuggingFaceModel::Qwen3_30B_A3B_Instruct_2507.get_info();
        let history = Some(vec![Prompt {
            role: String::from("user"),
            content: String::from("hi"),
        }]);
        assert_eq!(
            dense.convert_prompt("", history.clone()).unwrap(),
            moe.convert_prompt("", history).unwrap()
        );
    }

    /// GGUF 模型是**双仓库**的：权重在 unsloth 的 GGUF 仓库，分词器在官方
    /// base 仓库（GGUF 仓库里没有 tokenizer.json，已核对过文件清单）。
    /// 这里锁住两个仓库名，防止有人"顺手统一"成同一个而把下载跑成 404。
    #[test]
    fn qwen3_reads_weights_and_tokenizer_from_two_repositories() {
        let info = HuggingFaceModel::Qwen3_4B.get_info();
        assert_eq!(info.tokenizer_repository(), "Qwen/Qwen3-4B");
        assert_eq!(
            info.gguf_model_path().unwrap(),
            "./data/hf_hub/unsloth/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf"
        );
        // 非 GGUF 模型不受影响：分词器仓库回退到权重仓库。
        let bert = HuggingFaceModel::AllMiniLML6V2.get_info();
        assert!(bert.gguf_model_path().is_none());
        assert_eq!(
            bert.tokenizer_repository(),
            "sentence-transformers/all-MiniLM-L6-v2"
        );
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

    /// 视觉消息的形状：OpenAI 的 content parts，图片是 `data:` URI。
    ///
    /// 这是我们和 Ollama 兼容端点之间唯一的"非默认"约定，所以值得锁住：官方
    /// 兼容性文档标明 Vision ✓ 且只支持 base64（不支持图片 URL），上游
    /// `openai/openai.go` 的 `decodeImageURL` 也只认
    /// `data:image/{png,jpg,jpeg,webp};base64,`。
    #[tokio::test]
    async fn vision_goes_out_as_content_parts_with_a_data_uri() {
        let body = format!("{}data: [DONE]\n\n", sse_event("ok"));
        let (url, request) = serve_capturing("/v1/chat/completions", sse_head(), &body).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut answer = String::new();
        let media = crate::ai::dto::UserMediaData {
            images: vec![String::from("data:image/png;base64,QUJD")],
        };
        open_ai_compatible(
            "qwen3-vl:8b",
            prompt(),
            Some(&media),
            &url,
            "test-key",
            1_000,
            3_339,
            9_939,
            "",
            ResultSender::ChannelSender(SenderWrapper::new(tx, 0), &mut answer),
        )
        .await
        .unwrap();

        let request = request.await.unwrap();
        let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
        assert!(
            body.contains(r#""image_url":{"url":"data:image/png;base64,QUJD"}"#),
            "the image must be an OpenAI content part carrying a data URI: {body}"
        );
        assert!(
            body.contains(r#""type":"text""#),
            "the text must stay next to the image: {body}"
        );
        assert!(
            !body.contains(r#""images""#),
            "the native Ollama `images` array is gone with its provider: {body}"
        );
    }

    /// 老记录必须还能读进来：反序列化失败不是"少一个选项"，而是 `get_settings`
    /// 直接返回 Err、设置页整个打不开（老 `OpenAI` 那个坑就是这么来的）。
    ///
    /// `Ollama` 这条尤其要紧：UI 里已经没有这个选项，但历史记录存的是
    /// `{"id":"Ollama"}` 加上 `apiUrl = http://localhost:11434/api/chat`。
    /// 别名接过来之后，地址由 `settings::unify_legacy_api_url` 在读写设置时
    /// 挪到同一个 host 上的 `/v1/chat/completions`，所以请求形状仍然正确。
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
