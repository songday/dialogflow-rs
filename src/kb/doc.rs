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
use zip::ZipArchive;

use super::dto::DocData;
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
        );
        CREATE TABLE {robot_id}_vec (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            doc_id NOT NULL INTEGER
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
    let sql = format!("SELECT * FROM {robot_id} ORDER BY created_at DESC");
    let conn = DATA_SOURCE.get().unwrap().connect()?;
    let mut rows = conn.query(&sql, ()).await?;
    let mut results = Vec::with_capacity(10);
    while let Some(row) = rows.next().await? {
        results.push(DocData {
            id: row.get_value(0)?.as_integer().unwrap().clone(),
            file_name: String::from(row.get_value(0)?.as_text().unwrap()),
            file_size: row.get_value(0)?.as_integer().unwrap().clone(),
            doc_content: String::from(row.get_value(0)?.as_text().unwrap()),
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
