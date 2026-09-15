use std::collections::{HashMap, LinkedList};
use std::time::{SystemTime, UNIX_EPOCH};
use std::vec::Vec;

use serde::{Deserialize, Serialize};
use tokio::time::{Duration, interval};

use super::node::RuntimeNodeEnum;
use crate::ai::completion::Prompt;
use crate::db;
use crate::db_executor;
use crate::man::settings;
use crate::result::Result;
use crate::robot::crud as robot;
use crate::variable::dto::VariableValue;

/// 每机器人的会话表，表名是 `{robot_id}contexts`。
///
/// 原来是一张全局 `contexts` 表，外加一个以 `CONTEXT_KEY`（`"contexts"`）为键的
/// 全局索引行存全部 session id。索引已删除：
///
/// - 它是**冗余状态** —— 从机器人注册表 + 各机器人的表就能推导出来；
/// - 它的代价是真实的 —— 每轮对话都要多写一次，`Context::get` 里还有一处
///   "读索引 → push → 写回"的**未加锁**读-改-写。删掉索引，那个竞态直接消失。
///
/// 代价落在 [`clean_expired_session`]：从"遍历一个 Vec"变成"遍历机器人注册表"。
/// 那是每分钟一次的后台任务，每个机器人的表都很小，可以接受。
pub(crate) const TABLE_SUFFIX: &str = "contexts";

pub(crate) enum UserInputIntent {
    Unknown,
    Detected(String),
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Context {
    robot_id: String,
    pub(in crate::flow::rt) main_flow_id: String,
    session_id: String,
    pub(in crate::flow::rt) node: Option<Vec<u8>>,
    pub(in crate::flow::rt) nodes: LinkedList<String>,
    pub(crate) vars: HashMap<String, VariableValue>,
    #[serde(skip)]
    user_input_intent: Option<UserInputIntent>,
    #[serde(skip)]
    pub(crate) none_persistent_vars: HashMap<String, VariableValue>,
    #[serde(skip)]
    pub(crate) none_persistent_data: HashMap<String, String>,
    #[serde(skip)]
    pub(crate) user_media: Option<crate::ai::dto::UserMediaData>,
    last_active_time: u64,
    pub(crate) chat_history: Vec<Prompt>,
}

impl Context {
    pub(crate) fn add_answer_history(&mut self, content: &str) -> usize {
        let l = self.chat_history.len() - 1;
        self.chat_history.push(Prompt {
            role: String::from("assistant"),
            content: super::executor::HTML_TAG_REGEX
                .replace_all(content, "")
                .to_string(),
        });
        l
    }

    pub(crate) fn set_user_input_intent(&mut self, intent: String) {
        self.user_input_intent = Some(UserInputIntent::Detected(intent));
    }

    pub(crate) async fn get_user_input_intent(
        &mut self,
        req: &crate::flow::rt::dto::Request,
    ) -> Result<&UserInputIntent> {
        if self.user_input_intent.is_none()
            && req.user_input_result == crate::flow::rt::dto::UserInputResult::Successful
            && !req.user_input.is_empty()
        {
            let user_input_intent =
                crate::intent::detector::detect(&req.robot_id, &req.user_input).await?;
            if user_input_intent.is_none() {
                log::info!("No intent detected for user input: {}", req.user_input);
                self.user_input_intent = Some(UserInputIntent::Unknown);
            } else {
                log::info!(
                    "Detected intent for user input '{}': {}",
                    req.user_input,
                    user_input_intent.as_ref().unwrap()
                );
                self.user_input_intent =
                    Some(UserInputIntent::Detected(user_input_intent.unwrap()));
            }
        }
        Ok(self.user_input_intent.as_ref().unwrap())
    }
}

impl Context {
    pub(crate) async fn get(robot_id: &str, session_id: &str) -> Result<Self> {
        let last_active_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let existing: Option<Context> =
            db_executor!(db::query, robot_id, TABLE_SUFFIX, session_id)?;
        if let Some(mut ctx) = existing {
            ctx.last_active_time = last_active_time;
            return Ok(ctx);
        }
        Ok(Self {
            robot_id: String::from(robot_id),
            main_flow_id: String::with_capacity(64),
            session_id: String::from(session_id),
            node: None,
            nodes: LinkedList::new(),
            vars: HashMap::with_capacity(16),
            user_input_intent: None,
            none_persistent_vars: HashMap::with_capacity(16),
            none_persistent_data: HashMap::with_capacity(16),
            user_media: None,
            last_active_time,
            chat_history: Vec::with_capacity(16),
        })
    }

