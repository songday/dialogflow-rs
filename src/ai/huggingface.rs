use core::time::Duration;
use std::fs::OpenOptions as StdOpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::vec::Vec;

use candle::{DType, Device};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use candle_transformers::models::gemma::{Config as GemmaConfig, Model as GemmaModel};
use candle_transformers::models::llama::{Cache as LlamaCache, Llama, LlamaConfig, LlamaEosToks};
use candle_transformers::models::moondream::{Config as MoondreamConfig, Model as MoondreamModel};
use candle_transformers::models::parler_tts::{Config as ParlerTtsConfig, Model as ParlerTtsModel};
use candle_transformers::models::phi3::{Config as Phi3Config, Model as Phi3};
use candle_transformers::models::quantized_qwen3::ModelWeights as Qwen3;
use candle_transformers::models::quantized_qwen3_moe::GGUFQWenMoE as Qwen3Moe;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use tokenizers::{AddedToken, PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use crate::result::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum HuggingFaceModel {
    AllMiniLML6V2,
    ParaphraseMLMiniLML12V2,
    ParaphraseMLMpnetBaseV2,
    BgeSmallEnV1_5,
    BgeBaseEnV1_5,
    BgeLargeEnV1_5,
    BgeM3,
    NomicEmbedTextV1_5,
    MultilingualE5Small,
    MultilingualE5Base,
    MultilingualE5Large,
    MxbaiEmbedLargeV1,
    Phi3Mini4kInstruct,
    TinyLlama1_1bChatV1_0,
    Gemma2bInstruct,
    Gemma7bInstruct,
    Moondream2,
    ParlerTtsMiniV1,
    ParlerTtsLargeV1,
    WhisperLargeV3,
    // 本地千问（Qwen3）。权重与分词器分别来自两个仓库，见
    // `HuggingFaceModelInfo::tokenizer_repository`。
    Qwen3_0_6B,
    Qwen3_1_7B,
    Qwen3_4B,
    Qwen3_8B,
    Qwen3_14B,
    Qwen3_32B,
    Qwen3_30B_A3B_Instruct_2507,
}

pub(crate) enum LoadedHuggingFaceModel {
    Bert((BertModel, Tokenizer)),
    Llama((Device, Llama, LlamaCache, Tokenizer, Option<LlamaEosToks>)),
    Gemma((Device, GemmaModel, Tokenizer)),
    Phi3((Device, Phi3, Tokenizer)),
    Moondream((Device, MoondreamModel, Tokenizer)),
    Qwen3((Device, Qwen3, Tokenizer)),
    Qwen3Moe((Device, Qwen3Moe, Tokenizer)),
}

impl LoadedHuggingFaceModel {
    pub(super) fn load(m: &HuggingFaceModel) -> Result<LoadedHuggingFaceModel> {
        let info = m.get_info();
        let m = match info.model_type {
            HuggingFaceModelType::Llama => {
                LoadedHuggingFaceModel::Llama(load_llama_model_files(&info)?)
            }
            HuggingFaceModelType::Gemma => {
                LoadedHuggingFaceModel::Gemma(load_gemma_model_files(&info)?)
            }
            HuggingFaceModelType::Phi3 => {
                LoadedHuggingFaceModel::Phi3(load_phi3_model_files(&info)?)
            }
            HuggingFaceModelType::Moondream => {
                LoadedHuggingFaceModel::Moondream(load_moondream_model_files(&info)?)
            }
            HuggingFaceModelType::Bert => {
                LoadedHuggingFaceModel::Bert(load_bert_model_files(info.tokenizer_repository())?)
            }
            HuggingFaceModelType::Qwen3 => {
                LoadedHuggingFaceModel::Qwen3(super::qwen3::load_qwen3_model_files(&info)?)
            }
            HuggingFaceModelType::Qwen3Moe => {
                LoadedHuggingFaceModel::Qwen3Moe(super::qwen3::load_qwen3_moe_model_files(&info)?)
            }
        };
        Ok(m)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) enum HuggingFaceModelType {
    Bert,
    Llama,
    Gemma,
    Phi3,
    Moondream,
    Qwen3,
    Qwen3Moe,
}

// enum LoadedHfModel {
//     Bert(BertModel, Tokenizer),
//     Phi3(Phi3),
// }

pub(crate) struct HuggingFaceModelInfo {
    pub(super) model_type: HuggingFaceModelType,
    pub(crate) repository: &'static str,
    mirror: &'static str,
    model_files: Vec<&'static str>,
    model_index_file: &'static str,
    tokenizer_filename: &'static str,
    dimenssions: u32,
    /// GGUF 权重文件名（相对于 `mirror` 仓库根目录）。空串表示这个模型不是
    /// GGUF —— 那时权重由 `model_files` / `model_index_file` 描述。
    pub(super) gguf_model_filename: &'static str,
    /// 分词器与 `config.json` 所在的仓库。空串表示就在 `mirror` 里。
    ///
    /// 需要这个字段是因为 GGUF 权重仓库是**纯权重仓库**：`unsloth/*-GGUF` 与
    /// `Qwen/Qwen3-*-GGUF` 都只有 `.gguf`（外加 README/params），没有
    /// `tokenizer.json`，而 base 模型仓库（`Qwen/Qwen3-8B`）才有。上游 candle
    /// 示例也是分两处下载的。
    tokenizer_repository: &'static str,
}

impl HuggingFaceModelInfo {
    /// 分词器（以及 `config.json`）所在仓库；未单独指定时回退到权重仓库。
    pub(super) fn tokenizer_repository(&self) -> &'static str {
        if self.tokenizer_repository.is_empty() {
            self.mirror
        } else {
            self.tokenizer_repository
        }
    }

    /// GGUF 权重文件的完整本地路径。非 GGUF 模型返回 None。
    pub(super) fn gguf_model_path(&self) -> Option<String> {
        if self.gguf_model_filename.is_empty() {
            None
        } else {
            Some(construct_model_file_path(
                self.mirror,
                self.gguf_model_filename,
            ))
        }
    }

    pub(super) fn supports_vision(&self) -> bool {
        matches!(self.model_type, HuggingFaceModelType::Moondream)
    }

    pub(super) fn convert_prompt(
        &self,
        s: &str,
        history: Option<Vec<crate::ai::chat::Prompt>>,
    ) -> Result<String> {
        let mut system = String::new();
        let mut user = String::new();
        if !s.is_empty() && s.starts_with("[") {
            let mut prompts: Vec<super::chat::Prompt> = serde_json::from_str(s)?;
            for p in prompts.iter_mut() {
                if p.role.eq("system") {
                    std::mem::swap(&mut system, &mut p.content);
                } else if p.role.eq("user") {
                    std::mem::swap(&mut user, &mut p.content);
                }
            }
        }
        match self.model_type {
            HuggingFaceModelType::Bert => {
                let m = String::from("Bert model doesn't support prompt.");
                log::warn!("{}", &m);
                Err(Error::WithMessage(m))
            }
            HuggingFaceModelType::Llama => {
                let mut p = String::with_capacity(s.len());
                if !system.is_empty() {
                    p.push_str("<|system|>\n");
                    p.push_str(&system);
                    p.push_str("</s>\n");
                }
                if let Some(h) = history {
                    for i in h.iter() {
                        if i.content.is_empty() {
                            continue;
                        }
                        p.push_str("<|");
                        p.push_str(&i.role);
                        p.push_str("|>\n");
                        p.push_str(&i.content);
                        p.push_str("</s>\n");
                    }
                }
                if !user.is_empty() {
                    p.push_str("<|user|>\n");
                    p.push_str(&user);
                    p.push_str("</s>\n");
                }
                p.push_str("<|assistant|>");
                // p.push_str("<|begin_of_text|>");
                // if !system.is_empty() {
                //     p.push_str("<|start_header_id|>system<|end_header_id|>");
                //     p.push_str(&system);
                //     p.push_str("<|eot_id|>");
                // }
                // p.push_str("<|start_header_id|>user<|end_header_id|>");
                // p.push_str(&user);
                // p.push_str("<|eot_id|><|start_header_id|>assistant<|end_header_id|><|eot_id|>");
                Ok(p)
            }
            HuggingFaceModelType::Gemma => {
                let mut p = String::with_capacity(s.len());
                // p.push_str("<bos>");
                if let Some(h) = history {
                    for i in h.iter() {
                        p.push_str("<start_of_turn>");
                        if i.role.eq("assistant") {
                            p.push_str("model");
                        } else {
                            p.push_str(&i.role);
                        }
                        p.push('\n');
                        p.push_str(&i.content);
                        p.push_str("<end_of_turn>\n");
                    }
                }
                p.push_str("<start_of_turn>user\n");
                p.push_str(&user);
                p.push_str("<end_of_turn>\n<start_of_turn>model");
                Ok(p)
            }
            HuggingFaceModelType::Phi3 => {
                let mut p = String::with_capacity(s.len());
                p.push_str("<s>");
                if !system.is_empty() {
                    p.push_str("<|system|>\n");
                    p.push_str(&system);
                    p.push_str("<|end|>\n");
                }
                if let Some(h) = history {
                    for i in h.iter() {
                        p.push_str("<|");
                        p.push_str(&i.role);
                        p.push_str("|>\n");
                        p.push_str(&i.content);
                        p.push_str("<|end|>\n");
                    }
                }
                p.push_str("<|user|>\n");
                p.push_str(&user);
                p.push_str("<|end|>\n<|assistant|>");
                Ok(p)
            }
            // Qwen3 用 ChatML。和上面几个一样，只有 system 和我们自己拼的
            // assistant 开头是固定的，history 原样保留。
            //
            // 两点上游细节，故意跟随：
            // 1. 历史里的 `tool` 回合先开一个 `<|im_start|>user`；本项目的
            //    `Prompt.role` 目前只有 system/user/assistant，走不到这个分支，
            //    但保持一致，将来加角色时不会静默丢弃内容。
            // 2. 带思考模式的档位（0.6B–32B）在 `enable_thinking=false` 时，
            //    template 会在 assistant 前缀后注入 `<think>\n\n</think>\n\n`。
            //    我们不发这个指令，就用模型默认行为（思考模式开）；想关掉，
            //    在 system 提示词里加 `/no_think` 即可。
            HuggingFaceModelType::Qwen3 | HuggingFaceModelType::Qwen3Moe => {
                let mut p = String::with_capacity(s.len() + 64);
                if !system.is_empty() {
                    p.push_str("<|im_start|>system\n");
                    p.push_str(&system);
                    p.push_str("<|im_end|>\n");
                }
                if let Some(h) = history {
                    for i in h.iter() {
                        if i.content.is_empty() {
                            continue;
                        }
                        if i.role.eq("tool") {
                            p.push_str("<|im_start|>user\n<tool_response>\n");
                            p.push_str(&i.content);
                            p.push_str("\n</tool_response><|im_end|>\n");
                        } else {
                            p.push_str("<|im_start|>");
                            p.push_str(&i.role);
                            p.push('\n');
                            p.push_str(&i.content);
                            p.push_str("<|im_end|>\n");
                        }
                    }
                }
                // 两个调用方都传 `s = ""`，最新的那条 user 消息在 history 末尾，
                // 所以 `user` 通常是空的。空回合要跳过 —— `<|im_start|>user\n
                // <|im_end|>` 不是模板会产出的东西，白送一个空回合只会让模型困惑。
                // （其它分支会原样输出一个空回合，那是既有的小瑕疵，不在本次范围内。）
                if !user.is_empty() {
                    p.push_str("<|im_start|>user\n");
                    p.push_str(&user);
                    p.push_str("<|im_end|>\n");
                }
                p.push_str("<|im_start|>assistant\n");
                Ok(p)
            }
            HuggingFaceModelType::Moondream => todo!()
        }
    }
}

fn get_common_model_files() -> Vec<&'static str> {
    vec![
        "model.safetensors",
        "tokenizer.json",
        "config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    ]
}

/// GGUF 模型要下载的**非权重**文件。
///
/// 不能复用 `get_common_model_files()`：权重仓库里没有 `model.safetensors`，
/// 而 `unsloth/*-GGUF` 也没有 `special_tokens_map.json`，列进去只会拿到 404。
/// GGUF 文件本身由 `gguf_model_filename` 单独描述。
fn qwen3_model_files() -> Vec<&'static str> {
    vec!["tokenizer.json", "tokenizer_config.json", "config.json"]
}

