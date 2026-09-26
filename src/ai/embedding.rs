// use std::collections::VecDeque;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::vec::Vec;

use candle::{IndexOp, Tensor};
use candle_transformers::models::bert::BertModel;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokenizers::Tokenizer;

use super::huggingface::{HuggingFaceModel, HuggingFaceModelInfo, load_bert_model_files};
use crate::man::settings;
use crate::result::{Error, Result};

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "id", content = "model")]
pub(crate) enum SentenceEmbeddingProvider {
    HuggingFace(HuggingFaceModel),
    /// 兼容老记录的两个名字：
    /// - `OpenAI`：2026-09-17 之前这个变体叫这个名字；
    /// - `Ollama`：UI 里曾经有第三个"Ollama"选项，指向 Ollama 的原生
    ///   `/api/embeddings`。原生路径已经并进这条统一的兼容路径，老记录里的那种
    ///   地址由 `settings::unify_legacy_api_url` 在读写设置时挪到同一个 host 上的
    ///   `/v1/embeddings`（形状也不同：`prompt` + `embedding` vs
    ///   `input` + `data[].embedding`）。
    ///
    /// 两个别名都可以在所有实例保存过一次设置后删掉。
    #[serde(alias = "OpenAI", alias = "Ollama")]
    OpenAICompatible(String),
}

pub(crate) async fn embedding(robot_id: &str, s: &str) -> Result<(Vec<f32>, f32)> {
    if let Some(settings) = settings::get_settings(robot_id).await? {
        let v = match settings.sentence_embedding_provider.provider {
            SentenceEmbeddingProvider::HuggingFace(m) => hugging_face(robot_id, &m.get_info(), s),
            SentenceEmbeddingProvider::OpenAICompatible(m) => open_ai_compatible(
                &m,
                s,
                &settings.sentence_embedding_provider.api_url,
                &settings.sentence_embedding_provider.api_key,
                settings.sentence_embedding_provider.connect_timeout_millis,
                settings.sentence_embedding_provider.read_timeout_millis,
                &settings.sentence_embedding_provider.proxy_url,
            )
            .await,
        }?;
        Ok((v, settings.sentence_embedding_provider.similarity_threshold))
    } else {
        Err(Error::WithMessage(format!(
            "Can not find settings of {}",
            robot_id
        )))
    }
}

static EMBEDDING_MODEL: OnceLock<Mutex<HashMap<String, (BertModel, Tokenizer)>>> = OnceLock::new();

pub(crate) fn replace_model_cache(robot_id: &str, c: (BertModel, Tokenizer)) {
    if let Some(lock) = EMBEDDING_MODEL.get() {
        if let Ok(mut cache) = lock.lock() {
            cache.insert(String::from(robot_id), c);
        }
    }
}

fn hugging_face(robot_id: &str, info: &HuggingFaceModelInfo, s: &str) -> Result<Vec<f32>> {
    let lock = EMBEDDING_MODEL.get_or_init(|| Mutex::new(HashMap::with_capacity(32)));
    let mut model = lock.lock().unwrap_or_else(|e| {
        log::warn!("{:#?}", &e);
        e.into_inner()
    });
    if !model.contains_key(robot_id) {
        let r = load_bert_model_files(info)?;
        model.insert(String::from(robot_id), r);
    };
    let (m, t) = model.get_mut(robot_id).unwrap();
    // let tokenizer = match t.with_padding(None).with_truncation(None) {
    //     Ok(t) => t,
    //     Err(e) => return Err(Error::ErrorWithMessage(format!("{}", &e))),
    // };
    let tokens = match t.encode(s, true) {
        Ok(t) => t.get_ids().to_vec(),
        Err(e) => return Err(Error::WithMessage(format!("{}", &e))),
    };
    let token_ids = Tensor::new(&tokens[..], &m.device)?.unsqueeze(0)?;
    let token_type_ids = token_ids.zeros_like()?;
    // following attention_mask parameter is needed when batch inputs are of different token length
    // let attention_mask = tokens
    //     .iter()
    //     .map(|tokens| {
    //         let tokens = tokens.get_attention_mask().to_vec();
    //         Ok(Tensor::new(tokens.as_slice(), device)?)
    //     })
    //     .collect::<Result<Vec<_>>>()?;
    let outputs = m.forward(&token_ids, &token_type_ids, None)?;
    let (_n_sentence, n_tokens, _hidden_size) = outputs.dims3()?;
    let embeddings = (outputs.sum(1)? / (n_tokens as f64))?;
    let embeddings = embeddings.broadcast_div(&embeddings.sqr()?.sum_keepdim(1)?.sqrt()?)?;
    let r = embeddings.i(0)?.to_vec1::<f32>()?;
    Ok(r)
}

