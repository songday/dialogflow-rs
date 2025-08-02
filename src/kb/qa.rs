use std::sync::OnceLock;
use std::vec::Vec;

// use futures_util::StreamExt;
// use sqlx::{Row, Sqlite};

use super::dto::{QuestionAnswerPair, QuestionData};
use crate::ai::embedding::embedding;
use crate::result::{Error, Result};

// type SqliteConnPool = sqlx::Pool<Sqlite>;

// // static DATA_SOURCE: OnceCell<SqliteConnPool> = OnceCell::new();
// static DATA_SOURCE: OnceLock<SqliteConnPool> = OnceLock::new();
static TURSO_DATA_SOURCE: OnceLock<turso::Database> = OnceLock::new();
// static DATA_SOURCES: OnceLock<Mutex<HashMap<String, SqliteConnPool>>> = OnceLock::new();

pub(crate) async fn init_datasource() -> Result<()> {
    let p = std::path::Path::new(".").join("data");
    if !p.exists() {
        std::fs::create_dir_all(&p).expect("Create data directory failed.");
    }
    p.join("qa.dat");
    let turso = turso::Builder::new_local(p.as_path().to_str().unwrap())
        .build()
        .await?;
    TURSO_DATA_SOURCE
        .set(turso)
        .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
    // let p = get_sqlite_path();
    // let pool = crate::db::init_sqlite_datasource(p.as_path()).await?;
    // DATA_SOURCE
    //     .set(pool)
    //     .map_err(|_| Error::WithMessage(String::from("Datasource has been set.")))
}

pub async fn shutdown_db() {
    // let mut r = match DATA_SOURCES.lock() {
    //     Ok(l) => l,
    //     Err(e) => e.into_inner(),
    // };
    // let all_keys: Vec<String> = r.keys().map(|k| String::from(k)).collect();
    // let mut pools: Vec<SqliteConnPool> = Vec::with_capacity(all_keys.len());
    // for key in all_keys {
    //     let v = r.remove(&key).unwrap();
    //     pools.push(v);
    // }
    // tokio::task::spawn_blocking(|| async move {
    //     for p in pools.iter() {
    //         p.close().await;
    //     }
    // });
    // DATA_SOURCE.get().unwrap().close().await;
}

pub(crate) async fn init_tables(robot_id: &str) -> Result<()> {
    // println!("Init database");
    let sql = format!(
        "CREATE TABLE {robot_id}_qa (
            id INTEGER NOT NULL PRIMARY KEY,
            qa_data TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX idx_{robot_id}_created_at ON {robot_id}_qa (created_at);"
    );
    let conn = TURSO_DATA_SOURCE.get().unwrap().connect()?;
    conn.execute(&sql, ()).await?;
    Ok(())
}

pub(crate) async fn list(robot_id: &str) -> Result<Vec<QuestionAnswerPair>> {
    let conn = TURSO_DATA_SOURCE.get().unwrap().connect()?;
    let sql = format!("SELECT qa_data FROM {robot_id}_qa ORDER BY created_at DESC",);
    let mut stmt = conn.prepare(&sql).await?;
    let mut rows = stmt.query(["foo@example.com"]).await?;
    let mut d: Vec<QuestionAnswerPair> = Vec::with_capacity(10);
    while let Ok(op) = rows.next().await {
        if let Some(row) = op {
            d.push(serde_json::from_str(dbg!(
                row.get_value(0)?.as_text().unwrap()
            ))?);
        }
    }
    Ok(d)
}

