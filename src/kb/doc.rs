// use std::fs::File;
// use std::io::Read;
// use std::path::Path;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::OnceLock;
use std::vec::Vec;

use futures_util::StreamExt;
use futures_util::TryStreamExt;
use quick_xml::Reader;
use quick_xml::events::Event;
// use sqlx::{Row, Sqlite};
// use text_splitter::{ChunkConfig, TextSplitter};
use zip::ZipArchive;

use text_splitter::TextSplitter;

use super::dto::DocData;
use crate::ai::embedding;
use crate::result::{Error, Result};

// type SqliteConnPool = sqlx::Pool<Sqlite>;

// static DATA_SOURCE: OnceCell<SqliteConnPool> = OnceCell::new();
static DATA_SOURCE: OnceLock<turso::Database> = OnceLock::new();
// static DATA_SOURCES: OnceLock<Mutex<HashMap<String, SqliteConnPool>>> = OnceLock::new();

pub(crate) async fn init_datasource() -> Result<()> {
    let p = std::path::Path::new(".").join("data");
    if !p.exists() {
        std::fs::create_dir_all(&p).expect("Create data directory failed.");
    }
    let p = p.join("doc.dat");
    let turso = turso::Builder::new_local(p.as_path().to_str().unwrap())
        .build()
        .await?;
    DATA_SOURCE
        .set(turso)
        .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
    // let p = get_sqlite_path();
    // let pool = crate::db::init_sqlite_datasource(p.as_path()).await?;
    // DATA_SOURCE
    //     .set(pool)
    //     .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
}

pub(crate) async fn init_tables(robot_id: &str) -> Result<()> {
    // println!("Init database");
    // let ddl = include_str!("./embedding_ddl.sql");
    let sql = format!(
        "CREATE TABLE {robot_id} (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            file_name TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            doc_content TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );"
    );
    let conn = DATA_SOURCE.get().unwrap().connect()?;
    conn.execute(&sql, ()).await?;
    // // log::info!("sql = {}", &sql);
    // let mut stream = sqlx::raw_sql(&sql).execute_many(DATA_SOURCE.get().unwrap());
    // while let Some(res) = stream.next().await {
    //     match res {
    //         Ok(_r) => log::info!("Initialized doc table"),
    //         Err(e) => log::error!("Create table failed, err: {e:?}"),
    //     }
    // }
    // // let dml = include_str!("../resource/sql/dml.sql");
    // // if let Err(e) = sqlx::query(dml).execute(&pool).await {
    // //     panic!("{:?}", e);
    // // }
    Ok(())
}

// crate::sqlite_trans! {
//     fn save2(robot_id: &str,
//         file_name: &str,
//         file_size: usize,
//         doc_content: &str) -> Result<()> {
//             let sql = format!(
//                 "INSERT INTO {}(file_name, file_size, doc_content, created_at)VALUES(?, ?, ?, unixepoch())",
//                 robot_id
//             );
//             sqlx::query::<Sqlite>(&sql)
//                 .bind(file_name)
//                 .bind(file_size as i64)
//                 .bind(doc_content)
//                 .execute(&mut **transaction)
//                 .await?;
//         Ok(())
//     }
// }

pub(super) async fn list(robot_id: &str) -> Result<Vec<DocData>> {
    let sql = format!(
        "SELECT id, file_name, file_size, doc_content FROM {robot_id} ORDER BY created_at DESC"
    );
    let conn = DATA_SOURCE.get().unwrap().connect()?;
    let mut rows = conn.query(&sql, ()).await?;
    let mut results = Vec::with_capacity(10);
    while let Some(row) = rows.next().await? {
        results.push(DocData {
            id: row.get_value(0)?.as_integer().unwrap().clone(),
            file_name: String::from(row.get_value(1)?.as_text().unwrap()),
            file_size: row.get_value(2)?.as_integer().unwrap().clone(),
            doc_content: String::from(row.get_value(3)?.as_text().unwrap()),
        });
    }
    Ok(results)
}

pub(super) async fn save(
    robot_id: &str,
    file_name: &str,
    file_size: usize,
    doc_content: &str,
) -> Result<()> {
    let sql = format!(
        "INSERT INTO {robot_id}(file_name, file_size, doc_content, created_at)VALUES(?1, ?2, ?3, unixepoch())"
    );
    let mut conn = DATA_SOURCE.get().unwrap().connect()?;
    conn.execute(
        &sql,
        (
            file_name,
            turso::Value::Integer(file_size as i64),
            doc_content,
        ),
    )
    .await?;
    let doc_id = conn.last_insert_rowid();
    // log::info!("doc_id={}", doc_id);
    let tx = conn.transaction().await?;
    save_doc_embedding(&tx, robot_id, doc_id, doc_content).await?;
    tx.commit().await?;
    Ok(())
}

