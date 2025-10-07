use std::sync::OnceLock;
use std::vec::Vec;

// use futures_util::StreamExt;
// use sqlx::{Row, Sqlite};

use super::dto::IntentPhraseData;
use crate::ai::embedding::embedding;
use crate::result::{Error, Result};

// type SqliteConnPool = sqlx::Pool<Sqlite>;

// static DATA_SOURCE: OnceCell<SqliteConnPool> = OnceCell::new();
static DATA_SOURCE: OnceLock<turso::Database> = OnceLock::new();
// static DATA_SOURCES: OnceLock<Mutex<HashMap<String, SqliteConnPool>>> = OnceLock::new();
// static INDEXES: LazyLock<Mutex<HashMap<String, usearch::Index>>> =
//     LazyLock::new(|| Mutex::new(HashMap::with_capacity(32)));

fn get_sqlite_path() -> std::path::PathBuf {
    let p = std::path::Path::new(".").join("data");
    if !p.exists() {
        std::fs::create_dir_all(&p).expect("Create data directory failed.");
    }
    p.join("ripd.dat")
}

pub(crate) async fn init_datasource() -> Result<()> {
    let p = std::path::Path::new(".").join("data");
    if !p.exists() {
        std::fs::create_dir_all(&p).expect("Create data directory failed.");
    }
    let p = p.join("phrase.dat");
    // log::info!("Init phrase datasource, path = {:?}", p.as_path().to_str().unwrap());
    let turso = turso::Builder::new_local(p.as_path().to_str().unwrap())
        .build()
        .await?;
    DATA_SOURCE
        .set(turso)
        .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
}

// pub(crate) async fn init_tables(robot_id: &str) -> Result<()> {
//     // println!("Init database");
//     // let ddl = include_str!("./embedding_ddl.sql");
//     let sql = format!(
//         "CREATE TABLE {robot_id} (
//             id INTEGER NOT NULL PRIMARY KEY,
//             -- id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
//             intent_id TEXT NOT NULL,
//             intent_name TEXT NOT NULL,
//             phrase_vec F32_BLOB(384)
//         );",
//     );
//     // log::info!("sql = {}", &sql);
//     let mut stream = sqlx::raw_sql(&sql).execute_many(DATA_SOURCE.get().unwrap());
//     while let Some(res) = stream.next().await {
//         match res {
//             Ok(_r) => log::info!("Initialized phrase table"),
//             Err(e) => log::error!("Create table failed, err: {e:?}"),
//         }
//     }
//     // let dml = include_str!("../resource/sql/dml.sql");
//     // if let Err(e) = sqlx::query(dml).execute(&pool).await {
//     //     panic!("{:?}", e);
//     // }
//     Ok(())
// }

pub(crate) async fn search(robot_id: &str, vectors: &Vec<f32>) -> Result<Vec<(String, f64)>> {
    let sql = format!(
        "SELECT intent_id, intent_name, distance FROM {robot_id} WHERE phrase_vec MATCH ? ORDER BY distance ASC LIMIT 1",
    );
    let conn = DATA_SOURCE.get().unwrap().connect()?;
    let mut results = conn.query(&sql, [serde_json::to_string(vectors)?]).await?;
    // let results = sqlx::query::<Sqlite>(&sql)
    //     .bind(serde_json::to_string(vectors)?)
    //     .fetch_all(DATA_SOURCE.get().unwrap())
    //     .await?;
    let mut names = Vec::with_capacity(1);
    while let Some(r) = results.next().await? {
        names.push((
            r.get_value(1)?.as_text().unwrap().to_string(),
            r.get_value(2)?.as_real().unwrap().clone(),
        ));
    }
    Ok(names)
}

// fn update_idx(robot_id: &str, key: u64, vec: &[f32]) -> Result<()> {
//     let mut idxes = INDEXES.lock()?;
//     let p = std::path::Path::new("ipvd.vec");
//     let s = p.display().to_string();
//     if !idxes.contains_key(robot_id) {
//         let options = usearch::IndexOptions {
//             dimensions: vec.len(),
//             metric: usearch::MetricKind::Cos,
//             quantization: usearch::ScalarKind::F32,
//             connectivity: 0,                        // zero for auto
//             expansion_add: 0,                       // zero for auto
//             expansion_search: 0,                    // zero for auto
//             multi: false,
//         };
//         let index: usearch::Index = usearch::new_index(&options).unwrap();
//         if p.exists() {
//             index.load(&s)?;
//         }
//         idxes.insert(String::from(robot_id), index);
//     }
//     let idx = idxes.get(robot_id).unwrap();
//     log::info!("idx memory_usage: {}", idx.memory_usage());
//     idx.add(key, vec)?;
//     idx.save(&s)?;
//     Ok(())
// }