pub(crate) async fn save(robot_id: &str, mut d: QuestionAnswerPair) -> Result<u64> {
    let mut conn = TURSO_DATA_SOURCE.get().unwrap().connect()?;
    let tx = conn.transaction().await?;
    let record_id: u64;
    if d.id.is_none() {
        let sql = format!("INSERT INTO {robot_id}_qa(qa_data, created_at)VALUES(?, unixepoch())");
        let mut stmt = tx.prepare(&sql).await?;
        let r = stmt.execute((serde_json::to_string(&d)?,)).await?;
        record_id = r;
        d.id = Some(r);
    } else {
        let sql = format!("UPDATE {robot_id}_qa SET qa_data = ? WHERE id = ?");
        let mut stmt = tx.prepare(&sql).await?;
        record_id = d.id.unwrap();
        let _r = stmt
            .execute((serde_json::to_string(&d)?, record_id))
            .await?;
    }
    let mut questions: Vec<&mut QuestionData> = Vec::with_capacity(5);
    questions.push(&mut d.question);
    if !d.similar_questions.is_empty() {
        let similar_questions: Vec<&mut QuestionData> = d.similar_questions.iter_mut().collect();
        questions.extend(similar_questions);
    }
    let mut insert_stmt = Option::None::<turso::Statement>;
    let mut update_stmt = Option::None::<turso::Statement>;
    for q in questions.iter_mut() {
        let vectors = embedding(robot_id, &q.question).await?;
        if vectors.0.is_empty() {
            let err = format!("{} embedding data is empty", &q.question);
            log::warn!("{}", &err);
            return Err(Error::WithMessage(err));
        }

        log::info!("vectors.0.len() = {}", vectors.0.len());
        if q.vec_row_id.is_none() {
            if insert_stmt.is_none() {
                let sql = format!(
                    "CREATE TABLE IF NOT EXISTS {}_qa_vec (
                id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                qa_id INTEGER NOT NULL,
                vectors F32_BLOB({})
            );
            INSERT INTO {}_qa_vec (qa_id, vectors)VALUES(?, ?)",
                    //  ON CONFLICT(rowid) DO UPDATE SET vectors = excluded.vectors;
                    robot_id,
                    vectors.0.len(),
                    robot_id
                );
                insert_stmt = Some(tx.prepare(&sql).await?);
            }
            let id = insert_stmt
                .as_mut()
                .unwrap()
                .execute((
                    record_id,
                    turso::Value::Blob(
                        vectors
                            .0
                            .iter()
                            .map(|f| f.to_le_bytes())
                            .flatten()
                            .collect(),
                    ),
                ))
                .await?;
            q.vec_row_id = Some(id);
        } else {
            if update_stmt.is_none() {
                let sql = format!(
                    "UPDATE {robot_id}_qa_vec SET vectors = ? WHERE qa_id = ?",
                    //  ON CONFLICT(rowid) DO UPDATE SET vectors = excluded.vectors;
                );
                update_stmt = Some(tx.prepare(&sql).await?);
            }
            update_stmt
                .as_mut()
                .unwrap()
                .execute((
                    turso::Value::Blob(
                        vectors
                            .0
                            .iter()
                            .map(|f| f.to_le_bytes())
                            .flatten()
                            .collect(),
                    ),
                    q.vec_row_id.unwrap(),
                ))
                .await?;
        }
    }
    tx.commit().await?;
    Ok(record_id)
}

pub(crate) async fn delete(robot_id: &str, d: QuestionAnswerPair) -> Result<()> {
    let mut conn = TURSO_DATA_SOURCE.get().unwrap().connect()?;
    let tx = conn.transaction().await?;
    let id = d.id.unwrap();
    let sql = format!("DELETE FROM {robot_id}_qa_vec WHERE qa_id = ?1");
    let mut stmt = tx.prepare(&sql).await?;
    let id = turso::Value::Integer(id as i64);
    stmt.execute([id.clone()]).await?;
    let sql = format!("DELETE FROM {robot_id}_qa WHERE id = ?1");
    let mut stmt = tx.prepare(&sql).await?;
    stmt.execute([id]).await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn retrieve_answer(
    robot_id: &str,
    question: &str,
) -> Result<(Option<QuestionAnswerPair>, f64)> {
    let vectors = embedding(robot_id, question).await?;
    if vectors.0.is_empty() {
        let err = format!("{question} embedding data is empty");
        log::warn!("{}", &err);
        return Err(Error::WithMessage(err));
    }
    let conn = TURSO_DATA_SOURCE.get().unwrap().connect()?;

    let sql = format!(
        "
        SELECT qa_data, v.distance FROM {robot_id}_qa q INNER JOIN
        (SELECT qa_id, distance FROM {robot_id} WHERE vectors MATCH ?1 ORDER BY distance ASC LIMIT 1) v
        ON q.id = v.qa_id
        "
    );
    let mut results = conn
        .query(
            &sql,
            [turso::Value::Blob(
                vectors
                    .0
                    .iter()
                    .map(|f| f.to_le_bytes())
                    .flatten()
                    .collect::<Vec<u8>>(),
            )],
        )
        .await?;
    if let Some(row) = results.next().await? {
        return Ok((
            Some(serde_json::from_str(row.get_value(0)?.as_text().unwrap())?),
            row.get_value(1)?.as_real().unwrap().clone(),
        ));
    }
    Ok((None, 1.0))
}