pub(super) async fn update(robot_id: &str, doc_id: i64, doc_content: &str) -> Result<()> {
    let mut conn = DATA_SOURCE.get().unwrap().connect()?;
    let tx = conn.transaction().await?;
    let sql = format!("UPDATE {robot_id} SET doc_content = ?1 WHERE id = ?2");
    let r = tx.execute(&sql, (doc_content, doc_id)).await?;
    if r > 0 {
        let sql = format!("DELETE FROM {robot_id}_vec WHERE doc_id = ?1");
        tx.execute(&sql, [doc_id]).await?;
        save_doc_embedding(&tx, robot_id, doc_id, doc_content).await?;
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(())
}

pub(crate) async fn delete(robot_id: &str, doc_id: i64) -> Result<()> {
    let mut conn = DATA_SOURCE.get().unwrap().connect()?;
    let tx = conn.transaction().await?;
    let sql = format!("DELETE FROM {robot_id}_vec WHERE doc_id = ?1");
    let _r = tx.execute(&sql, [doc_id]).await?;
    let sql = format!("DELETE FROM {robot_id} WHERE id = ?1");
    let _r = tx.execute(&sql, [doc_id]).await?;
    tx.commit().await?;
    Ok(())
}

// chunk_size and overlap are counted in characters, so this works for both
// Chinese (no whitespace between words) and English texts.
// Paragraphs are kept intact whenever they fit into one chunk.
fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::with_capacity(chunk_size * 4);
    for para in text.split('\n') {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        let para_len = para.chars().count();
        if para_len > chunk_size {
            // A single paragraph longer than chunk_size: split it into
            // sliding windows of characters.
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let chars: Vec<char> = para.chars().collect();
            let mut start = 0;
            while start < chars.len() {
                let end = std::cmp::min(start + chunk_size, chars.len());
                chunks.push(chars[start..end].iter().collect());
                if end == chars.len() {
                    break;
                }
                start = end - overlap.min(chunk_size - 1);
            }
        } else {
            let cur_len = current.chars().count();
            if !current.is_empty() && cur_len + para_len + 1 > chunk_size {
                // Flush the current chunk, carrying its tail over as overlap.
                let tail: String = current
                    .chars()
                    .skip(current.chars().count().saturating_sub(overlap))
                    .collect();
                chunks.push(std::mem::take(&mut current));
                current = tail;
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(para);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

// Semantic chunking via text-splitter: splits at sentence/paragraph boundaries
// while maximizing chunk length. Suitable for long-context embedding models
// (OpenAI / Ollama etc.), where a much larger chunk capacity is affordable.
fn chunk_text_semantic(text: &str, chunk_size: usize, overlap: usize) -> Result<Vec<String>> {
    let splitter = TextSplitter::new(text_splitter::ChunkConfig::new(chunk_size).with_overlap(overlap).map_err(|e| {
        Error::WithMessage(format!("Invalid chunk config: {e:?}"))
    })?);
    Ok(splitter.chunks(text).map(String::from).collect())
}

async fn save_doc_embedding(
    tx: &turso::transaction::Transaction<'_>,
    robot_id: &str,
    doc_id: i64,
    doc_content: &str,
) -> Result<()> {
    // Local HuggingFace BERT models are capped at 512 tokens, so keep chunks
    // small. Long-context remote models (OpenAI / Ollama) can afford much
    // larger semantic chunks.
    let long_context = !matches!(
        crate::man::settings::get_settings(robot_id)?
            .map(|s| s.sentence_embedding_provider.provider.clone()),
        Some(embedding::SentenceEmbeddingProvider::HuggingFace(_))
    );
    let chunks = if long_context {
        chunk_text_semantic(doc_content, 2000, 200)?
    } else {
        chunk_text(doc_content, 500, 70)
    };
    if chunks.is_empty() {
        return Ok(());
    }
    // Embed all chunks up front with bounded concurrency, so the table can be
    // created once with the right vector size and inserts reuse a prepared
    // statement instead of parsing the SQL for every chunk.
    let embeddings: Vec<(Vec<f32>, f32)> = futures_util::stream::iter(
        chunks.iter().map(|c| embedding::embedding(robot_id, c)),
    )
    .buffered(4)
    .try_collect()
    .await?;
    let vec_size = embeddings
        .first()
        .map(|(v, _)| v.len())
        .ok_or_else(|| Error::WithMessage(String::from("Embedding data is empty.")))?;
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS {robot_id}_vec (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            doc_id INTEGER NOT NULL,
            chunk_text TEXT NOT NULL,
            chunk_vec F32_BLOB({vec_size}) NOT NULL
        );"
    );
    tx.execute(&sql, ()).await?;
    let sql = format!(
        "INSERT INTO {robot_id}_vec(doc_id, chunk_text, chunk_vec) VALUES(?1, ?2, vector32(?3));"
    );
    let mut stmt = tx.prepare(&sql).await?;
    for (chunk, (v, _)) in chunks.iter().zip(embeddings.iter()) {
        stmt.execute((
            doc_id,
            turso::Value::Text(String::from(chunk)),
            embedding::vec_to_db(v),
        ))
        .await?;
        // log::info!("Embedding id={}", conn.last_insert_rowid());
    }
    Ok(())
}

pub(super) fn parse_docx(b: Vec<u8>) -> Result<String> {
    // let mut file = File::open("./numbering.docx")?;
    // let mut buf = Vec::with_capacity(3096);
    // file.read_to_end(&mut buf)?;
    let mut doc_text = String::with_capacity(3096);
    let reader = Cursor::new(b);
    let mut archive = ZipArchive::new(reader)?;
    let mut zip_file = archive.by_name("word/document.xml")?;
    let mut cache = String::with_capacity(zip_file.size() as usize);
    zip_file.read_to_string(&mut cache)?;

    // 创建 XML 解析器
    let mut reader = Reader::from_str(&cache);
    reader.config_mut().trim_text(false);
    let mut in_paragraph = false;

    // 读取 XML 内容
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if e.name().0.eq("w:p") => in_paragraph = true,
            Ok(Event::End(ref e)) if e.name().0.eq("w:p") => {
                doc_text.push('\n');
                in_paragraph = false;
            }
            Ok(Event::Empty(ref e)) if e.name().0.eq("w:p") => doc_text.push('\n'),
            Ok(Event::Text(e)) if in_paragraph => {
                doc_text.push_str(&e.as_ref());
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!("Error at position {}: {:?}", reader.error_position(), e),
            _ => (),
        }
    }
    Ok(doc_text)
}

fn parse_pdf() {}

// Extracts search keywords from a user query: ASCII words (>= 2 chars,
// lowercased) plus Chinese characters, which are combined into bigrams so
// that e.g. "知识库" becomes "知识" / "识库". Bigrams work well for
// substring matching without any tokenizer.
fn extract_keywords(query: &str) -> Vec<String> {
    let mut kws: Vec<String> = Vec::new();
    let mut cjk = String::new();
    let r = regex::Regex::new(r"[0-9A-Za-z_]{2,}|\p{Han}").unwrap();
    for m in r.find_iter(query) {
        let t = m.as_str();
        if t.chars().next().unwrap().is_ascii() {
            let w = t.to_lowercase();
            if !kws.contains(&w) {
                kws.push(w);
            }
        } else {
            cjk.push_str(t);
        }
    }
    let chars: Vec<char> = cjk.chars().collect();
    if chars.len() <= 2 {
        if !chars.is_empty() {
            let w: String = chars.iter().collect();
            if !kws.contains(&w) {
                kws.push(w);
            }
        }
    } else {
        for w in chars.windows(2) {
            let w: String = w.iter().collect();
            if !kws.contains(&w) {
                kws.push(w);
            }
        }
    }
    kws.truncate(8);
    kws
}

// Reciprocal Rank Fusion: merges the vector recall list and the keyword
// recall list into one ranking. Each document earns 1/(60 + rank) per list
// it appears in, so documents found by both channels float to the top.
fn rrf_merge(vec_hits: &[(i64, String)], kw_hits: &[(i64, String)]) -> Vec<String> {
    let mut fused: HashMap<i64, (f64, &String)> = HashMap::new();
    for hits in [vec_hits, kw_hits] {
        for (rank, (id, text)) in hits.iter().enumerate() {
            let score = 1.0 / (60.0 + rank as f64);
            match fused.entry(*id) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    e.get_mut().0 += score;
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert((score, text));
                }
            }
        }
    }
    let mut ranked: Vec<(f64, &String)> = fused.into_values().collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    ranked.into_iter().take(4).map(|(_, t)| t.clone()).collect()
}

