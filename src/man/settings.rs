use std::collections::HashMap;
use std::default::Default;
use std::net::SocketAddr;
use std::sync::{LazyLock, Mutex};
use std::vec::Vec;

use axum::Json;
use axum::body::Bytes;
use axum::extract::Query;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ai::huggingface::HuggingFaceModel;
use crate::ai::{asr, chat, embedding, huggingface, tts};
use crate::db;
use crate::db_executor;
use crate::result::{Error, Result};
use crate::robot::dto::RobotQuery;
use crate::web::server::{self, to_res};

/// 全局设置表 —— **跨机器人**，只有它和机器人注册表是全局的。
///
/// 名字保持 `settings`：全局的那几个键（[`SETTINGS_KEY`] / `db_init_time` /
/// `version`）原来就住在这里。
pub(crate) const TABLE: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("settings");
/// 每机器人设置表的后缀，表名是 `{robot_id}settings`。
///
/// 原来每机器人的设置以 `robot_id` 为键混在上面的全局表里，是仓库里唯一一张
/// GLOBAL 与 PER-ROBOT 混住的表；拆开以后机器人导出/删除都只碰自己那张。
pub(crate) const TABLE_SUFFIX: &str = "settings";
pub(crate) const SETTINGS_KEY: &str = "global-settings";

static SETTINGS_CACHE: LazyLock<Mutex<HashMap<String, Settings>>> =
    LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));