// fn tt() {
//     let prs = vec![0.1f32,0.1f32,0.1f32,0.1f32,];
//     let mut top: Vec<_> = prs.iter().enumerate().collect();
//     top.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
//     let top = top.into_iter().take(5).collect::<Vec<_>>();

//     for &(i, p) in &top {
//         println!(
//             "{:50}: {:.2}%",
//             i,
//             p * 100.0
//         );
//     }
// }

async fn open_ai_compatible(
    m: &str,
    s: &str,
    api_url: &str,
    api_key: &str,
    connect_timeout_millis: u32,
    read_timeout_millis: u32,
    proxy_url: &str,
) -> Result<Vec<f32>> {
    let client = crate::external::http::get_client(
        connect_timeout_millis.into(),
        read_timeout_millis.into(),
        proxy_url,
    )?;
    let mut map = Map::new();
    map.insert(String::from("input"), Value::String(String::from(s)));
    map.insert(String::from("model"), Value::String(String::from(m)));
    let obj = Value::Object(map);
    let authorization = format!("Bearer {api_key}");
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
        .header("Authorization", &authorization)
        .body(serde_json::to_string(&obj)?);
    let res = req
        // .timeout(Duration::from_millis(60000))
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        // 这条路径原来不检查状态码，于是一个 401 或 404 会退化成看不懂的
        // JSON 解析错误。现在它要面对任意第三方端点，得把话说清楚。
        let body = res.text().await.unwrap_or_default();
        return Err(Error::WithMessage(format!(
            "OpenAI-compatible endpoint {u} returned {status}: {body}"
        )));
    }
    let r = res.text().await?;
    let v: Value = serde_json::from_str(&r)?;
    let mut embedding_result: Vec<f32> = Vec::with_capacity(3072);
    if let Some(d) = v["data"].as_array() {
        for item in d.iter() {
            if let Some(embedding) = item["embedding"].as_array() {
                for e in embedding.iter() {
                    if let Some(n) = e.as_number() {
                        if let Some(num) = n.as_f64() {
                            let s = format!("{num:.9}");
                            embedding_result.push(s.parse::<f32>()?);
                        }
                    }
                }
            }
        }
    }
    Ok(embedding_result)
}

pub(crate) fn vec_to_db(v: &Vec<f32>) -> turso::Value {
    turso::Value::Blob(
        v.iter()
            .map(|f| f.to_le_bytes())
            .flatten()
            .collect::<Vec<u8>>(),
    )
}

#[cfg(test)]
mod tests {
    use crate::ai::test_support::serve_capturing;

    use super::*;

    /// The URL used to be hardcoded to OpenAI's, so pointing this provider at
    /// anything else silently queried the wrong host. It also never checked the
    /// status code, turning a 401 into an unreadable JSON error.
    #[tokio::test]
    async fn open_ai_compatible_sends_url_and_key() {
        let body = String::from(r#"{"data":[{"embedding":[0.5,0.25]}]}"#);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let (url, request) = serve_capturing("/custom/llm/embeddings", head, &body).await;
        let v = open_ai_compatible("bge-m3", "你好", &url, "test-key", 3_327, 9_927, "")
            .await
            .unwrap();
        assert_eq!(v, vec![0.5f32, 0.25]);

        let request = request.await.unwrap();
        assert!(
            request.starts_with("POST /custom/llm/embeddings "),
            "the configured path must be used, not a hardcoded one: {request}"
        );
        // Header names arrive lowercased over HTTP/1.1.
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer test-key"),
            "the configured key must be sent: {request}"
        );
    }

    /// A rejection has to surface as itself, not as a parse error.
    #[tokio::test]
    async fn open_ai_compatible_reports_a_http_error() {
        let body = String::from(r#"{"error":{"message":"bad key"}}"#);
        let head = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let (url, _request) = serve_capturing("/custom/llm/embeddings", head, &body).await;
        let e = open_ai_compatible("bge-m3", "hi", &url, "test-key", 3_329, 9_929, "")
            .await
            .unwrap_err();
        let e = format!("{e:?}");
        assert!(e.contains("401"), "the error should name the status: {e}");
        assert!(e.contains("bad key"), "the error should carry the body: {e}");
    }
}