    pub(crate) async fn save(&self) -> Result<()> {
        db_executor!(
            db::write,
            &self.robot_id,
            TABLE_SUFFIX,
            self.session_id.as_str(),
            self
        )
    }

    // pub(crate) async fn clear(&mut self) -> Result<()> {
    //     self.nodes.clear();
    //     self.vars.clear();
    //     self.save().await
    // }

    pub(in crate::flow::rt) fn no_node(&self) -> bool {
        self.node.is_none() && self.nodes.is_empty()
    }

    pub(in crate::flow::rt) fn add_node(&mut self, node_id: &str) {
        // print!("add_node {} ", node_id);
        self.nodes.push_front(String::from(node_id));
    }

    pub(in crate::flow::rt) async fn pop_node(&mut self) -> Option<RuntimeNodeEnum> {
        // log::info!("nodes len {}", self.nodes.len());
        if self.node.is_some() {
            let node = Option::take(&mut self.node);
            let v = node.unwrap();
            match crate::flow::rt::node::deser_node(v.as_ref()) {
                Ok(n) => return Some(n),
                Err(e) => {
                    log::error!("pop_node failed err: {:?}", &e);
                }
            }
        }
        if let Some(node_id) = self.nodes.pop_front() {
            // log::info!("main_flow_id {} node_id {}", &self.main_flow_id, &node_id);
            let store = match db::store(db::StoreKey::Robot(&self.robot_id)).await {
                Ok(store) => store,
                Err(e) => {
                    log::error!("Resolving store of robot {} failed: {e:?}", self.robot_id);
                    return None;
                }
            };
            if let Ok(r) = super::crud::get_runtime_node(store, &self.main_flow_id, &node_id).await
            {
                return r;
            }
        }
        None
    }
}

/// 新机器人的会话表。
pub(crate) async fn init(robot_id: &str) -> Result<()> {
    db_executor!(db::init_table, robot_id, TABLE_SUFFIX,)
}

pub async fn clean_expired_session(mut recv: tokio::sync::oneshot::Receiver<()>) {
    let mut interval = interval(Duration::from_secs(60));
    loop {
        // https://docs.rs/tokio/latest/tokio/sync/oneshot/index.html
        // https://users.rust-lang.org/t/how-can-i-terminate-a-tokio-task-even-if-its-not-finished/40641
        tokio::select! {
          _ = interval.tick() => {
          }
          _ = &mut recv => {
            log::info!("Closing this app.");
            break;
          }
        }
        if let Err(e) = clean_once().await {
            log::error!("Cleaning expired sessions failed: {e:?}");
        }
    }
}

/// 扫一遍所有机器人的会话表。
async fn clean_once() -> Result<()> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let robots: Vec<crate::robot::dto::RobotData> = async {
        db::get_all(db::global_store().await?, robot::TABLE).await
    }
    .await?;
    for r in robots.iter() {
        let store = db::store(db::StoreKey::Robot(&r.robot_id)).await?;
        clean_robot_sessions(store, &r.robot_id, now).await?;
    }
    Ok(())
}

/// 清理一个机器人里已过期的会话。
///
/// 设置表现在是每机器人一张，所以每个机器人取一次就够了 —— 原来得在 session
/// 循环里逐条查设置。
async fn clean_robot_sessions(
    store: &db::RedbStore,
    robot_id: &str,
    now: u64,
) -> Result<()> {
    let sessions: Vec<Context> = db_executor!(db::get_all, robot_id, TABLE_SUFFIX,)?;
    if sessions.is_empty() {
        return Ok(());
    }
    let max_idle = match settings::get_settings(robot_id).await? {
        Some(s) => 86400u64.min(s.max_session_idle_sec as u64) /* 1 day */,
        None => {
            // 机器人设置没了，说明它已经被删掉（或从没建全）—— 会话一并清掉。
            log::info!("Settings of robot {robot_id} is missing, discarding its sessions");
            0
        }
    };
    for c in sessions.iter() {
        if now.saturating_sub(c.last_active_time) > max_idle {
            if let Err(e) =
                db_executor!(db::remove, robot_id, TABLE_SUFFIX, c.session_id.as_str())
            {
                log::warn!("Discarding expired session {} failed {e:?}", c.session_id);
            } else {
                log::info!("Discarded expired session: {}", c.session_id);
            }
        }
    }
    Ok(())
}
