# Research: AgentRecall / Recall / ctx Semantic Search 参考架构

- **Query**: 深读 AgentRecall 中的 semantic search 实现，为本地 semantic retrieval (#3) 提供架构参考
- **Scope**: external (固定 clone 源码深读)
- **Date**: 2026-08-16

## 关键澄清

用户指令提到 "AgentRecall"，但实际 sqlite-vec + candle + e5-small 的语义实现位于
**`samzong/Recall`**（Rust 项目，MIT），而非 `zszz3/AgentRecall`（Electron/TypeScript 项目，MIT）。
AgentRecall 仅使用 FTS5 trigram 全文搜索（`src/core/store/schema.ts:184-191`），无任何向量/
embedding 逻辑。本次研究覆盖两个 Rust 参考实现：**Recall**（简单直接）和 **ctx**（生产级）。

PRD `prd.md:55-57` 明确指出："Recall 的 sqlite-vec + candle + e5-small 后台 worker、ctx 的
hybrid reciprocal rank fusion"。这两个是设计意图中的参考来源。

## 许可证矩阵

| 项目 | License | 语言 | REUSE-LICENSE-AUDIT 判定 | 边界 |
|---|---|---|---|---|
| Recall (`samzong/Recall`) | MIT | Rust | adapt | 保留 MIT 声明；RRF k=10、迁移骨架可 adapt |
| ctx (`ctxrs/ctx`) | Apache-2.0 | Rust | adapt | 保留 NOTICE；加权 RRF k=60、模型 manifest 可 adapt |
| AgentRecall (`zszz3/AgentRecall`) | MIT | TypeScript | idea-only | 仅迁移 writer 原子协议思路，不复制代码 |

**审计表已知风险**（`REUSE-LICENSE-AUDIT.md:74`）：
> 向量检索引入 protoc/联网模型构建依赖 — memex LanceDB、Recall candle
> — v1.0 不含 vector/embedding；`ADR-0002` 禁止联网构建依赖。

这条约束针对 v1.0 的构建系统（candle 需要在线下载模型权重构建期不可控）。
本任务 (#3) 是 v1.0 之后的 semantic 功能，运行时下载是显式 PRD 需求（Q34/Q51），
但构建依赖仍需通过 `cargo deny` 门禁。

## 固定 clone 路径

- Recall: `C:/Users/小Q/.cache/agent-history-src/Recall` (commit 22625bf)
- ctx: `C:/Users/小Q/.cache/agent-history-src/ctx` (commit 06bc5ed)
- AgentRecall: `C:/AgentHub/project/Github_src/AgentRecall` (commit 7895144)

---

## Findings

### 1. Recall — sqlite-vec 向量存储（schema、索引、查询）

**许可证**: MIT | **判定**: adapt | **对应需求**: #3 Req 6 (向量存储)

#### Schema（`src/db/schema.rs:95-108`）

Recall 在主 SQLite DB 内直接创建 vec0 虚拟表：

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS message_vec USING vec0(
    message_id INTEGER PRIMARY KEY,
    embedding float[384]
);
```

配套的 embedding 状态表（job queue）：

```sql
CREATE TABLE IF NOT EXISTS session_embedding_state (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    status TEXT NOT NULL,              -- pending | processing | done | failed
    units_total INTEGER NOT NULL DEFAULT 0,
    units_done INTEGER NOT NULL DEFAULT 0,
    started_at INTEGER,
    finished_at INTEGER,
    last_error TEXT
);
CREATE INDEX IF NOT EXISTS idx_session_embedding_status ON session_embedding_state(status);
```

以及 background job 状态表：

```sql
CREATE TABLE IF NOT EXISTS background_job_state (
    job TEXT PRIMARY KEY,
    phase TEXT NOT NULL,               -- sync | semantic | error
    detail TEXT,
    updated_at INTEGER NOT NULL
);
```

#### sqlite-vec 注册（`src/db/schema.rs:6-12`）

```rust
pub(crate) fn register_sqlite_vec() {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    }
}
```

通过 `sqlite3_auto_extension` 全局注册，每个新 Connection 自动加载 vec0 模块。
`lib.rs:32-34` 在 `init()` 中调用一次；`db/store.rs` 的所有 `open*` 方法也调用。
依赖: `sqlite-vec = "0.1"` (`Cargo.toml:19`)。

#### 向量写入（`src/db/semantic_store.rs:12-26`）

```rust
pub(crate) fn upsert_embeddings(&self, items: &[(i64, &[f32])]) -> Result<()> {
    let tx = self.conn.unchecked_transaction()?;
    {
        let mut del = tx.prepare("DELETE FROM message_vec WHERE message_id = ?1")?;
        let mut ins = tx.prepare("INSERT INTO message_vec (message_id, embedding) VALUES (?1, ?2)")?;
        for &(message_id, embedding) in items {
            let blob = f32_slice_to_bytes(embedding);
            del.execute(rusqlite::params![message_id])?;
            ins.execute(rusqlite::params![message_id, blob])?;
        }
    }
    tx.commit()?;
    Ok(())
}
```

存储格式: `f32` 小端序 BLOB（`src/utils.rs:74-80`）：
```rust
pub(crate) fn f32_slice_to_bytes(data: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(data.len() * 4);
    for &f in data { bytes.extend_from_slice(&f.to_le_bytes()); }
    bytes
}
```

#### 向量查询（`src/db/search.rs:152-190`）

```rust
fn vec_search(&self, embedding: &[f32], filters: &SearchFilters, limit: usize) -> Result<Vec<Hit>> {
    let blob = f32_slice_to_bytes(embedding);
    let fetch_k = (limit * 5) as i64;
    // ...
    "SELECT m.session_id, MIN(mv.distance) AS best_distance
     FROM message_vec mv
     JOIN messages m ON m.id = mv.message_id
     JOIN sessions s ON s.id = m.session_id
     WHERE mv.embedding MATCH ?1
       AND k = ?2"
    // ... GROUP BY m.session_id ORDER BY best_distance
}
```

- `MATCH ?1` 传入查询向量 BLOB
- `k = ?2` 控制 ANN 搜索候选数（`limit * 5`）
- 相似度由 sqlite-vec 内部计算（默认 L2/余弦，取决于 vec0 表定义）
- 结果按 `mv.distance` ASC 排序，再 GROUP BY session 聚合

**关键观察**: Recall 的 vec0 表没有指定 `distance_metric=cosine`，使用 sqlite-vec 默认。
向量在写入前已 L2 归一化（见 embedding 推理部分），因此 L2 距离与余弦相似度等价。

**对应我们 #3 的启示**:
- vec0 虚拟表可直接嵌入主 catalog DB，与现有 FTS5 表同级
- job queue (session_embedding_state) 模式适合我们的 WriterLease / outbox 契约
- 但 Recall 没有重建投影语义——向量表是 primary，不是 catalog 的投影

---

### 2. Recall — candle + multilingual-e5-small 本地 embedding 推理

**许可证**: MIT | **判定**: adapt | **对应需求**: #3 Req 1 (官方默认模型)

#### 模型与依赖（`Cargo.toml:19-23, 46-53`）

```toml
sqlite-vec = "0.1"
hf-hub = { version = "0.5", features = ["ureq"] }
ureq = "3"
tokenizers = { version = "0.22", default-features = false }

[target.'cfg(target_os = "macos")'.dependencies]
candle-core = { version = "0.10", features = ["metal"] }
candle-nn = { version = "0.10", features = ["metal"] }
candle-transformers = { version = "0.10", features = ["metal"] }

[target.'cfg(not(target_os = "macos"))'.dependencies]
candle-core = "0.10"
candle-nn = "0.10"
candle-transformers = "0.10"
```

macOS 用 Metal 加速，其他平台 CPU。

#### 模型加载（`src/embedding.rs:9-37`）

```rust
const MODEL_ID: &str = "intfloat/multilingual-e5-small";

pub(crate) fn new(show_progress: bool) -> Result<Self> {
    let device = select_device()?;
    let (config_path, tokenizer_path, weights_path) = download_model(show_progress)?;
    let config: Config = serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)? };
    let model = BertModel::load(vb, &config)?;
    let mut tokenizer = Tokenizer::from_file(&tokenizer_path)?;
    tokenizer.with_padding(Some(PaddingParams {
        strategy: PaddingStrategy::BatchLongest,
        ..Default::default()
    }));
    tokenizer.with_truncation(Some(TruncationParams { max_length: 512, ..Default::default() }))?;
    Ok(Self { model, tokenizer, device })
}
```

- 模型: `intfloat/multilingual-e5-small`（384 维，~470MB ONNX / safetensors）
- 通过 `hf-hub` 从 HuggingFace Hub 下载（`src/embedding.rs:120-134`）
- 使用 `VarBuilder::from_mmaped_safetensors` 内存映射权重文件
- tokenizer 截断 max_length=512，padding=BatchLongest

#### 模型下载（`src/embedding.rs:120-134`）

```rust
fn download_model(show_progress: bool) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let api = hf_hub::api::sync::ApiBuilder::from_env().build()?;
    let repo = api.repo(Repo::with_revision(MODEL_ID.to_string(), RepoType::Model, "main".to_string()));
    let config = repo.get("config.json")?;
    let tokenizer = repo.get("tokenizer.json")?;
    let weights = repo.get("model.safetensors")?;
    Ok((config, tokenizer, weights))
}
```

**关键缺陷**: Recall 没有 hash 校验——依赖 hf-hub 缓存的隐式完整性。
首次使用只打印 "Downloading model..."（`show_progress`），无断点/重试/取消逻辑。

#### 推理调用（`src/embedding.rs:64-98`）

```rust
fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    let encodings = self.tokenizer.encode_batch(texts.to_vec(), true)?;
    // 构建 token_ids, attention_mask, token_type_ids tensors
    let hidden = self.model.forward(&token_ids, &token_type_ids, Some(&attention_mask))?;
    // mean pooling (attention_mask weighted)
    let mask = attention_mask.to_dtype(DTYPE)?.unsqueeze(2)?;
    let sum_mask = mask.sum(1)?;
    let pooled = hidden.broadcast_mul(&mask)?.sum(1)?;
    let pooled = pooled.broadcast_div(&sum_mask)?;
    // L2 归一化
    let norm = pooled.sqr()?.sum_keepdim(1)?.sqrt()?;
    let normalized = pooled.broadcast_div(&norm)?;
    Ok(normalized.to_vec2::<f32>()?)
}
```

- pooling: attention_mask 加权均值（非 CLS token）
- 归一化: L2
- 输出: `Vec<Vec<f32>>`，每个 384 维

#### E5 query/passage 前缀（`src/embedding.rs:39-49`）

```rust
pub(crate) fn embed_query(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    let prefixed: Vec<String> = texts.iter().map(|t| format!("query: {t}")).collect();
    // ...
}
pub(crate) fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let prefixed: Vec<String> = texts.iter().map(|t| format!("passage: {t}")).collect();
    // ...
}
```

E5 模型要求 query 前缀 `"query: "`，passage 前缀 `"passage: "`。

#### embedding 文本构建（`src/semantic.rs:115-118`）

```rust
pub(crate) fn build_embedding_text(title: &str, content: &str) -> String {
    let text = format!("{title}: {content}");
    if text.chars().count() > 500 { text.chars().take(500).collect() } else { text }
}
```

截断到 500 字符。

#### 维度与存储格式

- embedding 维度: **384**（`float[384]` in vec0 schema）
- 存储格式: `f32` little-endian BLOB，384 * 4 = 1536 bytes/vector
- 相似度: sqlite-vec 内部计算（默认 L2 距离），因向量已 L2 归一化，等价余弦

**对应我们 #3 的启示**:
- multilingual-e5-small (384 维) 是合理默认——CJK 兼容、体积小、CPU 可用
- query/passage 前缀是 E5 系列的硬性要求
- mean pooling + L2 归一化是标准 pipeline
- 但 Recall 的 hash 缺失不满足 PRD Q51 "校验 hash" 需求——需参考 ctx 的做法

---

### 3. Recall — 后台 worker 构建向量索引（不阻塞主路径）

**许可证**: MIT | **判定**: adapt | **对应需求**: #3 Req 8 (离线行为)

#### Worker 架构（`src/semantic.rs:14-63`）

```rust
pub(crate) fn ensure_background_worker(sync_first: bool) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("__background-worker");
    if sync_first { cmd.arg("--sync-first"); }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let _ = cmd.spawn()?;
    Ok(())
}
```

- Worker 是自身二进制的子进程（`__background-worker` 隐藏 CLI 命令）
- stdin/stdout/stderr 全部 null，完全脱离父进程
- 通过文件锁 `worker_lock_path()` 保证单例（`src/semantic.rs:120-140`）：

```rust
fn try_acquire_worker_lock() -> Result<Option<File>> {
    let path = worker_lock_path()?;
    let mut file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)?;
    match file.try_lock_exclusive() {
        Ok(()) => { file.set_len(0)?; writeln!(file, "{}", std::process::id())?; Ok(Some(file)) }
        Err(_) => Ok(None),  // 另一个 worker 在跑
    }
}
```

#### Worker 主循环（`src/semantic.rs:26-63`）

```rust
pub(crate) fn run_background_worker<F>(sync_first: bool, mut sync_fn: F) -> Result<()> {
    let Some(_lock) = try_acquire_worker_lock()? else { return Ok(()); };
    let store = Store::open()?;
    if sync_first {
        store.set_background_job_state(BACKGROUND_JOB, "sync", Some("Incremental sync"))?;
        sync_fn()?;
    }
    let provider = match EmbeddingProvider::new(false) {
        Ok(provider) => provider,
        Err(err) => {
            store.set_background_job_state(BACKGROUND_JOB, "error", Some(&format!("Semantic unavailable: {err:#}")))?;
            return Err(err);
        }
    };
    store.set_background_job_state(BACKGROUND_JOB, "semantic", Some(&format!("starting on {}", provider.device_name())))?;
    while process_next_session(&store, &provider)? {}
    store.clear_background_job_state(BACKGROUND_JOB)?;
    Ok(())
}
```

#### Job claim 模式（`src/db/semantic_store.rs:73-116`）

```rust
pub(crate) fn claim_next_session_embedding_job(&self) -> Result<Option<SemanticSessionJob>> {
    let now = Utc::now().timestamp_millis();
    let session_id: Option<String> = self.conn.query_row(
        "UPDATE session_embedding_state
         SET status = 'processing', started_at = COALESCE(started_at, ?1), finished_at = NULL, last_error = NULL
         WHERE session_id = (
             SELECT st.session_id FROM session_embedding_state st
             JOIN sessions s ON s.id = st.session_id
             WHERE st.status = 'pending'
             ORDER BY COALESCE(s.updated_at, s.started_at) DESC LIMIT 1
         )
         RETURNING session_id",
        rusqlite::params![now], |row| row.get(0),
    ).optional()?;
    // ...
}
```

原子 claim：`UPDATE ... RETURNING` 实现 SELECT-then-UPDATE 的原子化。

#### Embedding 处理（`src/semantic.rs:65-113`）

```rust
fn process_session(store: &Store, provider: &EmbeddingProvider, job: &SemanticSessionJob) -> Result<()> {
    let pending = store.pending_embeddable_messages(&job.session_id)?;
    for chunk in pending.chunks(SESSION_EMBED_BATCH) {  // SESSION_EMBED_BATCH = 8
        let texts: Vec<String> = chunk.iter().map(|(_, content)| build_embedding_text(&job.title, content)).collect();
        let embeddings = provider.embed_documents(&texts)?;
        let items: Vec<(i64, &[f32])> = chunk.iter().zip(embeddings.iter())
            .map(|((message_id, _), embedding)| (*message_id, embedding.as_slice())).collect();
        store.upsert_embeddings(&items)?;
        units_done += chunk.len() as u64;
        store.update_session_embedding_progress(&job.session_id, units_done)?;
        // 更新 background_job_state.detail 显示进度
    }
}
```

- batch size: 8 条消息
- 只对 `role = 'user'` 且 `LENGTH(content) > 2` 的消息做 embedding（`src/db/semantic_store.rs:28-38`）
- 通过 `pending_embeddable_messages` 的 LEFT JOIN ... IS NULL 只处理未嵌入的消息
- 每批更新进度到 `session_embedding_state.units_done`

#### Worker 触发点

| 触发位置 | 文件:行 | 说明 |
|---|---|---|
| `sync.rs:36` | `semantic::ensure_background_worker(false)` | sync 完成后触发 |
| `session.rs:336` | `semantic::ensure_background_worker(false)` | session list 后触发 |
| `tui/runner.rs:30` | `semantic::ensure_background_worker(true)` | TUI 启动时触发（带 sync） |

**对应我们 #3 的启示**:
- 自二进制子进程 + 文件锁单例是轻量但可靠的后台方案
- `UPDATE ... RETURNING` 原子 claim 避免 race
- 进度可见性（background_job_state）对 UX 重要
- 但 Recall 没有显式取消/暂停机制，只靠 kill 进程

---

### 4. ctx — 生产级语义检索架构（更成熟的参考）

**许可证**: Apache-2.0 | **判定**: adapt | **对应需求**: #3 全部

ctx 的语义子系统比 Recall 复杂得多，解决了 Recall 的多个缺陷。

#### 4.1 模型 manifest 与 hash 校验

ctx 有**两套**模型获取路径：

**CPU 路径（ONNX，跨平台）** — `preamble.rs:56-91`：
```rust
const SEMANTIC_BACKEND: &str = "multilingual-e5";
const SEMANTIC_MODEL_KEY: &str = "e5-small-v1:mean-pool:l2:query-passage";
const SEMANTIC_MODEL_ID: &str = "intfloat/multilingual-e5-small";
const SEMANTIC_MODEL_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
const SEMANTIC_REQUIRED_MODEL_FILES: &[SemanticModelFile] = &[
    SemanticModelFile::new("onnx/model.onnx", 470_268_510,
        "ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665"),
    SemanticModelFile::new("tokenizer.json", 17_082_730,
        "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39"),
    SemanticModelFile::new("config.json", 655,
        "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959"),
    SemanticModelFile::new("special_tokens_map.json", 167, "..."),
    SemanticModelFile::new("tokenizer_config.json", 443, "..."),
];
const SEMANTIC_DIMENSIONS: usize = 384;
const SEMANTIC_PASSAGE_PREFIX: &str = "passage: ";
const SEMANTIC_QUERY_PREFIX: &str = "query: ";
```

每个文件都有 **size + sha256 双重校验**（`SemanticModelFile { path, size, sha256 }`）。

**CoreML 路径（macOS）** — `model_acquisition.rs:57-76`：
```rust
pub(crate) const COREML_BUNDLE_DESCRIPTOR: CoreMlBundleDescriptor<'static> = CoreMlBundleDescriptor {
    artifact_url: "https://cli.ctx.rs/storage/v1/object/public/releases/artifacts/ctx-multilingual-e5-small-coreml-fp16-1.0.0.tar.xz",
    archive_sha256: "94c6fac5c4250079401d383adf1b10270fe5d370f2091dbad17bf4823222321e",
    manifest_sha256: "576c68756563333fdf442e6859f2392ca0065b09a2cb5d73983e30de75df1ad6",
    bundle_id: "ctx.multilingual-e5-small.coreml.fp16",
    bundle_version: "1.0.0",
    embedding_dimensions: 384,
    document_prefix: "passage: ",
    query_prefix: "query: ",
    pooling: "attention_mask_mean",
    normalization: "l2",
    // ...
};
```

- 自托管 tar.xz 包（不依赖 HF Hub 在线可用性）
- archive_sha256 + manifest_sha256 双重 hash pin
- 内容寻址缓存（content-addressed bundle path）
- 原子发布 + completion marker
- 中断修复（`repair_interrupted_cache_publication`）
- acquisition lock 防并发
- 防符号链接穿越（symlink traversal protection）
- 大小限制（MAX_ARCHIVE_BYTES = 1GB, MAX_BUNDLE_BYTES 等多层）

**manifest 结构**（`model_acquisition.rs:1352-1382` 测试中的 create_test_bundle）：
```json
{
    "schema_version": 1,
    "bundle_id": "ctx.multilingual-e5-small.coreml.fp16",
    "bundle_version": "1.0.0",
    "model": {
        "id": "intfloat/multilingual-e5-small",
        "source_revision": "614241f622f53c4eeff9890bdc4f31cfecc418b3",
        "embedding_space_id": "e5-small-v1:mean-pool:l2:query-passage",
        "precision": "fp16"
    },
    "tensor_contract": {
        "inputs": [...],
        "output": {"name": "sentence_embeddings", "dtype": "float32", "shape": [16, 384]},
        "document_batch_size": 16,
        "max_sequence_length": 512,
        "embedding_dimensions": 384,
        "document_prefix": "passage: ",
        "query_prefix": "query: ",
        "pooling": "attention_mask_mean",
        "normalization": "l2"
    },
    "artifacts": {"tokenizer": "tokenizer.json", "document_model": "document.mlpackage"},
    "files": [{"path": "...", "size_bytes": N, "sha256": "..."}]
}
```

**对应我们 #3 Req 1 (Q34/Q51)**：ctx 的 manifest 格式直接满足 PRD 的
"manifest 记录 model id、文件 hash、dimension、license、benchmark 结果"。
tensor_contract 声明了 pooling/normalization/prefix 等推理契约。

#### 4.2 向量存储 — 双层投影架构

ctx 采用**双表设计**（`vector_store_schema.rs:108-184`）：

```sql
-- 源表：完整 chunk 数据 + embedding BLOB（投影的 source of truth）
CREATE TABLE IF NOT EXISTS event_embedding_chunks (
    event_id TEXT NOT NULL,
    model_key TEXT NOT NULL,
    history_record_id TEXT,
    session_id TEXT,
    event_seq INTEGER NOT NULL,
    chunk_index INTEGER NOT NULL,
    chunk_count INTEGER NOT NULL,
    source_text_sha256 TEXT NOT NULL,
    chunk_text_sha256 TEXT NOT NULL,
    chunk_text TEXT NOT NULL DEFAULT '',   -- 空间优化后清空
    start_char INTEGER NOT NULL,
    end_char INTEGER NOT NULL,
    dimensions INTEGER NOT NULL,
    embedding_f32 BLOB NOT NULL,
    embedded_at_ms INTEGER NOT NULL,
    PRIMARY KEY (event_id, model_key, chunk_index)
);