#[derive(Deserialize, Serialize)]
pub(crate) struct HfModelDownload {
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "accessToken")]
    pub(crate) access_token: String,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct GlobalSettings {
    pub(crate) ip: String,
    pub(crate) port: u16,
    #[serde(rename = "selectRandomPortWhenConflict")]
    pub(crate) select_random_port_when_conflict: bool,
    #[serde(rename = "hfModelDownload")]
    pub(crate) hf_model_download: HfModelDownload,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Settings {
    settings_version: u8,
    #[serde(rename = "maxSessionIdleSec")]
    pub(crate) max_session_idle_sec: u32,
    #[serde(rename = "chatProvider")]
    pub(crate) chat_provider: ChatProvider,
    #[serde(rename = "sentenceEmbeddingProvider")]
    pub(crate) sentence_embedding_provider: SentenceEmbeddingProvider,
    #[serde(rename = "asrProvider")]
    pub(crate) asr_provider: AsrProvider,
    #[serde(rename = "ttsProvider")]
    pub(crate) tts_provider: TtsProvider,
    #[serde(rename = "smtpHost")]
    pub(crate) smtp_host: String,
    #[serde(rename = "smtpUsername")]
    pub(crate) smtp_username: String,
    #[serde(rename = "smtpPassword")]
    pub(crate) smtp_password: String,
    #[serde(rename = "smtpTimeoutSec")]
    pub(crate) smtp_timeout_sec: u16,
    #[serde(rename = "emailVerificationRegex")]
    pub(crate) email_verification_regex: String,
}

// #[test]
// fn deser() {
//     let s = Settings {
//         ip: String::new(),
//         port: 12715,
//         max_session_duration_min: 60,
//         embedding_provider: EmbeddingProvider {
//             provider: crate::intent::embedding::EmbeddingProvider::HuggingFace(
//                 crate::intent::embedding::HuggingFaceModel::AllMiniLML6V2,
//             ),
//             api_url: String::new(),
//             api_key: String::new(),
//             model: String::new(),
//             connect_timeout_millis: 1000,
//             read_timeout_millis: 5000,
//         },
//         smtp_host: String::new(),
//         smtp_username: String::new(),
//         smtp_password: String::new(),
//         smtp_timeout_sec: 30,
//         email_verification_regex: String::new(),
//         select_random_port_when_conflict: false,
//     };
//     let j = serde_json::to_string(&s);
//     assert!(j.is_ok());
//     println!("{}", j.unwrap());
//     let j = "{\"ip\":\"127.0.0.1\",\"port\":12715,\"selectRandomPortWhenConflict\":false,\"maxSessionDurationMin\":30,\"smtpHost\":\"\",\"smtpUsername\":\"\",\"smtpPassword\":\"\",\"smtpTimeoutSec\":60,\"emailVerificationRegex\":\"[-\\w\\.\\+]{1,100}@[A-Za-z0-9]{1,30}[A-Za-z\\.]{2,30}\",\"embeddingProvider\":{\"provider\":\"HuggingFace\",\"apiUrl\":\"Model will be downloaded locally at ./data/models\",\"apiKey\":\"\",\"model\":\"AllMiniLML6V2\",\"apiUrlDisabled\":true,\"showApiKeyInput\":false}}";
//     let r = serde_json::from_str(j);
//     assert!(r.is_ok());
//     let v: serde_json::Value = r.unwrap();
//     assert_eq!(v["embeddingProvider"]["provider"], "HuggingFace");
// }

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct ChatProvider {
    pub(crate) provider: chat::ChatProvider,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "maxResponseTokenLength")]
    pub(crate) max_response_token_length: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct SentenceEmbeddingProvider {
    pub(crate) provider: embedding::SentenceEmbeddingProvider,
    #[serde(rename = "similarityThreshold")]
    pub(crate) similarity_threshold: f32,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct AsrProvider {
    pub(crate) provider: asr::AsrProvider,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct TtsProvider {
    pub(crate) provider: tts::TtsProvider,
    #[serde(rename = "apiUrl")]
    pub(crate) api_url: String,
    #[serde(rename = "apiKey")]
    pub(crate) api_key: String,
    pub(crate) model: String,
    #[serde(rename = "connectTimeoutMillis")]
    pub(crate) connect_timeout_millis: u32,
    #[serde(rename = "readTimeoutMillis")]
    pub(crate) read_timeout_millis: u32,
    #[serde(rename = "proxyUrl")]
    pub(crate) proxy_url: String,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        GlobalSettings {
            ip: String::from("127.0.0.1"),
            port: 12715,
            select_random_port_when_conflict: false,
            hf_model_download: HfModelDownload {
                connect_timeout_millis: 2000,
                read_timeout_millis: 10000,
                access_token: String::new(),
            },
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            settings_version: 1u8,
            max_session_idle_sec: 1800,
            chat_provider: ChatProvider {
                provider: chat::ChatProvider::HuggingFace(
                    huggingface::HuggingFaceModel::TinyLlama1_1bChatV1_0,
                ),
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                max_response_token_length: 1000,
                proxy_url: String::new(),
            },
            sentence_embedding_provider: SentenceEmbeddingProvider {
                provider: embedding::SentenceEmbeddingProvider::HuggingFace(
                    huggingface::HuggingFaceModel::AllMiniLML6V2,
                ),
                similarity_threshold: 0.85f32,
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                proxy_url: String::new(),
            },
            asr_provider: AsrProvider {
                provider: asr::AsrProvider::HuggingFace(
                    huggingface::HuggingFaceModel::WhisperLargeV3,
                ),
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                proxy_url: String::new(),
            },
            tts_provider: TtsProvider {
                provider: tts::TtsProvider::HuggingFace(
                    huggingface::HuggingFaceModel::ParlerTtsMiniV1,
                ),
                api_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                connect_timeout_millis: 5000,
                read_timeout_millis: 10000,
                proxy_url: String::new(),
            },
            smtp_host: String::new(),
            smtp_username: String::new(),
            smtp_password: String::new(),
            smtp_timeout_sec: 60u16,
            email_verification_regex: String::new(),
        }
    }
}

pub(crate) async fn init_table(store: &db::RedbStore) -> Result<()> {
    db::init_table(store, TABLE).await
}

pub(crate) async fn exists(store: &db::RedbStore) -> Result<bool> {
    let cnt = db::count(store, TABLE).await?;
    Ok(cnt > 0)
}

pub(crate) async fn init_global(store: &db::RedbStore) -> Result<GlobalSettings> {
    let settings = GlobalSettings::default();
    db::write(store, TABLE, SETTINGS_KEY, &settings).await?;
    let format = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]]")
        .expect("Invalid format description");
    let t = time::OffsetDateTime::now_utc();
    let t_str = t
        .format(&format)
        .map_err(|e| Error::TimeFormat(Box::new(e)))?;
    db::write(store, TABLE, "db_init_time", &t_str).await?;
    db::write(store, TABLE, "version", &String::from(server::VERSION)).await?;
    Ok(settings)
}