async fn keyword_recall(
    conn: &turso::Connection,
    robot_id: &str,
    query: &str,
) -> Result<Vec<(i64, String)>> {
    let keywords = extract_keywords(query);
    if keywords.is_empty() {
        return Ok(Vec::new());
    }
    // Escape LIKE wildcards, then match any keyword with an OR chain.
    let escaped: Vec<String> = keywords
        .iter()
        .map(|k| {
            k.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        })
        .collect();
    let cond = escaped
        .iter()
        .enumerate()
        .map(|(i, _)| format!("chunk_text LIKE ?{i} ESCAPE '\\'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!(
        "SELECT id, chunk_text FROM {robot_id}_vec WHERE {cond} LIMIT 64"
    );
    let params: Vec<turso::Value> = escaped
        .iter()
        .map(|k| turso::Value::Text(k.clone()))
        .collect();
    let mut rows = conn.query(&sql, params).await?;
    let mut hits: Vec<(i64, String, usize)> = Vec::new();
    while let Some(row) = rows.next().await? {
        let id = row.get_value(0)?.as_integer().unwrap().clone();
        let text = String::from(row.get_value(1)?.as_text().unwrap());
        let lower = text.to_lowercase();
        let score = keywords.iter().filter(|k| lower.contains(k.as_str())).count();
        hits.push((id, text, score));
    }
    // Keep only chunks matching at least two keywords when possible, so a
    // single common word does not flood the recall list.
    hits.sort_by(|a, b| b.2.cmp(&a.2));
    let min_score = hits.first().map(|h| h.2).unwrap_or(0).min(2).max(1);
    hits.retain(|h| h.2 >= min_score);
    hits.truncate(8);
    Ok(hints_to_pairs(hits))
}

fn hints_to_pairs(hits: Vec<(i64, String, usize)>) -> Vec<(i64, String)> {
    hits.into_iter().map(|(id, text, _)| (id, text)).collect()
}

pub(crate) async fn search_doc(
    robot_id: &str,
    query: &str,
    recall_distance: f64,
    connect_timeout: u32,
    read_timeout: u32,
) -> Result<Option<String>> {
    let r = embedding::embedding(robot_id, query).await?;
    // log::info!("{:?}", &r.0);
    let sql = format!(
        "SELECT id, chunk_text, vector_distance_cos(chunk_vec, vector32(?1)) AS distance FROM {robot_id}_vec WHERE distance < ?2 ORDER BY distance ASC LIMIT 8"
    );
    let conn = DATA_SOURCE.get().unwrap().connect()?;
    let mut rows = conn
        .query(
            &sql,
            [
                embedding::vec_to_db(&r.0),
                turso::Value::Real(recall_distance),
            ],
        )
        .await?;
    let mut vec_hits: Vec<(i64, String)> = Vec::with_capacity(8);
    while let Some(row) = rows.next().await? {
        log::info!(
            "{} {}",
            recall_distance,
            row.get_value(2)?.as_real().unwrap(),
        );
        vec_hits.push((
            row.get_value(0)?.as_integer().unwrap().clone(),
            String::from(row.get_value(1)?.as_text().unwrap()),
        ));
    }
    // Hybrid recall: fuse vector ranking with keyword (LIKE) ranking.
    let chunks = match keyword_recall(&conn, robot_id, query).await {
        Ok(kw_hits) => rrf_merge(&vec_hits, &kw_hits),
        Err(e) => {
            log::warn!("Keyword recall failed, fallback to vector only, err: {e:?}");
            vec_hits.into_iter().map(|(_, t)| t).take(4).collect()
        }
    };
    if !chunks.is_empty() {
        let prompts = vec![
            crate::ai::completion::Prompt {
                role: String::from("system"),
                content: String::from(
                    "你是一个专业的文档助手。请根据提供的文档内容回答问题。\
                                如果文档内容中没有相关信息，请明确说明。\
                                回答要基于文档内容，不要编造信息。",
                ),
            },
            crate::ai::completion::Prompt {
                role: String::from("user"),
                content: format!(
                    "文档内容：\n{}\n\n问题：{}",
                    chunks.join("\n\n"),
                    query
                ),
            },
        ];
        let mut s = String::with_capacity(1024);
        if let Err(e) = crate::ai::chat::chat(
            robot_id,
            Some(prompts),
            Some(connect_timeout),
            Some(read_timeout),
            crate::ai::chat::ResultSender::StrBuf(&mut s),
        )
        .await
        {
            log::error!("LlmChatNode response failed, err: {:?}", &e);
        } else {
            return Ok(Some(s));
        }
    }
    Ok(None)
}