-- 投影表：vec0 虚拟表（可从 chunks 表完全重建）
CREATE VIRTUAL TABLE IF NOT EXISTS event_embedding_vec0
    USING vec0(embedding float[384] distance_metric=cosine);

-- 元数据表：vec0 rowid → chunk 的映射
CREATE TABLE IF NOT EXISTS event_embedding_vec0_meta (
    rowid INTEGER PRIMARY KEY,
    event_id TEXT NOT NULL,
    model_key TEXT NOT NULL,
    history_record_id TEXT,
    session_id TEXT,
    event_seq INTEGER NOT NULL,
    chunk_index INTEGER NOT NULL,
    source_text_sha256 TEXT NOT NULL,
    start_char INTEGER NOT NULL,
    end_char INTEGER NOT NULL
);
```

关键设计：
- **vec0 是投影**，可从 `event_embedding_chunks` 完全重建（`rebuild_sqlite_vec0_from_chunks`，`vector_store_schema.rs:487-544`）
- 显式指定 `distance_metric=cosine`（Recall 没有指定）
- `source_text_sha256` 用于检测 stale embedding（文本变更后 hash 不匹配）
- `model_key` 支持多模型共存
- chunk-level embedding（不是 message-level），有 `chunk_index` / `chunk_count` / `start_char` / `end_char`

**向量索引重建**（`vector_store_schema.rs:474-545`）：
```rust
fn rebuild_sqlite_vec0_from_chunks(&mut self) -> Result<()> {
    self.drop_sqlite_vec0_schema()?;
    self.create_sqlite_vec0_schema()?;
    let tx = self.conn.transaction()?;
    // 从 event_embedding_chunks 读取，写入 event_embedding_vec0 + _meta
    tx.commit()?;
    Ok(())
}
```

以及自动同步检测（`sync_sqlite_vec0_from_chunks_if_needed`），通过 `sqlite_vec0_mismatch_count`
检查 meta/vector 一致性，不一致时自动重建。

**chunk 策略**（`preamble.rs:151-156`）：
```rust
const SEMANTIC_CHUNK_TARGET_CHARS: usize = 1_200;
pub(crate) const SEMANTIC_CHUNK_OVERLAP_CHARS: usize = 200;
const SEMANTIC_SOURCE_MAX_CHARS: usize = 64 * 1024;
```

1200 字符目标 + 200 字符重叠的 chunking。

**对应我们 #3 Req 3 (message/placement 粒度) + Req 6 (可重建投影)**：
ctx 的双表设计直接满足 PRD 的"向量索引必须是可从 catalog 重建的投影"。
chunk 级 embedding + start_char/end_char 满足"semantic 命中必须可回溯到 Evidence span"。

#### 4.3 Hybrid RRF 融合（加权 reciprocal rank fusion）

**ctx 的 RRF** — `crates/ctx-history-search/src/search.rs:289-291`：
```rust
fn reciprocal_rank(rank: usize) -> f32 {
    1.0 / (60.0 + rank.max(1) as f32)
}
```

k=60（标准 RRF 论文值），不同于 Recall 的 k=10。

**加权融合** — `search.rs:159-172`：
```rust
let semantic_weight = semantic_weight.clamp(0.0, 1.0);
let mut results = candidates.into_values().map(|mut candidate| {
    candidate.result.rank = if hybrid {
        let lexical = candidate.lexical_score.unwrap_or(0.0);
        let semantic = candidate.semantic_score.unwrap_or(0.0);
        ((1.0 - semantic_weight) * lexical) + (semantic_weight * semantic)
    } else {
        candidate.semantic_score.unwrap_or(candidate.result.rank)
    };
    candidate
}).collect::<Vec<_>>();
```

- 加权 RRF：`(1-w) * rr_lexical + w * rr_semantic`
- `semantic_weight` 可配置（CLI `--semantic-weight 0.0-1.0`）
- 纯 semantic 模式：直接用 cosine similarity（`search.rs:128-131`）

**来源标注** — `search.rs:149-156`：
```rust
push_unique_why(&mut entry.result.why_matched, "semantic_similarity".to_owned());
push_unique_why(&mut entry.result.why_matched, format!("semantic:{}", event_reason(hit.event_type)));
```

每个命中标注 `why_matched`，满足 PRD Req 5 "结果标注命中来源"。

**session 聚合** — `search.rs:304-318`：
```rust
fn cluster_hybrid_candidates_by_session(candidates: Vec<HybridCandidate>) -> Vec<HybridCandidate> {
    // 按 session_id 聚合，累加 more_matches_in_session 计数
}
```

满足 PRD Req 3 "session 聚合排序"。

**Recall 的 RRF** — `src/db/search.rs:263-293`：
```rust
fn rrf_merge(fts_hits: &[Hit], vec_hits: &[Hit], k: u32) -> Vec<(String, f64, MatchSource)> {
    let mut scores: HashMap<String, (f64, bool, bool)> = HashMap::new();
    for (rank, hit) in fts_hits.iter().enumerate() {
        let entry = scores.entry(hit.session_id.clone()).or_insert((0.0, false, false));
        entry.0 += 1.0 / (k as f64 + rank as f64 + 1.0);
        entry.1 = true;
    }
    for (rank, hit) in vec_hits.iter().enumerate() {
        let entry = scores.entry(hit.session_id.clone()).or_insert((0.0, false, false));
        entry.0 += 1.0 / (k as f64 + rank as f64 + 1.0);
        entry.2 = true;
    }
    // MatchSource: Hybrid | Fts | Vector
}
```

- 标准无权重 RRF：`1/(k+rank+1)`，k=10
- 简单的来源标注（Fts/Vector/Hybrid 三态）
- 无 semantic_weight 参数

**对应我们 #3 Req 5 (Hybrid 融合)**：
ctx 的加权 RRF + `why_matched` 更符合 PRD 需求。k=60 是论文推荐值，
但 PRD 说"基准对比后定"，应 benchmark k=10 vs k=60 vs 加权。

#### 4.4 降级显式化

ctx 有完整的降级报告机制 — `preamble.rs:387-409`：
```rust
pub(crate) fn to_json(&self) -> Value {
    compact_json(json!({
        "requested_mode": self.requested_mode.as_str(),
        "effective_mode": self.effective_mode.as_str(),
        "semantic_weight": self.semantic_weight,
        "semantic_status": self.semantic_status,
        "semantic_fallback_code": self.semantic_fallback_code,
        "semantic_fallback": self.semantic_fallback,
        // ...
    }))
}
```

降级代码（`preamble.rs` 中的各种 fallback）：
- `semantic_disabled` — 配置禁用
- `unsupported_platform` — 平台不支持
- `semantic_index_empty` — 索引为空
- `semantic_index_missing` — 索引不存在
- `semantic_coverage_not_ready` — 覆盖率不完整
- `semantic_coverage_unknown` — 覆盖率未知
- `model_cache_missing` — 模型缓存缺失
- `daemon_query_service_unavailable` — daemon 不可用
- `filtered_vector_lookup_unsupported` — 过滤不支持
- `term_or_semantics_unsupported` — OR 语义不支持
- `semantic_retrieval_failed` — 检索失败
- `semantic_index_open_error` — 索引打开失败

每个降级都输出 `effective_mode=lexical` + `semantic_fallback_code` + warning。

**对应我们 #3 Req 4 (降级显式化, Q54)**：ctx 的 fallback code 体系直接满足
PRD 的 `retrieval_mode=lexical_fallback` + warning + 修复建议需求。

#### 4.5 向量搜索后端（双后端）

ctx 支持两种向量搜索后端（`preamble.rs:157-159`）：
```rust
const SEMANTIC_VECTOR_BACKEND_RUST: &str = "rust_blob_scan";
const SEMANTIC_VECTOR_BACKEND_SQLITE_VEC: &str = "sqlite_vec0";
const SEMANTIC_SQLITE_VEC0_MAX_K: usize = 4_096;
```

- `sqlite_vec0`: vec0 虚拟表 ANN 查询（上限 k=4096）
- `rust_blob_scan`: 纯 Rust 暴力扫描 BLOB（无 sqlite-vec 时 fallback）

全扫描限制（`preamble.rs:155-156`）：
```rust
const SEMANTIC_FULL_SCAN_MAX_CHUNKS: usize = 250_000;
const SEMANTIC_FULL_SCAN_MAX_VECTOR_BYTES: usize = 512 * 1024 * 1024;  // 512MB
```

#### 4.6 Daemon 架构

ctx 使用 daemon 进程（而非 Recall 的 fire-and-forget 子进程）：
- Unix socket 查询服务（`preamble.rs:182-183`）
- 状态文件 `semantic-worker.json` + 锁文件 `semantic-worker.lock`
- 后台 job 调度（`daemon/jobs/` 目录）
- idle exit + autostart

---

### 5. AgentRecall — 仅 FTS5 参考（无语义检索）

**许可证**: MIT | **判定**: idea-only (TS 项目)

AgentRecall (`zszz3/AgentRecall`) 是 Electron 应用，仅使用 FTS5 trigram：

`src/core/store/schema.ts:184-191`：
```typescript
CREATE VIRTUAL TABLE IF NOT EXISTS session_fts USING fts5(
    session_key UNINDEXED,
    title,
    first_question,
    content_text,
    project_path,
    tokenize = 'trigram'
);
```

从 unicode61 迁移到 trigram 的逻辑（`schema.ts:304-322`）和 FTS refresh 函数
（`schema.ts:360-388`）可作 idea 参考。无向量/embedding 相关代码。

**对应我们 #3**: 不直接相关，已有 FTS5 实现可参考其 trigram 迁移逻辑。

---

## 建议：我们的本地 semantic retrieval 实现大纲

### A. 候选模型选择

| 模型 | 维度 | 大小 | CJK 兼容 | License | 推荐度 |
|---|---|---|---|---|---|
| `intfloat/multilingual-e5-small` | 384 | ~470MB | 优秀（多语言训练含 CJK） | MIT (model) | **首选** |
| `BAAI/bge-m3` | 1024 | ~2.2GB | 优秀（专为多语言设计） | MIT (model) | 可选（大） |
| `intfloat/multilingual-e5-base` | 768 | ~1.1GB | 优秀 | MIT | 可选（中） |

**推荐**: `multilingual-e5-small` (384 维) 作为默认模型，理由：
1. Recall 和 ctx 都用这个模型，有成熟参考
2. 384 维存储紧凑（1.5KB/vector），查询快
3. CJK 兼容性已被两个独立项目验证
4. CPU 推理性能可接受（470MB 模型）
5. PRD 要求"脱敏真实结构 fixture 上做 CJK/英文/代码混合 benchmark，达标后锁定"

推理后端选择：
- **推荐 ONNX Runtime（ctx 方案）** 而非 candle（Recall 方案）
  - ONNX Runtime 跨平台二进制分发成熟（ctx 用 `ort-sys` 动态加载）
  - candle 的 Metal/CUDA feature 会引入构建期平台依赖
  - 但 `cargo deny` 需放行 onnxruntime native 依赖
- 如果构建依赖受限，candle CPU-only 是 fallback（无 native 依赖）

### B. sqlite-vec 集成方式

**推荐**: 采纳 ctx 的双表投影架构，而非 Recall 的单表：

```
catalog.db (现有)
├── messages (现有)
├── messages_fts (现有 FTS5)
├── message_embedding_chunks (新增 — 投影 source of truth)
│   ├── message_id, model_key, chunk_index, chunk_count
│   ├── source_text_sha256, chunk_text_sha256
│   ├── start_char, end_char
│   ├── dimensions, embedding_f32 BLOB
│   └── embedded_at_ms
├── message_embedding_vec0 (新增 — vec0 虚拟表，可重建)
│   └── embedding float[384] distance_metric=cosine
├── message_embedding_vec0_meta (新增 — vec0 rowid 映射)
│   └── rowid, message_id, model_key, chunk_index, source_text_sha256, start_char, end_char
├── session_embedding_state (新增 — job queue)
│   └── session_id, status, units_total, units_done, started_at, finished_at, last_error
└── background_job_state (新增 — worker 状态)
    └── job, phase, detail, updated_at