/// 新机器人的默认设置，写在**它自己**那张表里。
pub(crate) async fn init(robot_id: &str) -> Result<Settings> {
    let settings = Settings::default();
    db_executor!(db::write, robot_id, TABLE_SUFFIX, robot_id, &settings)?;
    Ok(settings)
}

pub(crate) async fn get_global_settings(store: &db::RedbStore) -> Result<Option<GlobalSettings>> {
    db::query(store, TABLE, SETTINGS_KEY).await
}

pub(crate) async fn rest_get_global_settings() -> impl IntoResponse {
    let r = match db::store(db::StoreKey::Global).await {
        Ok(store) => get_global_settings(store).await,
        Err(e) => Err(e),
    };
    to_res(r)
}

pub(crate) async fn get_settings(robot_id: &str) -> Result<Option<Settings>> {
    // 缓存锁必须在 await 之前放开：std 的 MutexGuard 不是 Send。
    {
        let l = SETTINGS_CACHE.lock()?;
        if let Some(s) = l.get(robot_id) {
            return Ok(Some(s.clone()));
        }
    }
    let r: Option<Settings> = db_executor!(db::query, robot_id, TABLE_SUFFIX, robot_id)?;
    Ok(r.map(unify_legacy_api_urls))
}

/// 老记录里指向 Ollama **原生**端点的地址。
///
/// 原生路径这次并进了统一的 OpenAI 兼容路径，所以这些地址要挪到同一个 host 上的
/// 兼容端点：不挪的话，老记录会把 OpenAI 形状的请求发到原生端点，回来的是 NDJSON
/// 或 `{"embedding":[...]}`，表现为"应答是空的"，比报错更难查。
///
/// 只在读写设置时归一化（[`get_settings`] / [`save_settings`]），所以设置页里显示
/// 的就是真正会被使用的地址，用户下一次保存就落库。等所有实例都保存过一次之后，
/// 这个函数可以连同两个 serde 别名一起删掉。
fn unify_legacy_api_url(u: &str) -> String {
    let t = u.trim().trim_end_matches('/');
    let rewritten = ["/api/chat", "/api/embed", "/api/embeddings"]
        .iter()
        .find_map(|suffix| t.strip_suffix(suffix).map(|base| (suffix, base)));
    match rewritten {
        Some((suffix, base)) if *suffix == "/api/chat" => format!("{base}/v1/chat/completions"),
        Some((_, base)) => format!("{base}/v1/embeddings"),
        None => String::from(u),
    }
}

fn unify_legacy_api_urls(mut s: Settings) -> Settings {
    s.chat_provider.api_url = unify_legacy_api_url(&s.chat_provider.api_url);
    s.sentence_embedding_provider.api_url =
        unify_legacy_api_url(&s.sentence_embedding_provider.api_url);
    s
}

pub(crate) async fn get(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    to_res::<Option<Settings>>(get_settings(&q.robot_id).await)
}

pub(crate) async fn save(
    Query(q): Query<RobotQuery>,
    Json(data): Json<Settings>,
) -> impl IntoResponse {
    to_res(save_settings(&q.robot_id, data).await)
}

pub(crate) async fn save_global_settings(
    store: &db::RedbStore,
    data: &GlobalSettings,
) -> Result<()> {
    let addr = format!("{}:{}", data.ip, data.port);
    let _: SocketAddr = addr.parse().map_err(|_| {
        log::error!("Saving invalid listen IP: {}", &addr);
        Error::WithMessage(String::from("lang.settings.invalidIp"))
    })?;
    db::write(store, TABLE, SETTINGS_KEY, data).await
}

pub(crate) async fn rest_save_global_settings(
    Json(data): Json<GlobalSettings>,
) -> impl IntoResponse {
    let r = match db::store(db::StoreKey::Global).await {
        Ok(store) => save_global_settings(store, &data).await,
        Err(e) => Err(e),
    };
    to_res(r)
}