pub(crate) async fn add(
    robot_id: &str,
    vec_row_id: Option<i64>,
    intent_id: &str,
    intent_name: &str,
    phrase: &str,
) -> Result<i64> {
    // check_datasource(robot_id, intent_id).await?;
    let vectors = embedding(robot_id, phrase).await?;
    if vectors.0.is_empty() {
        let err = format!("{phrase} embedding data is empty");
        log::warn!("{}", &err);
        return Err(Error::WithMessage(err));
    }
    // log::info!("vectors.0.len() = {}", vectors.0.len());
    let conn = DATA_SOURCE.get().unwrap().connect()?;
    if vec_row_id.is_none() {
        let sql = format!(
                "CREATE TABLE IF NOT EXISTS {robot_id} (
                            id INTEGER NOT NULL PRIMARY KEY,
                            intent_id TEXT NOT NULL,
                            intent_name TEXT NOT NULL,
                            phrase TEXT NOT NULL,
                            phrase_vec F32_BLOB({})
                        );
                INSERT INTO {robot_id} (id, intent_id, intent_name, phrase, phrase_vec)VALUES(?1, ?2, ?3, ?4, ?5)",
                vectors.0.len()
            );
        let id = time::UtcDateTime::now().unix_timestamp() - 1760025600000;
        conn.execute(
            &sql,
            (
                id,
                intent_id,
                intent_name,
                phrase,
                serde_json::to_string(&vectors.0)?,
            ),
        )
        .await?;
        // sqlx::query::<Sqlite>(&sql)
        //     .bind(last_insert_rowid)
        //     .bind(intent_id)
        //     .bind(intent_name)
        //     .bind(phrase)
        //     .bind(serde_json::to_string(vec)?)
        //     .execute(&mut **txn)
        //     .await?;
        Ok(id)
    } else {
        let sql = format!("UPDATE {robot_id} SET phrase = ?1, phrase_vec = ?2 WHERE id = ?3",);
        let vec_row_id = vec_row_id.unwrap();
        conn.execute(
            &sql,
            (phrase, serde_json::to_string(&vectors.0)?, vec_row_id),
        )
        .await?;
        Ok(vec_row_id)
    }
}

pub(crate) async fn batch_add(
    robot_id: &str,
    intent_id: &str,
    intent_name: &str,
    phrases: &[IntentPhraseData],
) -> Result<()> {
    // check_datasource(robot_id, intent_id).await?;
    for p in phrases.iter() {
        add(robot_id, Some(p.id), intent_id, intent_name, &p.phrase).await?;
    }
    Ok(())
}

pub(crate) async fn remove(robot_id: &str, id: i64) -> Result<()> {
    // INDEXES.lock()?.get(robot_id).and_then(|idx| {idx.remove(id as u64); None::<()>});
    let sql = format!("DELETE FROM {robot_id} WHERE id = ?1");
    DATA_SOURCE
        .get()
        .unwrap()
        .connect()?
        .execute(&sql, [id])
        .await?;
    // sqlx::query::<Sqlite>(&sql)
    //     .bind(id)
    //     .execute(DATA_SOURCE.get().unwrap())
    //     .await?;
    Ok(())
}

pub(crate) async fn remove_by_intent_id(robot_id: &str, intent_id: &str) -> Result<()> {
    let sql = format!("DELETE FROM {robot_id} WHERE intent_id = ?1");
    DATA_SOURCE
        .get()
        .unwrap()
        .connect()?
        .execute(&sql, [intent_id])
        .await?;
    // match sqlx::query::<Sqlite>(&sql)
    //     .bind(intent_id)
    //     .execute(DATA_SOURCE.get().unwrap())
    //     .await
    // {
    //     Ok(_) => return Ok(()),
    //     Err(e) => match e {
    //         sqlx::Error::Database(database_error) => {
    //             if let Some(code) = database_error.code() {
    //                 if code.eq("1") {
    //                     return Ok::<_, Error>(());
    //                 }
    //             }
    //         }
    //         _ => return Err(e.into()),
    //     },
    // };
    Ok(())
}

pub(crate) async fn remove_tables(robot_id: &str) -> Result<()> {
    let sql = format!("DROP TABLE {robot_id}");
    DATA_SOURCE
        .get()
        .unwrap()
        .connect()?
        .execute(&sql, ())
        .await?;
    // match sqlx::query::<Sqlite>(&sql)
    //     .execute(DATA_SOURCE.get().unwrap())
    //     .await
    // {
    //     Ok(_) => return Ok(()),
    //     Err(e) => match e {
    //         sqlx::Error::Database(database_error) => {
    //             if let Some(code) = database_error.code() {
    //                 if code.eq("1") {
    //                     return Ok::<_, Error>(());
    //                 }
    //             }
    //         }
    //         _ => return Err(e.into()),
    //     },
    // };
    Ok(())
}