impl HuggingFaceModel {
    pub(crate) fn get_info(&self) -> HuggingFaceModelInfo {
        match self {
            HuggingFaceModel::AllMiniLML6V2 => HuggingFaceModelInfo {
                repository: "sentence-transformers/all-MiniLM-L6-v2",
                mirror: "sentence-transformers/all-MiniLM-L6-v2",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 384,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::ParaphraseMLMiniLML12V2 => HuggingFaceModelInfo {
                repository: "sentence-transformers/paraphrase-MiniLM-L12-v2",
                mirror: "sentence-transformers/paraphrase-MiniLM-L12-v2",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 384,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::ParaphraseMLMpnetBaseV2 => HuggingFaceModelInfo {
                repository: "sentence-transformers/paraphrase-multilingual-mpnet-base-v2",
                mirror: "sentence-transformers/paraphrase-multilingual-mpnet-base-v2",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 768,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::BgeSmallEnV1_5 => HuggingFaceModelInfo {
                repository: "BAAI/bge-small-en-v1.5",
                mirror: "BAAI/bge-small-en-v1.5",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 384,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::BgeBaseEnV1_5 => HuggingFaceModelInfo {
                repository: "BAAI/bge-base-en-v1.5",
                mirror: "BAAI/bge-base-en-v1.5",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 768,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::BgeLargeEnV1_5 => HuggingFaceModelInfo {
                repository: "BAAI/bge-large-en-v1.5",
                mirror: "BAAI/bge-large-en-v1.5",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::BgeM3 => HuggingFaceModelInfo {
                repository: "BAAI/bge-m3",
                mirror: "BAAI/bge-m3",
                model_files: vec!["onnx/model.onnx", "onnx/model.onnx_data"],
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::NomicEmbedTextV1_5 => HuggingFaceModelInfo {
                repository: "nomic-ai/nomic-embed-text-v1.5",
                mirror: "nomic-ai/nomic-embed-text-v1.5",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 768,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::MultilingualE5Small => HuggingFaceModelInfo {
                repository: "intfloat/multilingual-e5-small",
                mirror: "intfloat/multilingual-e5-small",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 384,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::MultilingualE5Base => HuggingFaceModelInfo {
                repository: "intfloat/multilingual-e5-base",
                mirror: "intfloat/multilingual-e5-base",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 768,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::MultilingualE5Large => HuggingFaceModelInfo {
                repository: "intfloat/multilingual-e5-large",
                mirror: "intfloat/multilingual-e5-large",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::MxbaiEmbedLargeV1 => HuggingFaceModelInfo {
                repository: "mixedbread-ai/mxbai-embed-large-v1",
                mirror: "mixedbread-ai/mxbai-embed-large-v1",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Bert,
            },
            HuggingFaceModel::Phi3Mini4kInstruct => HuggingFaceModelInfo {
                repository: "microsoft/Phi-3-mini-4k-instruct",
                mirror: "microsoft/Phi-3-mini-4k-instruct",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Phi3,
            },
            HuggingFaceModel::TinyLlama1_1bChatV1_0 => HuggingFaceModelInfo {
                repository: "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
                mirror: "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Llama,
            },
            HuggingFaceModel::Gemma2bInstruct => HuggingFaceModelInfo {
                repository: "google/gemma-2b-it",
                mirror: "google/gemma-2b-it",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Gemma,
            },
            HuggingFaceModel::Gemma7bInstruct => HuggingFaceModelInfo {
                repository: "google/gemma-7b-it",
                mirror: "google/gemma-7b-it",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Gemma,
            },
            HuggingFaceModel::Moondream2 => HuggingFaceModelInfo {
                repository: "vikhyatk/moondream2",
                mirror: "vikhyatk/moondream2",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Moondream,
            },
            HuggingFaceModel::ParlerTtsMiniV1 => HuggingFaceModelInfo {
                repository: "parler-tts/parler-tts-mini-v1",
                mirror: "parler-tts/parler-tts-mini-v1",
                model_files: get_common_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Llama,
            },
            HuggingFaceModel::ParlerTtsLargeV1 => HuggingFaceModelInfo {
                repository: "parler-tts/parler-tts-large-v1",
                mirror: "parler-tts/parler-tts-large-v1",
                model_files: {
                    let mut v = get_common_model_files();
                    let mut idx = 0usize;
                    for &f in v.iter() {
                        if f.eq("model.safetensors") {
                            break;
                        }
                        idx += 1;
                    }
                    v.remove(idx);
                    v
                },
                model_index_file: "model.safetensors.index.json",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "",
                tokenizer_repository: "",
                model_type: HuggingFaceModelType::Gemma,
            },
            // ---- 本地千问 Qwen3（GGUF 量化） ----
            //
            // 权重来自 unsloth 的 GGUF 仓库，分词器来自官方 base 仓库 —— 前者是
            // 纯权重仓库（只有 .gguf + README/params），没有 tokenizer.json。
            // 选 unsloth 而不是 `Qwen/Qwen3-*-GGUF` 是因为量化档位全得多：
            // 官方 8B 最小只到 Q4_K_M（约 5GB），而低配机器真正能跑的
            // Q2_K / IQ4_XS / UD-Q2_K_XL 只有 unsloth 有。这与上游 candle 的
            // quantized-qwen3 示例一致。
            //
            // 这些 GGUF 都是**单文件**（分片只出现在 BF16/ 子目录和 UD-* 大分片
            // 里），这点很重要：candle 的 `from_gguf` 只接受一个 reader，不支持
            // 多分片。
            HuggingFaceModel::Qwen3_0_6B => HuggingFaceModelInfo {
                repository: "Qwen/Qwen3-0.6B",
                mirror: "unsloth/Qwen3-0.6B-GGUF",
                model_files: qwen3_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "Qwen3-0.6B-Q4_K_M.gguf",
                tokenizer_repository: "Qwen/Qwen3-0.6B",
                model_type: HuggingFaceModelType::Qwen3,
            },
            HuggingFaceModel::Qwen3_1_7B => HuggingFaceModelInfo {
                repository: "Qwen/Qwen3-1.7B",
                mirror: "unsloth/Qwen3-1.7B-GGUF",
                model_files: qwen3_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "Qwen3-1.7B-Q4_K_M.gguf",
                tokenizer_repository: "Qwen/Qwen3-1.7B",
                model_type: HuggingFaceModelType::Qwen3,
            },
            HuggingFaceModel::Qwen3_4B => HuggingFaceModelInfo {
                repository: "Qwen/Qwen3-4B",
                mirror: "unsloth/Qwen3-4B-GGUF",
                model_files: qwen3_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "Qwen3-4B-Q4_K_M.gguf",
                tokenizer_repository: "Qwen/Qwen3-4B",
                model_type: HuggingFaceModelType::Qwen3,
            },
            HuggingFaceModel::Qwen3_8B => HuggingFaceModelInfo {
                repository: "Qwen/Qwen3-8B",
                mirror: "unsloth/Qwen3-8B-GGUF",
                model_files: qwen3_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "Qwen3-8B-Q4_K_M.gguf",
                tokenizer_repository: "Qwen/Qwen3-8B",
                model_type: HuggingFaceModelType::Qwen3,
            },
            HuggingFaceModel::Qwen3_14B => HuggingFaceModelInfo {
                repository: "Qwen/Qwen3-14B",
                mirror: "unsloth/Qwen3-14B-GGUF",
                model_files: qwen3_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "Qwen3-14B-Q4_K_M.gguf",
                tokenizer_repository: "Qwen/Qwen3-14B",
                model_type: HuggingFaceModelType::Qwen3,
            },
            HuggingFaceModel::Qwen3_32B => HuggingFaceModelInfo {
                repository: "Qwen/Qwen3-32B",
                mirror: "unsloth/Qwen3-32B-GGUF",
                model_files: qwen3_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "Qwen3-32B-Q4_K_M.gguf",
                tokenizer_repository: "Qwen/Qwen3-32B",
                model_type: HuggingFaceModelType::Qwen3,
            },
            // MoE：30B 总参数 / 3B 激活。GGUF 仓库里**没有** config.json，
            // 正好由 tokenizer_repository 一起兜住。
            HuggingFaceModel::Qwen3_30B_A3B_Instruct_2507 => HuggingFaceModelInfo {
                repository: "Qwen/Qwen3-30B-A3B-Instruct-2507",
                mirror: "unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF",
                model_files: qwen3_model_files(),
                model_index_file: "",
                tokenizer_filename: "tokenizer.json",
                dimenssions: 1024,
                gguf_model_filename: "Qwen3-30B-A3B-Instruct-2507-Q4_K_M.gguf",
                tokenizer_repository: "Qwen/Qwen3-30B-A3B-Instruct-2507",
                model_type: HuggingFaceModelType::Qwen3Moe,
            },
            HuggingFaceModel::WhisperLargeV3 => todo!(),
        }
    }
}

impl std::fmt::Display for HuggingFaceModel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
        // or, alternatively:
        // fmt::Debug::fmt(self, f)
    }
}

const HUGGING_FACE_MODEL_ROOT: &str = "./data/hf_hub/";

#[derive(Clone, Serialize)]
pub(crate) struct DownloadStatus {
    pub(crate) downloading: bool,
    #[serde(rename = "totalLen")]
    pub(crate) total_len: u64,
    #[serde(rename = "downloadedLen")]
    pub(crate) downloaded_len: u64,
    pub(crate) url: String,
    pub(crate) err: String,
}

pub(crate) static DOWNLOAD_STATUS: OnceLock<Mutex<DownloadStatus>> = OnceLock::new();

pub(crate) fn get_download_status() -> Option<DownloadStatus> {
    if let Some(op) = DOWNLOAD_STATUS.get() {
        return match op.lock() {
            Ok(s) => Some(s.clone()),
            Err(e) => {
                log::error!("{:?}", &e);
                None
            }
        };
    }
    None
}

fn download_status<'l>() -> Result<std::sync::MutexGuard<'l, DownloadStatus>> {
    let locker = match DOWNLOAD_STATUS
        .get_or_init(|| {
            Mutex::new(DownloadStatus {
                downloading: false,
                total_len: 1,
                downloaded_len: 0,
                url: String::new(),
                err: String::new(),
            })
        })
        .try_lock()
    {
        Ok(l) => l,
        Err(e) => match e {
            std::sync::TryLockError::Poisoned(pe) => {
                log::warn!("{:?}", &pe);
                pe.into_inner()
            }
            std::sync::TryLockError::WouldBlock => {
                return Err(Error::WithMessage(String::from(
                    "Model files are downloading.",
                )));
            }
        },
    };
    Ok(locker)
}

pub(crate) async fn download_hf_models(
    info: &HuggingFaceModelInfo,
    huggingface_token: &str,
    connect_timeout: u64,
    read_timeout: u64,
) -> Result<()> {
    // if let Ok(v) = DOWNLOAD_STATUS
    //     .get_or_init(|| {
    //         Mutex::new(DownloadStatus {
    //             downloading: false,
    //             total_len: 1,
    //             downloaded_len: 0,
    //             url: String::new(),
    //         })
    //     })
    //     .lock()
    // {
    //     if v.downloading {
    //         return Err(Error::ErrorWithMessage(String::from(
    //             "Model files are downloading.",
    //         )));
    //     }
    // }
    {
        let mut status = download_status()?;
        if status.downloading {
            return Err(Error::WithMessage(String::from(
                "Model files are downloading.",
            )));
        }
        status.downloading = true;
    }
    let root_path = format!("{}{}", HUGGING_FACE_MODEL_ROOT, info.repository);
    tokio::fs::create_dir_all(&root_path).await?;

    let mut headers = HeaderMap::new();
    let user_agent = format!(
        "unkown/None; dialogflowai/{}; rust/unknown",
        crate::web::server::VERSION
    );
    headers.insert("User-Agent", HeaderValue::from_str(&user_agent)?);
    if !huggingface_token.is_empty() {
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {huggingface_token}"))?,
        );
    }

    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(connect_timeout))
        .read_timeout(Duration::from_millis(read_timeout))
        .default_headers(headers);
    if let Ok(proxy) = std::env::var("https_proxy") {
        if !proxy.is_empty() {
            log::info!("Detected proxy setting: {}", &proxy);
            builder = builder.proxy(reqwest::Proxy::https(&proxy)?)
        }
    }
    let client = builder.build()?;
    // 要下载的文件是 (仓库, 文件名) 而不是纯文件名：GGUF 模型的权重在本仓库、
    // 分词器在另一个仓库（本仓库里根本没有）。不比较仓库就跳过，因为分词器仓库
    // 通常已经在本仓库的 model_files 里（`tokenizer.json` / `config.json`）。
    let tokenizer_repository = info.tokenizer_repository();
    let mut files: Vec<(&str, String)> = info
        .model_files
        .iter()
        .map(|&s| (info.mirror, String::from(s)))
        .collect();
    if tokenizer_repository != info.mirror {
        for &f in info.model_files.iter() {
            files.push((tokenizer_repository, String::from(f)));
        }
    }
    if !info.gguf_model_filename.is_empty() {
        files.push((info.mirror, String::from(info.gguf_model_filename)));
    }
    let mut r: Result<_> = Ok(());
    if !info.model_index_file.is_empty() {
        let model_index_file = construct_model_file_path(info.mirror, info.model_index_file);
        let path = std::path::Path::new(&model_index_file);
        if !path.exists() {
            r = download_hf_file(&client, info.mirror, &root_path, info.model_index_file).await;
        }
        if r.is_ok() {
            let f = load_safetensors(info.mirror, info.model_index_file)?;
            files.extend(f.into_iter().map(|v| (info.mirror, v)));
        }
    };
    if r.is_ok() {
        for (repository, f) in files.iter() {
            r = download_hf_file(&client, repository, &root_path, f).await;
            if r.is_err() {
                break;
            }
        }
    }
    {
        let mut status = download_status()?;
        if r.is_err() {
            status.err = format!("Download failed,err: {:?}", r.as_ref().err());
        }
        status.downloading = false;
    }

    r
}

/// 下载 `repository` 仓库里的 `f`。仓库必须显式传入，不能假设是 `info.mirror`：
/// GGUF 模型的分词器来自另一个仓库。
async fn download_hf_file(
    client: &reqwest::Client,
    repository: &str,
    root_path: &str,
    f: &str,
) -> Result<()> {
    let file_path_str = format!("{root_path}/{f}");
    let file_path = std::path::Path::new(&file_path_str);
    if tokio::fs::try_exists(file_path).await? {
        return Ok(());
    }
    let u = format!("https://huggingface.co/{repository}/resolve/main/{f}");
    if let Some(s) = DOWNLOAD_STATUS.get() {
        if let Ok(mut v) = s.lock() {
            v.url = String::from(f);
            // 进度是**按文件**的，每个文件从头计。原来只重置上限、不重置已下载
            // 字节数，于是多文件模型（现在所有 GGUF 模型都是）的进度条会累计上
            // 一个文件的字节数，看着像卡住或倒退。
            v.downloaded_len = 0;
        }
    }
    let res = client.get(&u).query(&[("download", "true")]).send().await?;
    let status = res.status();
    if !status.is_success() {
        // 以前这里不看状态码，404 的响应体会被当成模型文件写进磁盘，直到加载
        // 时才以「GGUF 魔数不对」之类的怪错爆出来。带上仓库名，因为现在同一个
        // 模型可能来自两个仓库。
        let body = res.text().await.unwrap_or_default();
        return Err(Error::WithMessage(format!(
            "Download {repository}/{f} failed with {status}: {}",
            body.chars().take(256).collect::<String>()
        )));
    }
    let total_size = res.content_length().unwrap_or(0);
    // println!("Downloading {f}, total size {total_size}");
    if let Some(s) = DOWNLOAD_STATUS.get() {
        if let Ok(mut v) = s.lock() {
            v.total_len = total_size;
        }
    }
    // let b = res.bytes().await?;
    // fs::write("./temp.file", b.as_ref()).await?;
    // let mut downloaded = 0u64;
    let mut stream = res.bytes_stream();
    let mut file = OpenOptions::new()
        .read(false)
        .write(true)
        .truncate(false)
        .create_new(true)
        .open(file_path)
        .await?;
    // let mut file = File::create("./temp.file").await?;

    let mut downloaded = 0u64;
    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        downloaded = std::cmp::min(downloaded + (chunk.len() as u64), total_size);
        if let Some(s) = DOWNLOAD_STATUS.get() {
            if let Ok(mut v) = s.lock() {
                // log::info!("Downloaded {new}");
                v.downloaded_len = downloaded;
            }
        }
    }
    Ok(())
}

pub(super) fn construct_model_file_path(mirror: &str, f: &str) -> String {
    format!("{HUGGING_FACE_MODEL_ROOT}{mirror}/{f}")
}

pub(super) fn device() -> Result<Device> {
    if candle::utils::cuda_is_available() {
        Ok(Device::new_cuda(0)?)
    } else if candle::utils::metal_is_available() {
        Ok(Device::new_metal(0)?)
    } else {
        Ok(Device::Cpu)
    }
}

// type TokenizerImpl = tokenizers::TokenizerImpl<
//     tokenizers::ModelWrapper,
//     tokenizers::NormalizerWrapper,
//     tokenizers::PreTokenizerWrapper,
//     tokenizers::PostProcessorWrapper,
//     tokenizers::DecoderWrapper,
// >;

pub(crate) fn check_model_files(info: &HuggingFaceModelInfo) -> Result<()> {
    // let f = construct_model_file_path(repo, "config.json");
    // let config = std::fs::read(&f)?;
    // let config: serde_json::Value = serde_json::from_slice(&config)?;
    // let arch = &config["architectures"];
    // if !arch.is_array() {
    //     return Ok(false)
    // }
    // let architectures=arch.as_array().unwrap();
    // if architectures.len() <1{
    //     return Ok(false)
    // }
    // let arch=architectures.get(0).unwrap();
    // if !arch.is_string() {
    //     return Ok(false)
    // }
    // if arch.as_str().unwrap().starts_with("Bert") {
    let files = get_model_files(info)?;
    for f in files.iter() {
        let p = Path::new(f);
        if !p.exists() {
            return Err(Error::WithMessage(format!("Path {:?} is not exist.", p)));
        }
        let ext = p.extension();
        if ext.is_none() {
            return Err(Error::WithMessage(format!(
                "{:?} doesn't have extension.",
                p
            )));
        }
        let ext = ext.unwrap();
        if ext.eq("json") {
            // https://github.com/serde-rs/json/issues/160
            // let file = StdOpenOptions::new().read(true).write(false).create(false).open(&f)?;
            // let br = std::io::BufReader::with_capacity(4096, file);
            // let _ = serde_json::from_reader(br)?;
            let mut file = StdOpenOptions::new()
                .read(true)
                .write(false)
                .create(false)
                .open(f)?;
            let mut bytes = Vec::with_capacity(4096);
            file.read_to_end(&mut bytes)?;
            let _: serde::de::IgnoredAny = serde_json::from_slice(&bytes)?;
        } else if ext.eq("safetensors") {
            let metadata = std::fs::metadata(p)?;
            if metadata.len() < 62914560u64 {
                return Err(Error::WithMessage(format!(
                    "{:?} file size is too small.",
                    p
                )));
            }
        } else if ext.eq("gguf") {
            // 校验魔数。这一步不能省：下载失败时旧代码不看状态码，404 的响应体
            // 会被原样写进文件，直到 `Content::read` 才报一个和"文件不对"毫不
            // 相干的错。这里提前把话说明白。
            let mut file = StdOpenOptions::new()
                .read(true)
                .write(false)
                .create(false)
                .open(p)?;
            let mut magic = [0u8; 4];
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut magic)?;
            if &magic != b"GGUF" {
                return Err(Error::WithMessage(format!(
                    "{:?} is not a GGUF file (magic: {:?}).",
                    p, magic
                )));
            }
        } else {
            // 原来是"未知后缀静默通过"：列错文件也检查不出来。
            return Err(Error::WithMessage(format!(
                "{:?} has an unsupported extension.",
                p
            )));
        }
    }
    Ok(())
    // match info.model_type {
    //     HuggingFaceModelType::Bert => load_bert_model_files(&info.repository)
    //         .map(|_| true)
    //         .or_else(|e| {
    //             log::warn!("Check bert model files failed,err: {:?}", &e);
    //             Ok(false)
    //         }),
    //     HuggingFaceModelType::Gemma => {
    //         let device = device()?;
    //         load_gemma_model_files(&info, &device)
    //             .map(|_| true)
    //             .or_else(|e| {
    //                 log::warn!("Check gemma model files failed,err: {:?}", &e);
    //                 Ok(false)
    //             })
    //     }
    //     HuggingFaceModelType::Llama => {
    //         let device = device()?;
    //         load_llama_model_files(&info, &device)
    //             .map(|_| true)
    //             .or_else(|e| {
    //                 log::warn!("Check llama model files failed,err: {:?}", &e);
    //                 Ok(false)
    //             })
    //     }
    //     HuggingFaceModelType::Phi3 => {
    //         let device = device()?;
    //         load_phi3_model_files(&info, &device)
    //             .map(|_| true)
    //             .or_else(|e| {
    //                 log::warn!("Check phi3 model files failed,err: {:?}", &e);
    //                 Ok(false)
    //             })
    //     }
    // }
}

pub(super) fn init_tokenizer(repo: &str) -> Result<Tokenizer> {
    let f = construct_model_file_path(repo, "tokenizer.json");
    match Tokenizer::from_file(&f) {
        Ok(t) => Ok(t),
        Err(e) => Err(Error::WithMessage(format!("{}", &e))),
    }
}

fn set_tokenizer_config(
    tokenizer_repository: &str,
    mut tokenizer: Tokenizer,
    pad_token_id: u32,
) -> Result<Tokenizer> {
    let f = construct_model_file_path(tokenizer_repository, "tokenizer_config.json");
    let p = std::path::Path::new(&f);
    let t = if p.exists() {
        let j: serde_json::Value = serde_json::from_slice(std::fs::read(&f)?.as_slice())?;
        let model_max_length = j["model_max_length"]
            .as_f64()
            .expect("Error reading model_max_length from tokenizer_config.json")
            as f32;
        let max_length = 8192.min(model_max_length as usize);
        let pad_token = j["pad_token"]
            .as_str()
            .expect("Error reading pad_token from tokenier_config.json")
            .into();
        // log::info!("p1 {}", tokenizer.get_padding().unwrap().pad_token);
        // log::info!("t1 {}", tokenizer.get_truncation().unwrap().max_length);
        tokenizer
            .with_padding(Some(PaddingParams {
                strategy: PaddingStrategy::BatchLongest,
                pad_token,
                pad_id: pad_token_id,
                ..Default::default()
            }))
            .with_truncation(Some(TruncationParams {
                max_length,
                ..Default::default()
            }))
    } else {
        tokenizer.with_padding(None).with_truncation(None)
    };
    let t = match t {
        Ok(t) => t.clone().into(),
        Err(e) => {
            log::warn!("{:?}", &e);
            tokenizer
        }
    };

    Ok(t)
    // log::info!("p2 {}", tokenizer.get_padding().unwrap().pad_token);
    // log::info!("t2 {}", tokenizer.get_truncation().unwrap().max_length);
}

fn set_special_tokens_map(mirror: &str, tokenizer: &mut Tokenizer) -> Result<()> {
    let f = construct_model_file_path(mirror, "special_tokens_map.json");
    let p = std::path::Path::new(&f);
    if !p.exists() {
        return Ok(());
    }
    if let serde_json::Value::Object(root_object) =
        serde_json::from_slice(std::fs::read(&f)?.as_slice())?
    {
        for (_, value) in root_object.iter() {
            if value.is_string() {
                tokenizer.add_special_tokens([AddedToken {
                    content: value.as_str().unwrap().into(),
                    special: true,
                    ..Default::default()
                }]);
            } else if value.is_object() {
                tokenizer.add_special_tokens([AddedToken {
                    content: value["content"].as_str().unwrap().into(),
                    special: true,
                    single_word: value["single_word"].as_bool().unwrap(),
                    lstrip: value["lstrip"].as_bool().unwrap(),
                    rstrip: value["rstrip"].as_bool().unwrap(),
                    normalized: value["normalized"].as_bool().unwrap(),
                }]);
            }
        }
    }
    Ok(())
}

pub(crate) fn load_bert_model_files(tokenizer_repository: &str) -> Result<(BertModel, Tokenizer)> {
    let f = construct_model_file_path(tokenizer_repository, "config.json");
    let config = std::fs::read_to_string(&f)?;
    let config: serde_json::Value = serde_json::from_str(&config)?;
    let pad_token_id = config["pad_token_id"].as_u64().unwrap_or(0) as u32;
    let config: Config = serde_json::from_value(config)?;
    let tokenizer = init_tokenizer(tokenizer_repository)?;
    let mut tokenizer = set_tokenizer_config(tokenizer_repository, tokenizer, pad_token_id)?;
    set_special_tokens_map(tokenizer_repository, &mut tokenizer)?;
    let f = construct_model_file_path(tokenizer_repository, "model.safetensors");
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[&f], DTYPE, &device()?)? };
    let model = BertModel::load(vb, &config)?;
    Ok((model, tokenizer))
}

fn load_safetensors(mirror: &str, json_file: &str) -> Result<Vec<String>> {
    let json_file = construct_model_file_path(mirror, json_file);
    let json_file = std::fs::File::open(json_file)?;
    let json: serde_json::Value =
        serde_json::from_reader(&json_file).map_err(candle::Error::wrap)?;
    let weight_map = match json.get("weight_map") {
        None => {
            return Err(Error::WithMessage(format!(
                "no weight map in {json_file:?}"
            )));
        }
        Some(serde_json::Value::Object(map)) => map,
        Some(_) => {
            return Err(Error::WithMessage(format!(
                "weight map in {json_file:?} is not a map"
            )));
        }
    };
    let mut safetensors_files = std::collections::HashSet::new();
    for value in weight_map.values() {
        if let Some(file) = value.as_str() {
            safetensors_files.insert(file.to_string());
        }
    }
    Ok(Vec::from_iter(safetensors_files))
}

pub(crate) fn load_phi3_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, Phi3, Tokenizer)> {
    let device = device()?;
    let dtype = if device.is_cuda() {
        DType::BF16
    } else {
        DType::F32
    };
    let filenames = load_safetensors(info.mirror, info.model_index_file)?
        .iter()
        .map(|v| std::path::PathBuf::from(construct_model_file_path(info.mirror, v)))
        .collect::<Vec<_>>();
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, dtype, &device)? };
    let config_filename = construct_model_file_path(info.mirror, "config.json");
    let config = std::fs::read_to_string(config_filename)?;
    let config: Phi3Config = serde_json::from_str(&config)?;
    let phi3 = Phi3::new(&config, vb)?;
    let tokenizer = init_tokenizer(info.tokenizer_repository())?;
    Ok((device, phi3, tokenizer))
}