```

与现有 SQLite catalog 的关系：
- 向量表是 catalog 的**投影**（可从 messages + model 完全重建）
- 与 FTS5 投影同级，rebuild 语义一致
- `model_key` 字段支持未来模型升级（旧 embedding 不会污染新模型查询）
- `source_text_sha256` 检测 stale embedding（消息编辑后自动标记 dirty）

sqlite-vec 注册：采纳 Recall 的 `sqlite3_auto_extension` 全局注册方式（`schema.rs:6-12`），
在 crate `init()` 中调用一次。

### C. Embedding Worker 后台化

**推荐**: 采纳 Recall 的自二进制子进程方案（比 ctx 的 daemon 轻）：

```
CLI/MCP 主路径
  ├── 用户执行 search/scan
  ├── 写入 session_embedding_state (status=pending)
  └── ensure_background_worker(false)  ← fire-and-forget spawn
        └── 子进程 (agent-session-grep __semantic-worker)
              ├── try_acquire_worker_lock()  ← fs2 文件锁单例
              ├── claim_next_session_embedding_job()  ← UPDATE...RETURNING
              ├── for each pending message:
              │   ├── build_embedding_text(title, content)
              │   ├── embed_documents([texts])  ← batch=8
              │   ├── upsert_embeddings(items)
              │   └── update_session_embedding_progress()
              └── loop until no pending
