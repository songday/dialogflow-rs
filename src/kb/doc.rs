// use std::fs::File;
// use std::io::Read;
// use std::path::Path;
use std::io::{Cursor, Read};
use std::sync::OnceLock;
use std::vec::Vec;

// use futures_util::StreamExt;
use quick_xml::Reader;
use quick_xml::events::Event;
// use sqlx::{Row, Sqlite};
// use text_splitter::{ChunkConfig, TextSplitter};
use zip::ZipArchive;

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
    let conn = DATA_SOURCE.get().unwrap().connect()?;
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
    save_doc_embedding(robot_id, doc_id, doc_content).await?;
    Ok(())
}

pub(crate) async fn delete(robot_id: &str, doc_id: i64) -> Result<()> {
    let mut conn = DATA_SOURCE.get().unwrap().connect()?;
    let tx = conn.transaction().await?;
    let sql = format!("DELETE FROM {robot_id}_vec WHERE doc_id = ?1");
    let _r = tx.execute(&sql, [doc_id]).await?;
    let sql = format!("DELETE FROM {robot_id} WHERE id = ?1");
    let _r = tx.execute(&sql, [doc_id]).await?;
    Ok(())
}

fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < words.len() {
        let end = std::cmp::min(start + chunk_size, words.len());
        let chunk = words[start..end].join(" ");
        chunks.push(chunk);

        if end == words.len() {
            break;
        }
        start = end.saturating_sub(overlap);
    }
    chunks
}

async fn save_doc_embedding(robot_id: &str, doc_id: i64, doc_content: &str) -> Result<()> {
    let chunks = chunk_text(doc_content, 500, 70);
    let mut created_table = false;
    let conn = DATA_SOURCE.get().unwrap().connect()?;
    for chunk in chunks.iter() {
        let r = embedding::embedding(robot_id, chunk).await?;
        if !created_table {
            let sql = format!(
                "CREATE TABLE IF NOT EXISTS {robot_id}_vec (
                    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                    doc_id INTEGER NOT NULL,
                    chunk_text TEXT NOT NULL,
                    chunk_vec F32_BLOB({}) NOT NULL
                );",
                r.0.len()
            );
            conn.execute(&sql, ()).await?;
            created_table = true;
        }
        let sql = format!(
            "INSERT INTO {robot_id}_vec(doc_id, chunk_text, chunk_vec) VALUES(?1, ?2, vector32(?3));"
        );
        conn.execute(
            &sql,
            (
                doc_id,
                turso::Value::Text(String::from(chunk)),
                embedding::vec_to_db(&r.0),
            ),
        )
        .await?;
        log::info!("Embedding id={}", conn.last_insert_rowid());
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
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"w:p" => in_paragraph = true,
            Ok(Event::End(ref e)) if e.name().as_ref() == b"w:p" => {
                doc_text.push('\n');
                in_paragraph = false;
            }
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"w:p" => doc_text.push('\n'),
            Ok(Event::Text(e)) if in_paragraph => {
                doc_text.push_str(&e.decode()?);
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!("Error at position {}: {:?}", reader.error_position(), e),
            _ => (),
        }
    }
    Ok(doc_text)
}

fn parse_pdf() {}

pub(crate) async fn search_doc(
    robot_id: &str,
    query: &str,
    recall_distance: f64,
    connect_timeout: u32,
    read_timeout: u32,
) -> Result<Option<String>> {
    let r = embedding::embedding(robot_id, query).await?;
    let sql = format!(
        "SELECT chunk_text, vector_distance_cos(chunk_vec, vector32(?1)) AS distance FROM {robot_id}_vec WHERE distance < ?2 ORDER BY distance ASC LIMIT 1"
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
    if let Some(row) = rows.next().await? {
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
                    row.get_value(0)?.as_text().unwrap(),
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