/// 需要校验/下载的权重文件路径。
///
/// 注意每个路径都按**它自己所属的仓库**来拼：GGUF 模型的 tokenizer 在另一个
/// 仓库，`info.model_files` 里既有本仓库的文件（`config.json`）也有 tokenizer
/// 仓库的文件（`tokenizer.json`）。
fn get_model_files(info: &HuggingFaceModelInfo) -> Result<Vec<String>> {
    let tokenizer_repository = info.tokenizer_repository();
    let mut f = if info.model_index_file.is_empty() {
        vec![construct_model_file_path(info.mirror, "model.safetensors")]
    } else {
        load_safetensors(info.repository, info.model_index_file)?
            .iter()
            .map(|v| construct_model_file_path(info.mirror, v))
            .collect::<Vec<_>>()
    };
    if !info.gguf_model_filename.is_empty() {
        // GGUF 模型没有 model.safetensors —— 上面那个占位路径要把权重换成 GGUF。
        f.clear();
        f.push(construct_model_file_path(
            info.mirror,
            info.gguf_model_filename,
        ));
    }
    for &name in info.model_files.iter() {
        let repository = if tokenizer_repository != info.mirror && is_tokenizer_file(name) {
            tokenizer_repository
        } else {
            info.mirror
        };
        f.push(construct_model_file_path(repository, name));
    }
    Ok(f)
}