```

关键约束（满足 PRD Req 8 离线行为）：
- Worker 失败不阻塞主路径（Recall 的 `ensure_background_worker` 忽略 spawn 错误）
- `--offline` 时跳过模型下载，semantic 标记 unavailable
- 模型缓存缺失时 worker 写 `background_job_state.phase=error`，不 panic

**chunk 策略**（采纳 ctx 的参数）：
- target_chars: 1200
- overlap_chars: 200
- max_source_chars: 65536
- 仅对 `role='user'` 且 `len(content) > 2` 的消息做 embedding（Recall 策略）
- 但考虑 PRD Req 3 "message/placement 级"——可能需要对 assistant 消息也做 embedding

### D. Hybrid RRF 融合策略

**推荐**: 采纳 ctx 的加权 RRF，k=60：

```rust
fn reciprocal_rank(rank: usize) -> f32 {
    1.0 / (60.0 + rank.max(1) as f32)
}

// hybrid score = (1-w) * rr_lexical + w * rr_semantic
// default w = 0.5（benchmark 后调整）
```

- k=60 是 Cormack et al. 2009 论文推荐值
- 加权融合允许通过 `--semantic-weight` 调节 lexical vs semantic 权重
- PRD Req 5 "semantic 不得破坏 lexical 的精确命中" → 权重不应 >0.7

来源标注（采纳 ctx 的 `why_matched` 模式）：
- `lexical_exact` — FTS 精确匹配
- `semantic_similarity` — 向量相似度匹配
- `hybrid_both` — 两路都命中

session 聚合（采纳 ctx 的 cluster_by_session）：
- message/placement 级召回 → 按 session_id 聚合
- `more_matches_in_session` 计数
- 聚合后 score = max(chunk_score)

### E. 模型 Manifest 和首次下载流程

**推荐**: 采纳 ctx 的 CPU 模型 manifest 格式（比 Recall 的无校验强得多）：

```json
{
  "schema_version": 1,
  "model_key": "e5-small-v1:mean-pool:l2:query-passage",
  "model_id": "intfloat/multilingual-e5-small",
  "source_revision": "614241f622f53c4eeff9890bdc4f31cfecc418b3",
  "backend": "onnx",
  "dimensions": 384,
  "distance": "cosine",
  "normalized": true,
  "tensor_contract": {
    "max_sequence_length": 512,
    "document_batch_size": 16,
    "document_prefix": "passage: ",
    "query_prefix": "query: ",
    "pooling": "attention_mask_mean",
    "normalization": "l2"
  },
  "files": [
    {"path": "onnx/model.onnx", "size_bytes": 470268510, "sha256": "ca456c..."},
    {"path": "tokenizer.json", "size_bytes": 17082730, "sha256": "0b44a9..."},
    {"path": "config.json", "size_bytes": 655, "sha256": "691377..."}
  ],
  "license": "MIT",
  "benchmark": {
    "corpus_version": "...",
    "recall_at_5": 0.0,
    "mrr": 0.0,
    "p50_latency_ms": 0,
    "p95_latency_ms": 0
  }
}
```

首次下载流程（满足 PRD Req 1 Q51 "断点/重试/取消"）：
1. 检查本地 manifest → 缓存命中则跳过
2. 从 HuggingFace Hub 下载（`hf-hub` crate，带 revision pin）
3. 逐文件下载 + size + sha256 双重校验（采纳 ctx 的 `SemanticModelFile` 模式）
4. 下载到 staging 目录 → 校验通过后原子 rename 到最终路径
5. 写入 completion marker（采纳 ctx 的 `write_completion_marker_atomic`）
6. 失败时清理 staging（采纳 ctx 的 `repair_interrupted_cache_publication`）

降级（满足 PRD Req 4 Q54）：
- 下载失败 → `retrieval_mode=lexical_fallback` + `fallback_code=model_download_failed` + warning
- hash 校验失败 → `retrieval_mode=lexical_fallback` + `fallback_code=model_integrity_error` + warning
- CPU 不支持 → `retrieval_mode=lexical_fallback` + `fallback_code=unsupported_platform` + warning
- 索引未就绪 → `retrieval_mode=lexical_fallback` + `fallback_code=semantic_index_not_ready` + warning

## Caveats / Not Found

1. **candle vs ONNX 构建依赖未验证**: PRD 引用 `ADR-0002-platform-targets.md` 禁止联网构建依赖。
   需确认 candle CPU-only 是否通过 `cargo deny`，以及 ONNX Runtime 动态加载方案是否可接受。
   ctx 用 `ctx_semantic_fastembed` feature flag 隔离 ONNX 依赖——我们可能需要类似 feature gate。

2. **bge-m3 未深研**: PRD 提到"候选模型优先 multilingual CJK 兼容，如 bge-m3 / multilingual-e5 系"。
   bge-m3 (1024 维) 未在参考项目中使用，需独立 benchmark。1024 维存储是 384 维的 2.67x。

3. **placement 级 embedding 未在参考中实现**: Recall 只对 `role='user'` 消息做 embedding。
   PRD Req 3 要求 "message/placement 级"——placement (tool call/assistant 回复片段) 的
   embedding 策略无直接参考，需自行设计 chunk 策略。

4. **WriterLease / durable outbox 契约交互未深研**: 向量表作为投影写入时如何与现有
   WriterLease 交互、durable outbox 是否需要包含 embedding 写入，需查看现有 catalog 写入路径。

5. **Recall 的 vec0 表未指定 distance_metric**: 使用 sqlite-vec 默认（L2）。
   因向量已 L2 归一化，L2 距离排序等价余弦相似度排序，但显式指定 `distance_metric=cosine`
   （如 ctx）更清晰、更安全。

6. **AgentRecall 实际无语义检索**: 用户指令中的 "AgentRecall" 可能是 "Recall" 的误称。
   AgentRecall (zszz3) 是 Electron 应用，仅 FTS5 trigram，无向量检索。