pub(crate) async fn save_settings(robot_id: &str, data: Settings) -> Result<()> {
    // 保存时也归一化，这样老地址会被真正写掉（读时归一化只保证运行期正确，
    // 存储里的旧值要等一次保存才能收敛）。
    let data = unify_legacy_api_urls(data);
    // 本地 HF 模型在这里就装进缓存，免得第一次对话时才现装。
    //
    // 这里原来有**两段**长得一样的代码，都去匹配 `text_generation_provider` 的
    // 本地模型：第一段把模型塞进 chat 的缓存，第二段（已经不存在的）
    // `completion::replace_model_cache`。也就是说对话模型自己反而从来没在保存时
    // 进过缓存，全靠第一次调用时懒加载。文本生成并入对话之后只剩这一段。
    if let chat::ChatProvider::HuggingFace(m) = &data.chat_provider.provider {
        if let Err(e) = chat::replace_model_cache(robot_id, m) {
            log::warn!(
                "Hugging face model files for chat were incorrect. Err: {:?}",
                &e
            );
        }
    }

    if let embedding::SentenceEmbeddingProvider::HuggingFace(m) =
        &data.sentence_embedding_provider.provider
    {
        match crate::ai::huggingface::load_bert_model_files(m.get_info().repository) {
            Ok(m) => embedding::replace_model_cache(robot_id, m),
            Err(e) => {
                log::warn!(
                    "Hugging face model files for sentence embedding were incorrect. Err: {:?}",
                    &e
                );
            }
        }
    }
    db_executor!(db::write, robot_id, TABLE_SUFFIX, robot_id, &data)?;
    let mut l = SETTINGS_CACHE.lock()?;
    l.insert(String::from(robot_id), data);
    Ok(())
}

pub(crate) async fn smtp_test(Json(settings): Json<Settings>) -> impl IntoResponse {
    to_res(check_smtp_settings(&settings))
}

pub(crate) fn check_smtp_settings(settings: &Settings) -> Result<bool> {
    use lettre::SmtpTransport;
    use lettre::transport::smtp::authentication::Credentials;
    let creds = Credentials::new(
        settings.smtp_username.to_owned(),
        settings.smtp_password.to_owned(),
    );

    let mailer = SmtpTransport::relay(&settings.smtp_host)?
        .credentials(creds)
        .timeout(Some(core::time::Duration::from_secs(
            settings.smtp_timeout_sec as u64,
        )))
        .build();

    Ok(mailer.test_connection()?)
}

pub(crate) async fn download_model_files(Json(m): Json<HuggingFaceModel>) -> impl IntoResponse {
    let store = match db::store(db::StoreKey::Global).await {
        Ok(store) => store,
        Err(e) => return to_res(Err(e)),
    };
    let global_settings = get_global_settings(store).await;
    if global_settings.is_err() {
        return to_res(Err(Error::WithMessage(String::from(
            "Load global settings failed.",
        ))));
    }
    let global_settings = global_settings.unwrap();
    if global_settings.is_none() {
        return to_res(Err(Error::WithMessage(String::from(
            "Global settings not found.",
        ))));
    }
    let global_settings = global_settings.unwrap();
    tokio::spawn(async move {
        match huggingface::download_hf_models(
            &m.get_info(),
            &global_settings.hf_model_download.access_token,
            global_settings.hf_model_download.connect_timeout_millis as u64,
            global_settings.hf_model_download.read_timeout_millis as u64,
        )
        .await
        {
            Ok(_) => log::info!("All model files download successfully."),
            Err(e) => log::error!("Model file downloaded failed, err: {:?}", &e),
        }
        // if let Some(s) = huggingface::DOWNLOAD_STATUS.get() {
        //     if let Ok(mut v) = s.lock() {
        //         v.downloading = false;
        //     }
        // }
    });
    to_res(Ok(()))
}

pub(crate) async fn download_model_progress() -> impl IntoResponse {
    let r = huggingface::get_download_status();
    to_res(Ok(r))
}