/// `model_files` 里哪些属于分词器仓库。GGUF 权重仓库只有 `config.json`，
/// 其余（`tokenizer.json` / `tokenizer_config.json`）都在 base 模型仓库。
fn is_tokenizer_file(name: &str) -> bool {
    name.starts_with("tokenizer") || name.eq("special_tokens_map.json") || name.eq("vocab.json")
}

pub(crate) fn load_llama_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, Llama, LlamaCache, Tokenizer, Option<LlamaEosToks>)> {
    log::info!("load_llama_model_files start");
    let tokenizer = init_tokenizer(info.repository)?;
    let device = device()?;

    let config_filename = construct_model_file_path(info.repository, "config.json");
    let config: LlamaConfig = serde_json::from_slice(&std::fs::read(config_filename)?)?;
    let config = config.into_config(device.is_cuda());
    let dtype = DType::F16;
    let cache = LlamaCache::new(true, dtype, &config, &device)?;
    let filenames = get_model_files(info)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, dtype, &device)? };
    let m = Llama::load(vb, &config)?;
    let eos_token_id = config
        .eos_token_id
        .or_else(|| tokenizer.token_to_id("</s>").map(LlamaEosToks::Single));
    log::info!("load_llama_model_files end");
    Ok((device, m, cache, tokenizer, eos_token_id))
}

