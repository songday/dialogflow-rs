# AGENTS.md

本文件给 AI 编码助手（以及新加入的人）用。目标：改得快、不误触发昂贵构建、不污染仓库。

## 1. 项目概览

- **单二进制应用**：Rust 后端 + Vue3 前端，最终只有 `dialogflowai(.exe)` 一个可执行文件，前端产物被编译进二进制。
- 技术栈：Rust edition **2024**（MSRV 1.88）、axum 0.8、tokio、Candle（本地模型推理）、redb / turso（存储）、reqwest（rustls）。
- 前端：Vue 3 + Vite 8 + Element Plus + UnoCSS，代码在 `frontend/`。
- 目录：`src/` 后端（`flow/`、`intent/`、`kb/`、`ai/`、`external/`、`variable/`、`robot/`、`man/`、`web/`、`test/`），`sdk/` 多语言客户端 SDK（java/javascript/python），`doc/` 文档，`frontend/` 前端，`data/` 运行期数据（已 gitignore）。
- 默认监听 `127.0.0.1:12715`，可用 `-ip` / `-port` 改。

## 2. 命令约定（优先直接敲命令，不要用 .bat）

仓库根目录下的 `r.bat` / `t.bat` / `z.bat`、`frontend/` 下的 `b.bat` / `r.bat` / `u.bat` 是**给人手工双击用的**，脚本里带 `cls`、全局 `RUST_LOG`、通配删除/覆盖拷贝等副作用。AI 助手请直接用等价命令：

| 目的 | 用这个 | 不要用 |
| --- | --- | --- |
| 编译检查（快） | `cargo check` | `cargo build`（除非确实要跑二进制） |
| 跑测试 | `cargo test`（必要时 `cargo test <filter> -- --nocapture`） | `t.bat` |
| 格式化 / 静态检查 | `cargo fmt`、`cargo clippy` | 手改格式 |
| 跑服务做接口验证 | 无头运行自己构建的 `target/.../dialogflowai.exe`，用完 `taskkill` | `r.bat`（会 `cls` 并全局打开 trace 日志） |
| 前端开发调试 | `pnpm dev`（在 `frontend/`） | — |
| 前端产物同步进后端 | **默认不做**，见第 3 节 | `frontend/b.bat` |
| 打包发布 zip | 人工确认后再 `z.bat` | — |

- 不要为了验证而做昂贵的全量 `cargo build --release`；能用 `cargo check` / `cargo test` / 单测过滤解决就用它们。
- 不要长期占用端口或后台留着服务进程；验证完把进程杀掉（`taskkill //F //IM dialogflowai.exe`）。
- 不要主动联网调用真实大模型 / 外部 API 来验证；需要时用本地假服务（临时脚本放系统临时目录，用完删掉）。

## 3. 前端改动：不要动 `src/resources/assets/`，不要跑 `b.bat`

这是**最重要的一条约束**，原因是链路耦合：

1. `frontend/b.bat` 会 `pnpm build` → 覆盖 `sdk/javascript/DialogFlowAiSDK.min.js` → **清空并全量拷贝** `src/resources/assets/`。
2. `src/resources/assets/` 里的文件带内容哈希名（`index-<hash>.js|css`），每次构建都会改名，于是 `git status` 出现一堆 D / ?? 噪音。
3. `build.rs` 会遍历 `src/resources/assets/`、生成 `src/web/asset.txt` 与 `src/web/asset.rs`（二者已 gitignore），再被 `src/web/server.rs` 用 `include!` 编进二进制 —— 也就是**改了 assets 就等于触发 Rust 重新编译**。
4. `build.rs` 的 `rerun-if-changed` 只声明了 `build.rs` 和 `resources/assets/index.html`。

因此：

- 只改前端 `frontend/src/**` 时，**只改源码**，不要执行 `pnpm build`、不要跑 `b.bat`、不要手动拷贝 `frontend/dist`。
- 需要看效果：用 `pnpm dev`（vite dev server）验证；`dist/`、`src/resources/assets/`、`src/web/asset.{rs,txt}` 都是**生成物**，不要手改、不要提交、不要为了“同步”而重建。
- 只有当用户**明确要求**把前端产物嵌进二进制（准备出包/发布）时，才执行 `frontend/b.bat`，并在跑完后如实报告 `git status` 里新增/删除了哪些 `src/resources/assets/` 文件。
- 不要用 `rm -rf src/resources/assets/*` 之类的整目录删除来“清理”。

## 4. 前后端契约与 SDK 同步

- 只改后端接口（路由、DTO、响应结构）时，检查 `frontend/` 是否依赖同一字段，以及 `sdk/` 下对应实现是否需要同步；README 中已公开的契约（如 `POST /flow/answer` 的 `stream: true` → `application/x-ndjson`、末帧 `contentSeq: null`）**不能悄悄破坏**。
- 流式协议相关改动同时涉及 `src/flow/`、`sdk/javascript`、`sdk/java`、`doc/streaming.md`，视为一组改动一起处理。
- JavaScript SDK 的校验方式：`node --check` 语法检查 + 跑 `sdk/javascript/test/*.test.mjs`，不要为此构建整个后端。

## 5. 代码风格

- 以 `cargo fmt` + `rustfmt.toml` 为准，不要手工调格式；提交前跑 `cargo fmt --check` 的精神等价物（`cargo fmt` 后看 diff）。
- 注释掉的历史依赖/大段被注释代码在 `Cargo.toml` 里是既有风格，**不要顺手大清理**。
- 中文注释/文档与英文混用是现状，跟随所改文件的既有语言。
- 错误处理沿用 `anyhow` + 现有 `Result` 约定；不要引入新的错误处理框架。

## 6. 测试

- 后端用 `cargo test`；仓库内已有大量 `#[test]` / `#[tokio::test]`，新逻辑请就近加单测，而不是只靠手动 curl。
- 改动涉及数据/存储时，用临时目录或临时数据，跑完清理，并确认 `git status` 干净（`data/`、`*.db`、`*.dat` 都不该进仓库）。
- Windows 上做浏览器行为验证用无头 Edge/Chrome（`--headless=new --dump-dom` 或 `--remote-debugging-port`），探针脚本放临时目录并在结束时删除。

## 7. 不要碰的东西

- `data/`、`*.db`、`*.dat`、`Cargo.lock`（已 gitignore）、`target/`：不要提交、不要“整理”。
- `src/web/asset.rs`、`src/web/asset.txt`：自动生成，改了下一次构建就被覆盖。
- 与本任务无关的既有改动：`git status` 里不是本次任务产生的改动不要顺手改/回滚/提交。
- 未经用户确认不要 `git commit`、`git push`、不要改 `.github/workflows/` 的触发条件。

## 8. 交付习惯

- 报告要包含：改了什么文件、跑了哪些命令、实际输出结论（通过/失败）、以及**没做什么**（例如“未执行前端构建、未重新生成 resources/assets”）。
- 不要声称验证过却没跑；跑不了就说明原因。
- 用户用中文提问时用中文回答。