pub(crate) async fn check_model_files(bytes: Bytes) -> impl IntoResponse {
    match serde_json::from_slice::<Vec<HuggingFaceModel>>(bytes.as_ref()) {
        Ok(repositories) => {
            let mut map = Map::new();
            for model in repositories.iter() {
                let info = model.get_info();
                let r = match huggingface::check_model_files(&info) {
                    Ok(_) => true,
                    Err(e) => {
                        log::warn!(
                            "Hugging face model {} files incorrect. Err: {:?}",
                            info.repository,
                            &e
                        );
                        false
                    }
                };
                map.insert(model.to_string(), Value::from(r));
            }
            to_res(Ok(map))
        }
        Err(e) => to_res(Err(Error::WithMessage(format!(
            "Invalid request body, err {:?}",
            &e
        )))),
    }
}

pub(crate) async fn check_embedding_model(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    let r = if let Ok(r) = get_settings(&q.robot_id).await {
        if let Some(settings) = r {
            match settings.sentence_embedding_provider.provider {
                embedding::SentenceEmbeddingProvider::HuggingFace(m) => {
                    let info = m.get_info();
                    huggingface::check_model_files(&info)
                }
                embedding::SentenceEmbeddingProvider::OpenAICompatible(_) => {
                    if settings.sentence_embedding_provider.api_url.is_empty() {
                        Err(Error::WithMessage(String::from(
                            "lang.settings.apiUrlEmpty",
                        )))
                    } else if settings.sentence_embedding_provider.api_key.is_empty() {
                        Err(Error::WithMessage(String::from("OPENAI_API_KEY is empty.")))
                    } else {
                        Ok(())
                    }
                }
            }
        } else {
            Err(Error::WithMessage(String::from(
                "Can NOT find settings of this robot.",
            )))
        }
    } else {
        Err(Error::WithMessage(String::from(
            "Failed to get settings of this robot.",
        )))
    };
    to_res(r)
}

/// 列出 OpenAI 兼容端点的模型，供前端"获取模型列表"按钮使用。
///
/// 参数走 query 而不是 body：模型名现在由用户手输，拼错只会得到一个看不出
/// 原因的 404，这个接口就是为了消掉那个首次使用的障碍。
pub(crate) async fn list_openai_models(
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(url) = q.get("url") else {
        return to_res(Err(Error::WithMessage(String::from(
            "API URL parameter not found",
        ))));
    };
    if url.trim().is_empty() {
        return to_res(Err(Error::WithMessage(String::from(
            "API URL parameter is empty",
        ))));
    }
    let api_key = q.get("apiKey").map(String::as_str).unwrap_or_default();
    let proxy_url = q.get("proxyUrl").map(String::as_str).unwrap_or_default();
    let connect_timeout_millis = q
        .get("connectTimeoutMillis")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000u64);
    let read_timeout_millis = q
        .get("readTimeoutMillis")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000u64);
    to_res(
        retrieve_openai_models(
            url,
            api_key,
            connect_timeout_millis,
            read_timeout_millis,
            proxy_url,
        )
        .await,
    )
}

/// 模型列表就在用户配置的那个端点隔壁：`.../v1/chat/completions` 和
/// `.../v1/embeddings` 都在 `.../v1/models` 下。Ollama 的兼容端点形状相同，
/// 所以原生 `/api/tags` 那套 `rfind('/')` 派生方式在这里不适用。
///
/// 例外是 Ollama 的原生对话端点 `/api/chat`：它的模型列表在 `/api/tags`，
/// 不带 `/v1`（形状也不同，`retrieve_openai_models` 两种都认）。
fn models_url(u: &str) -> String {
    let base = u.trim().trim_end_matches('/');
    if let Some(base) = base.strip_suffix("/api/chat") {
        return format!("{}/api/tags", base.trim_end_matches('/'));
    }
    let base = ["/chat/completions", "/completions", "/embeddings"]
        .iter()
        .find_map(|suffix| base.strip_suffix(suffix))
        .unwrap_or(base);
    format!("{}/models", base.trim_end_matches('/'))
}