pub(crate) fn load_gemma_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, GemmaModel, Tokenizer)> {
    let tokenizer = init_tokenizer(info.repository)?;
    let device = device()?;

    let config_filename = construct_model_file_path(info.repository, "config.json");
    let config: GemmaConfig = serde_json::from_reader(std::fs::File::open(config_filename)?)?;
    let dtype = if device.is_cuda() {
        DType::BF16
    } else {
        DType::F32
    };
    let filenames = get_model_files(info)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, dtype, &device)? };
    let model = GemmaModel::new(device.is_cuda(), &config, vb)?;
    Ok((device, model, tokenizer))
}

pub(crate) fn load_parler_tts_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, ParlerTtsModel, Tokenizer)> {
    let tokenizer = init_tokenizer(info.repository)?;
    let device = device()?;
    let filenames = get_model_files(info)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, DType::F32, &device)? };
    let config_filename = construct_model_file_path(info.repository, "config.json");
    let config: ParlerTtsConfig = serde_json::from_reader(std::fs::File::open(config_filename)?)?;
    let model = ParlerTtsModel::new(&config, vb)?;
    Ok((device, model, tokenizer))
}

pub(crate) fn load_moondream_model_files(
    info: &HuggingFaceModelInfo,
) -> Result<(Device, MoondreamModel, Tokenizer)> {
    let tokenizer = init_tokenizer(info.repository)?;
    let device = device()?;
    let config = MoondreamConfig::v2();
    let dtype = if device.is_cuda() {
        DType::F16
    } else {
        DType::F32
    };
    let filenames = get_model_files(info)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, dtype, &device)? };
    let model = MoondreamModel::new(&config, vb)?;
    Ok((device, model, tokenizer))
}

pub(crate) fn load_pytorch_mode_files(info: &HuggingFaceModelInfo, device: &Device) -> Result<()> {
    let weights_filename = construct_model_file_path(info.repository, "pytorch_model.bin");
    let vb = VarBuilder::from_pth(&weights_filename, DType::BF16, device)?;
    Ok(())
}