async fn retrieve_openai_models(
    url: &str,
    api_key: &str,
    connect_timeout_millis: u64,
    read_timeout_millis: u64,
    proxy_url: &str,
) -> Result<Vec<String>> {
    // 走共享 client 而不是裸 reqwest::get，这样用户的代理和超时设置才生效。
    let client = crate::external::http::get_client(
        connect_timeout_millis,
        read_timeout_millis,
        proxy_url,
    )?;
    let mut req = client.get(models_url(url));
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {api_key}"));
    }
    let res = req.send().await?;
    let status = res.status();
    let bytes = res.bytes().await?;
    if !status.is_success() {
        return Err(Error::WithMessage(format!(
            "Model list endpoint returned {status}: {}",
            String::from_utf8_lossy(bytes.as_ref())
        )));
    }
    let json: Value = serde_json::from_slice(bytes.as_ref())?;
    let mut result: Vec<String> = Vec::with_capacity(16);
    // `{"data":[{"id":...}]}` 是 OpenAI 的形状；裸数组和 Ollama 原生的
    // `{"models":[{"id":...}]}` 顺带接受，代价极小。
    let items = json
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| json.get("models").and_then(Value::as_array))
        .or_else(|| json.as_array());
    if let Some(items) = items {
        for item in items {
            if let Some(id) = item
                .get("id")
                .or_else(|| item.get("model"))
                .and_then(Value::as_str)
            {
                result.push(String::from(id));
            }
        }
    }
    result.sort();
    result.dedup();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_url_sits_next_to_the_configured_endpoint() {
        assert_eq!(
            models_url("https://api.deepseek.com/v1/chat/completions"),
            "https://api.deepseek.com/v1/models"
        );
        assert_eq!(
            models_url("https://api.openai.com/v1/embeddings"),
            "https://api.openai.com/v1/models"
        );
        // Ollama's compatibility endpoint, which is why the native `/api/tags`
        // derivation cannot be reused here.
        assert_eq!(
            models_url("http://localhost:11434/v1/chat/completions"),
            "http://localhost:11434/v1/models"
        );
        // Ollama's native chat endpoint instead: the model list is `/api/tags`.
        assert_eq!(
            models_url("http://localhost:11434/api/chat"),
            "http://localhost:11434/api/tags"
        );
        assert_eq!(
            models_url("http://192.168.1.9:11434/api/chat/"),
            "http://192.168.1.9:11434/api/tags"
        );
        // Trailing slashes and stray whitespace are the common paste accidents.
        assert_eq!(
            models_url("  https://api.moonshot.cn/v1/chat/completions/  "),
            "https://api.moonshot.cn/v1/models"
        );
        // A bare host is a documented wrong guess: the field stays editable.
        assert_eq!(
            models_url("https://api.deepseek.com"),
            "https://api.deepseek.com/models"
        );
    }

    /// 老记录里指向 Ollama 原生端点的地址必须被挪到兼容端点，否则老机器人会在
    /// 运行期把 OpenAI 形状的请求打过去，表现为"应答是空的"。
    #[test]
    fn legacy_ollama_urls_are_moved_to_the_compatible_endpoints() {
        assert_eq!(
            unify_legacy_api_url("http://localhost:11434/api/chat"),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            unify_legacy_api_url("  http://192.168.1.9:11434/api/chat/  "),
            "http://192.168.1.9:11434/v1/chat/completions"
        );
        assert_eq!(
            unify_legacy_api_url("http://localhost:11434/api/embeddings"),
            "http://localhost:11434/v1/embeddings"
        );
        // `/api/embed` 是同一个原生端点的较新名字（请求体是 `input`）。
        assert_eq!(
            unify_legacy_api_url("http://localhost:11434/api/embed"),
            "http://localhost:11434/v1/embeddings"
        );

        // 已经是兼容端点、别的厂商、以及空地址都原样不动。
        for u in [
            "http://localhost:11434/v1/chat/completions",
            "https://api.deepseek.com/v1/chat/completions",
            "http://localhost:11434/v1/embeddings",
            "",
        ] {
            assert_eq!(unify_legacy_api_url(u), u, "{u} must be left alone");
        }
        // 只有**结尾**是那个路径才动；带查询串的不猜。
        assert_eq!(
            unify_legacy_api_url("http://localhost:11434/api/chat?x=1"),
            "http://localhost:11434/api/chat?x=1"
        );
    }
}
