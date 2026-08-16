# Research: Local Embedding Model Options for agent-session-grep（2026-08-16）

- **Query**: 调研 2026 年 Rust 本地 embedding 运行时、适合中英混合 agent transcript 的模型、竞品分发策略，并给出 agent-session-grep 的落地/不落地建议
- **Scope**: external（crates.io / GitHub / Hugging Face / arXiv）+ internal（当前 `EmbeddingModel` 契约与本地固定版本竞品源码）
- **Date**: 2026-08-16
- **Current contract**: `EmbeddingModel::embed(text, is_query) -> Vec<f32>`；当前 `bigram-hash-v1` 是 384 维 fuzzy lexical vectorizer，不是真语义；`message_vec` 按 `model_id` 隔离
- **Method**: 先用 `grok-search-rs web_search` 搜索；Grok Responses 端点返回 HTTP 401，工具自动退回 Tavily source fallback。版本、许可、二进制下载行为等关键事实随后均用 crates.io API + 上游 Cargo/README/LICENSE 或 Hugging Face API + pinned model card 交叉核验。报告不把 fallback 的 AI 摘要当证据，只引用直接来源。

---

## 0. 一句话结论

**现在不应把真实语义模型放进默认构建；若开实验入口，首选 `candle` CPU + pinned `intfloat/multilingual-e5-small`（fp32 safetensors），Cargo feature 建议 `semantic-candle`、默认关闭，模型只能由用户显式 `model install` 或本地目录导入。** 这是唯一同时守住“构建时零下载、运行时不隐式联网、Windows/Linux/macOS、无额外运行时 DLL、现有 384 维/E5 前缀契约不变”的现实路线；代价是约 449 MiB 权重 + 16 MiB tokenizer、CPU 索引成本和 512-token 截断，必须先过项目自己的中英混合 recall benchmark，未过就继续把 `bigram-hash-v1` 明确定位为“模糊词法”，不宣传为 semantic。

来源：当前代码契约（`C:/AgentSessions/crates/agent-session-grep-application/src/embedding.rs`）；[Candle README/CPU backend](https://github.com/huggingface/candle)、[Candle 三平台 CI](https://github.com/huggingface/candle/blob/main/.github/workflows/rust-ci.yml)、[multilingual-e5-small model card](https://huggingface.co/intfloat/multilingual-e5-small)、[pinned model revision API](https://huggingface.co/api/models/intfloat/multilingual-e5-small)。

---

## 1. Rust 本地 embedding 运行时（2026-08-16 快照）

### 1.1 对比总表

| 方案 | 当前版本 / 活跃度 | 许可 | CPU | 构建/运行时二进制现实 | 三平台现实 | 对本项目判断 |
|---|---|---|---|---|---|---|
| `ort` | `2.0.0-rc.13`（2026-07-28），仓库 2026-08 仍活跃；但 2.0 仍是 RC | crate `MIT OR Apache-2.0`；ONNX Runtime `MIT` | 是 | **默认 feature `download-binaries` 会在 `cargo build` 下载预编译 ORT**；关掉后必须自行静态链接、系统链接或 `load-dynamic` 在运行时提供 DLL/so/dylib | 原生库按 target/EP 分发，交叉编译和 Intel macOS 等非默认 artifact 需要自备 ORT；Windows DLL 搜索/复制尤其麻烦 | 推理成熟、INT8 ONNX 体积小，但默认行为直接违反“构建时不下载”；只适合 release archive 自带 runtime 或高级用户 `load-dynamic`，不适合默认 `cargo install` |
| `candle` | `0.11.0`（2026-06-26），仓库 2026-08 仍活跃 | `MIT OR Apache-2.0` | 是，默认 CPU；MKL/Accelerate/Metal/CUDA 都是可选 | 不需要 ONNX Runtime/C++ 推理库或运行时 DLL，不自动下载模型；但 `tokenizers/onig` 等依赖仍可能编译少量原生代码，不能称“整个依赖树 100% Rust” | 上游 CI 对 Ubuntu、Windows、macOS 做 CPU check/test；GPU feature 会显著增加平台负担 | **最符合离线/Cargo 原则**；缺点是要自己写 BERT pooling/tokenization glue，fp32 E5 权重大，性能通常不及 ORT INT8 |
| `fastembed`（crate 名） | `5.17.4`（2026-07-28），仓库 2026-08 活跃 | Apache-2.0 | 是 | E5/BGE 等普通模型底层仍是 `ort`；**默认 features 同时开启 ORT build-time binary download 和 Hugging Face 支持**；模型默认首次使用下载并缓存。可 `default-features=false` + `ort-load-dynamic` + user-defined local model | Linux 有完整测试；Windows 有 DirectML feature clippy；实际跨平台仍继承 ORT 二进制矩阵和 loader 问题 | API 最省代码，且直接支持 E5；但默认行为与本项目原则冲突。若用，必须禁掉默认 features，结果又退化成“用户另装 ORT runtime” |
| `llama-cpp-2` | `0.1.154`（2026-08-05），仓库持续跟随 llama.cpp；README 明示不严格遵循 semver | wrapper `MIT OR Apache-2.0`；llama.cpp `MIT` | 是，CPU 为基本路径 | crate 内带 llama.cpp C/C++ 源码，`build.rs` 用 CMake/cc/bindgen 编译；不需要下载 ORT，但需要完整 C/C++/CMake/libclang 工具链。模型是外部 GGUF | Windows MSVC 可做但编译重；Linux/macOS 也要 C++ toolchain；交叉编译最难。macOS arm64 默认启 Metal | 能做 embedding 且 GGUF 自带 tokenizer，但为 384 维 E5 引入整个 llama.cpp 栈过重；模型/架构转换与兼容性不如 BERT/Candle 直接 |

**版本与许可交叉验证**：

- `ort`: [crates.io](https://crates.io/crates/ort/2.0.0-rc.13) + [Cargo.toml（version/license/default features）](https://github.com/pykeio/ort/blob/main/Cargo.toml) + [ONNX Runtime LICENSE](https://github.com/microsoft/onnxruntime/blob/main/LICENSE)。
- `candle`: [crates.io](https://crates.io/crates/candle-core/0.11.0) + [repository LICENSE/README](https://github.com/huggingface/candle) + [三平台 CI](https://github.com/huggingface/candle/blob/main/.github/workflows/rust-ci.yml)。
- `fastembed`: [crates.io](https://crates.io/crates/fastembed/5.17.4) + [Cargo.toml（默认 feature 与精确 `ort` 版本）](https://github.com/Anush008/fastembed-rs/blob/main/Cargo.toml) + [README（model cache / local model / supported models）](https://github.com/Anush008/fastembed-rs)。
- `llama-cpp-2`: [crates.io](https://crates.io/crates/llama-cpp-2/0.1.154) + [wrapper Cargo.toml](https://github.com/utilityai/llama-cpp-rs/blob/main/llama-cpp-2/Cargo.toml) + [sys Cargo.toml（CMake/bindgen/cc）](https://github.com/utilityai/llama-cpp-rs/blob/main/llama-cpp-sys-2/Cargo.toml) + [llama.cpp LICENSE](https://github.com/ggml-org/llama.cpp/blob/master/LICENSE)。

### 1.2 `ort`：性能/模型兼容最好，但与 `cargo install` 的离线承诺冲突

当前 `ort` 的默认 features 包含 `download-binaries`；官方文档明确说它会从 pyke CDN 下载预编译 ONNX Runtime，禁用后要自己编译并链接。`load-dynamic` 可以让进程启动不依赖 DLL 存在，并由 `ORT_DYLIB_PATH` 或代码路径在运行时加载。这三条同时由 [Cargo features 文档](https://ort.pyke.io/setup/cargo-features)、[linking 文档](https://ort.pyke.io/setup/linking) 和 [Cargo.toml](https://github.com/pykeio/ort/blob/main/Cargo.toml) 交叉确认。

因此只有三种现实包装：

1. **默认 `download-binaries`**：开发者体验好，但 `cargo build/install` 联网，直接不合规。
2. **静态链接**：构建机必须提前具备目标平台 ORT 静态库；交叉编译要维护每个平台/架构 artifact，CI 很重。
3. **`load-dynamic`**：构建不下载，但用户必须另行安装/提供 ORT 动态库；`cargo install --features semantic-onnx` 不再“装完就能用”。

官方 prebuilt 文档还明确提醒：ONNX Runtime 是“huge library”、源码编译超过一小时，预编译 x86 baseline/EP 组合会排除部分用户；当前 `dist.tsv` 只覆盖选择性 target/EP 组合，而不是全 target 矩阵。[Prebuilt binaries](https://ort.pyke.io/misc/prebuilt-binaries)、[current `dist.tsv`](https://github.com/pykeio/ort/blob/main/ort-sys/build/download/dist.tsv)。

#### 静态体积实测（官方 rc.13 / ONNX Runtime 1.28 artifact）

本次只读调研下载并解包了 `dist.tsv` 指向的官方 artifact（未写入仓库）：

| target artifact | 压缩包 | 内含原生库 | 含义 |
|---|---:|---:|---|
| Linux x86_64 CPU | 9.59 MiB | `libonnxruntime.a` **100.60 MiB** | 链接器/LTO 会裁剪，不能把 100.60 MiB 直接当最终 exe 增量；但静态输入库已是百 MiB 级 |
| macOS arm64 CoreML | 8.81 MiB | `libonnxruntime.a` **76.70 MiB** | 同上 |
| Windows x86_64 DirectML | 29.64 MiB | `onnxruntime.lib` **325.35 MiB** + `DirectML.dll` **17.67 MiB** | Windows `.lib` 是最重构建输入；最终 exe 增量取决于死代码消除，动态分发仍要带 DLL |

所以“静态链接 ORT 大概多少 MB”的诚实答案是：**官方当前静态输入库约 77–325 MiB；最终 stripped executable 通常是“增加数十 MiB”，但没有不经实际 release build 就可信的单一数字。** 把“10–30 MiB 压缩下载包”当成最终二进制大小是错误的。来源同上，并与 `ort` 官方“huge / prefer researched build config”的说明交叉验证。

### 1.3 `candle`：最符合离线构建，但工程工作和模型体积转移到应用侧

Candle 的 CPU backend 默认可用，CUDA/Metal/MKL/Accelerate 是 opt-in；上游 `rust-ci.yml` 对 Ubuntu/Windows/macOS 执行 workspace check/test，说明 Windows MSVC 并非二等平台。[README](https://github.com/huggingface/candle)、[CPU install guide](https://huggingface.github.io/candle/guide/installation.html)、[CI](https://github.com/huggingface/candle/blob/main/.github/workflows/rust-ci.yml)。

优点：

- 不下载或链接 ONNX Runtime；`cargo install` 本身可以保持离线（Cargo 获取 crate 依赖这一常规行为除外）。
- `candle-transformers` 已有 BERT；E5 只需 tokenizer → BERT → attention-mask mean pooling → L2 normalize，`Recall` 已证明这个组合在真实 Rust 会话搜索工具里可工作。[Recall `embedding.rs`](https://github.com/samzong/Recall/blob/22625bf8979a185d9913c1bfb20290ac466d492b/src/embedding.rs)。
- CPU 是完整实现，不假设 GPU。

代价：

- `multilingual-e5-small` 的 safetensors 是 470,641,600 bytes（448.84 MiB）；失去 ONNX INT8 的约 4× 体积优势。
- 要自己固定 tokenizer/padding/truncation/pooling/normalization 语义；这恰恰是 vector space identity 的一部分。
- `tokenizers`/onig 等依赖可能仍编译原生代码；“Candle 推理内核纯 Rust”不等于整个构建完全没有 C toolchain。
- 现有 trait 一次只 embed 一条文本，真实模型吞吐需要 batching；若直接逐条调用，索引速度会明显差于 `Recall` 的 batch=8 路径。

### 1.4 `fastembed`：适合原型，不适合照默认配置进入本项目

当前 `fastembed` 直接列出 `intfloat/multilingual-e5-small`，并提供 `try_new_from_user_defined(...)` 加载本地文件；但 README 同时明确默认模型“first use download and cache”，Cargo.toml 默认 feature 是 `ort-download-binaries-native-tls + hf-hub-native-tls + image-models`。[README](https://github.com/Anush008/fastembed-rs)、[Cargo.toml](https://github.com/Anush008/fastembed-rs/blob/main/Cargo.toml)。

可合规配置只能是：`default-features=false`、禁用 `hf-hub`、启 `ort-load-dynamic`、只允许 user-defined local model。这样虽然省掉 tokenizer/ONNX glue，却同时把 ORT DLL 安装责任推给用户；与直接 `ort` 相比只是 API 更高层，不消除核心分发问题。

### 1.5 `llama-cpp-2`：可行但不匹配问题形状

llama.cpp 有正式 embedding example、默认 L2 normalize，GGUF 内可携带 tokenizer metadata；wrapper 当前紧跟上游且 CPU 可运行。[llama.cpp embedding example](https://github.com/ggml-org/llama.cpp/tree/master/examples/embedding)、[`llama-cpp-2` docs](https://docs.rs/llama-cpp-2/latest/llama_cpp_2/)。

但 `llama-cpp-sys-2` 的 build dependencies 包含 `bindgen`、`cc`、`cmake`，默认还启 OpenMP；`common` feature 的注释称其静态库约 14 MiB。模型必须是 llama.cpp 支持的 GGUF 架构，不是直接加载现成 E5 ONNX/safetensors。对一个 384 维 BERT encoder，这是“用 LLM 推理基础设施解决小 encoder 问题”，Windows/MSVC 和交叉编译成本都不划算。[sys Cargo.toml](https://github.com/utilityai/llama-cpp-rs/blob/main/llama-cpp-sys-2/Cargo.toml)、[build.rs](https://github.com/utilityai/llama-cpp-rs/blob/main/llama-cpp-sys-2/build.rs)。

---

## 2. 模型选择

### 2.1 候选对比

| 模型 | 许可 | native dimension / context | 主文件体积（HF 当前 pinned tree） | 中英/多语言证据 | 与当前契约的 fit |
|---|---|---|---:|---|---|
| **`intfloat/multilingual-e5-small`** | **MIT** | **384 / 512 tokens** | safetensors 448.84 MiB；fp32 ONNX 448.48 MiB；官方 AVX512-VNNI qint8 ONNX 112.86 MiB；tokenizer.json 16.29 MiB | 100 languages；训练含中文 DuReader、MIRACL；Mr. TyDi avg MRR@10 64.4，日/韩 55.4/54.3 | **最佳**：维度和 `query:`/`passage:` 完全对齐；模型老但成熟、可重现 |
| `BAAI/bge-m3` | MIT | 1024 / 8192 | official ONNX external data 2.11 GiB + tokenizer 16.31 MiB | 100+ languages；dense/sparse/ColBERT；论文报告多语/跨语/长文领先 | 质量更强但不是同尺寸；维度、权重、推理成本都大幅上升，现有 bool 前缀契约也不是它的核心优势 |
| `Alibaba-NLP/gte-multilingual-base` | Apache-2.0 | 768，可截取 128–768 / 8192 | safetensors 582.46 MiB + tokenizer 16.29 MiB | 论文称同 base size 超过既有 XLM-R，并匹配大尺寸 BGE-M3、长文更优 | 可输出 384，但模型卡依赖 `trust_remote_code`/CLS pooling 语义，Rust 适配和等价性验证高于 E5；“截 384 后质量”需单独 benchmark |
| `Qwen/Qwen3-Embedding-0.6B` | Apache-2.0 | 1024，MRL / 32K | safetensors 1,136.39 MiB + tokenizer 10.89 MiB | 100+ natural/programming languages；模型卡报告 multilingual MTEB 64.33、English 70.70、C-MTEB 66.33 | 明显更强但约 0.6B/1.1 GiB；query 要固定 task instruction + last-token pooling，不是 E5 的两前缀契约；可裁 384 仍需新 benchmark/model_id |
| `jinaai/jina-embeddings-v3` | **CC BY-NC 4.0** | 1024，MRL 32–1024 / 8192 | fp16 ONNX 1,094.01 MiB + tokenizer 16.29 MiB | 30 个重点语言含中英，task LoRA | **排除**：非商业许可不适合作为 MIT/Apache 工具的通用默认分发；还需 task_id/LoRA 语义 |
| `minishlab/potion-multilingual-128M`（搜索中发现） | MIT | 256 / static | safetensors/ONNX 488.63 MiB + tokenizer 17.75 MiB | 101 languages；model card 报 multilingual MTEB mean task 47.31 | `model2vec-rs` 可 pure local/`local-only`，推理极快；但不是 384 维，权重并不小，公开质量低于现代 encoder，不能证明比 E5 更适合本语料 |
| `google/embeddinggemma-300m`（fastembed 当前支持） | Gemma custom license；HF manual gated | 768/可裁剪，模型相关上下文 | safetensors 1,155.36 MiB + tokenizer 31.84 MiB | 新、多语、轻量 LLM encoder 路线 | gated/custom license + 1.1 GiB，不适合作为项目首个可再分发模型 |

文件大小与许可均由 **HF API tree/metadata + pinned raw model card** 交叉核验：

- E5: [model card](https://huggingface.co/intfloat/multilingual-e5-small)、[API metadata](https://huggingface.co/api/models/intfloat/multilingual-e5-small)、[file tree](https://huggingface.co/api/models/intfloat/multilingual-e5-small/tree/main?recursive=true)、revision `614241f622f53c4eeff9890bdc4f31cfecc418b3`。
- BGE-M3: [model card](https://huggingface.co/BAAI/bge-m3)、[paper](https://arxiv.org/abs/2402.03216)、[file tree](https://huggingface.co/api/models/BAAI/bge-m3/tree/main?recursive=true)。
- GTE: [model card](https://huggingface.co/Alibaba-NLP/gte-multilingual-base)、[paper](https://arxiv.org/abs/2407.19669)、[file tree](https://huggingface.co/api/models/Alibaba-NLP/gte-multilingual-base/tree/main?recursive=true)。
- Qwen3: [model card](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B)、[paper](https://arxiv.org/abs/2506.05176)、[file tree](https://huggingface.co/api/models/Qwen/Qwen3-Embedding-0.6B/tree/main?recursive=true)。精确 leaderboard 数字来自模型作者的 model card，**仅单源，未对 2026-08-16 的动态 leaderboard 快照交叉验证**；本报告不以这些数字决定首发模型。
- Jina: [model card/license](https://huggingface.co/jinaai/jina-embeddings-v3)、[API metadata](https://huggingface.co/api/models/jinaai/jina-embeddings-v3)。
- Model2Vec: [model card/results](https://huggingface.co/minishlab/potion-multilingual-128M)、[`model2vec-rs` README/local-only](https://github.com/MinishLab/model2vec-rs)。精确 MTEB 数字同样是模型作者单源，未作为推荐依据。

### 2.2 `multilingual-e5-small` 的许可与分发

HF metadata 和 pinned README YAML 都声明 `license: mit`；因此开源项目可随 release archive 分发模型或让用户自行下载，但必须保留 MIT notice，并把“model weights license”与 runtime/crate license 分开列出。它不是 gated 模型。[API](https://huggingface.co/api/models/intfloat/multilingual-e5-small)、[pinned README](https://huggingface.co/intfloat/multilingual-e5-small/raw/614241f622f53c4eeff9890bdc4f31cfecc418b3/README.md)。

大小要区分：

- Candle 路线：`model.safetensors` 470,641,600 bytes（448.84 MiB）。
- ORT fp32：`onnx/model.onnx` 470,268,510 bytes（448.48 MiB）。
- 官方 qint8：`onnx/model_qint8_avx512_vnni.onnx` 118,346,824 bytes（112.86 MiB），**名字明确要求 AVX512-VNNI，不是通用 Windows/Linux/macOS CPU artifact**。
- `Xenova/multilingual-e5-small` 有约 112.6–112.8 MiB 的 generic int8/uint8 community conversions，但其 HF metadata 没有明确 license 字段；不能把原模型 MIT 自动等同于转换仓库的完整分发元数据。若采用，必须自己固定转换 provenance、验证输出并补齐许可来源。[Xenova file tree](https://huggingface.co/api/models/Xenova/multilingual-e5-small/tree/main?recursive=true)。

所以 Candle 推荐虽然“二进制干净”，模型 bundle 实际约 **465 MiB（权重 + tokenizer）**；这本身就是不默认开启的主要理由。

### 2.3 中英混合表现：E5 是稳妥基线，不是 2026 年质量冠军

E5-small 的直接证据是：100 languages、中文示例、中文 DuReader + 多语 MIRACL 监督数据、Mr. TyDi avg MRR@10 64.4；这些同时见 [model card](https://huggingface.co/intfloat/multilingual-e5-small) 和 [technical report](https://arxiv.org/abs/2402.05672)。它的优势是“小 BERT encoder + 384 维 + 成熟前缀契约”，不是绝对质量。

BGE-M3、mGTE、Qwen3 的论文/模型卡均报告更强的现代多语、跨语、长文能力，但代价分别是 1024/768/1024 native dimensions、约 0.58–2.1 GiB 模型或更复杂 pooling/instruction。对本项目“消息级、离线 CPU、384 维、中英混合”的约束，没有发现一个 **同为约 100–120M 参数、384 维、许可宽松、通用 CPU artifact、Rust 直接支持、公开证据明显全面优于 E5-small** 的 drop-in 替代。

最重要的负面结论：**没有公开 benchmark 精确模拟 AI coding-agent transcript（中英自然语言 + 代码标识符 + 路径 + tool JSON + 错误日志）**。MTEB/MIRACL/Mr. TyDi 只能做代理。因此任何“更强模型”结论都必须经本项目自己的 labeled query→relevant-message recall@10 / MRR / CJK slice 验证，不能按排行榜直接替换。

### 2.4 tokenizer

Rust 侧直接用 Hugging Face `tokenizers` crate（当前 `0.23.1`，Apache-2.0）即可；`Tokenizer::from_file` / `from_bytes` 可加载完整 `tokenizer.json`。HF 文档说明 `tokenizer.json` 可以保存词表、normalizer、pre-tokenizer、post-processor 等完整流水线；因此不需要 Python，也不必须额外使用 sentencepiece `.model`，前提是 pinned `tokenizer.json` 已包含完整配置。[docs.rs API](https://docs.rs/tokenizers/latest/tokenizers/tokenizer/struct.Tokenizer.html)、[HF Tokenizers quicktour](https://huggingface.co/docs/tokenizers/main/en/quicktour)。

对 E5 建议 bundle 至少固定并校验：

- `model.safetensors`
- `tokenizer.json`
- `config.json`
- `tokenizer_config.json`
- `special_tokens_map.json`

`tokenizer.json` 自身约 16.29 MiB；其他 JSON 很小。`sentencepiece.bpe.model` 可作为 provenance/兼容资产，但若只走 `Tokenizer::from_file(tokenizer.json)` 不是运行必要文件。**注意：tokenizer 内容一旦变化，即使权重没变，向量空间也应视为新 pipeline。**

---

## 3. 竞品与通行做法

### 3.1 成熟先例：默认 lexical，语义显式 opt-in

`ctx` 是最接近本项目原则的先例：

- `SEMANTIC_SEARCH_DEFAULT_ENABLED = false`；未启用时默认 backend 是 lexical，启用后默认 hybrid。
- help 明写“默认 lexical，除非 config 启用 local semantic”。
- semantic 模型固定为 `intfloat/multilingual-e5-small@614241...`，固定 384 维、query/passage prefix、每个文件的 size + SHA-256。
- 搜索路径不会因 cache 缺失而下载模型；daemon/setup 在 semantic 已显式启用时才允许下载。
- CPU backend 使用 `fastembed + ort load-dynamic`；动态库可以由 release package、环境变量或 runtime cache 提供。

来源（固定 commit `06bc5ed17ce4d0f6c8255981e009f86802dbe32f`）：[`config.rs`](https://github.com/ctxrs/ctx/blob/06bc5ed17ce4d0f6c8255981e009f86802dbe32f/crates/ctx-cli/src/config.rs)、[`main.rs` help](https://github.com/ctxrs/ctx/blob/06bc5ed17ce4d0f6c8255981e009f86802dbe32f/crates/ctx-cli/src/main.rs)、[`search.rs`](https://github.com/ctxrs/ctx/blob/06bc5ed17ce4d0f6c8255981e009f86802dbe32f/crates/ctx-cli/src/commands/search.rs)、[`semantic/preamble.rs`](https://github.com/ctxrs/ctx/blob/06bc5ed17ce4d0f6c8255981e009f86802dbe32f/crates/ctx-cli/src/semantic/preamble.rs)、[`cpu_model_cache.rs`](https://github.com/ctxrs/ctx/blob/06bc5ed17ce4d0f6c8255981e009f86802dbe32f/crates/ctx-cli/src/semantic/cpu_model_cache.rs)、[`ort_runtime.rs`](https://github.com/ctxrs/ctx/blob/06bc5ed17ce4d0f6c8255981e009f86802dbe32f/crates/ctx-cli/src/semantic/ort_runtime.rs)。

这个先例证明“**默认 lexical + config opt-in + 模型显式获取 + hash 校验**”成熟可行；但它也显示 ORT 会膨胀出大量平台/runtime discovery 代码，不应轻视。

### 3.2 First-use download 先例：方便，但不适合照搬

`Recall` 直接依赖 Candle + `hf-hub` + `tokenizers` + sqlite-vec，固定 E5-small；`EmbeddingProvider::new()` 调 `download_model()`，首次运行从 HF 获取 `config.json/tokenizer.json/model.safetensors`，随后 mmap safetensors。它是“纯本地推理”的成功案例，但 `sync`/后台 worker 会初始化 provider，因此可能在用户未执行独立模型安装命令时触发网络。[Cargo.toml](https://github.com/samzong/Recall/blob/22625bf8979a185d9913c1bfb20290ac466d492b/Cargo.toml)、[`embedding.rs`](https://github.com/samzong/Recall/blob/22625bf8979a185d9913c1bfb20290ac466d492b/src/embedding.rs)、[`semantic.rs`](https://github.com/samzong/Recall/blob/22625bf8979a185d9913c1bfb20290ac466d492b/src/semantic.rs)。

`fastembed` 自身也是同一路线：first use 下载、cache 后离线。[README model cache](https://github.com/Anush008/fastembed-rs#model-cache)。

对 agent-session-grep，这种“第一次 semantic/search/sync 隐式下载”不可接受；只有用户显式 `model install`（命令名字本身表明联网）才满足原则。

### 3.3 用户提供路径先例

- `fastembed` 提供 `try_new_from_user_defined(...)`。
- `model2vec-rs` 支持本地 path、`from_bytes` 和 `local-only` feature。
- llama.cpp/`llama-cpp-2` 的常规用法就是用户提供 GGUF 路径，tokenizer 随 GGUF。

来源：[fastembed local models](https://github.com/Anush008/fastembed-rs#locally-available-models)、[model2vec-rs README](https://github.com/MinishLab/model2vec-rs)、[llama.cpp embedding example](https://github.com/ggml-org/llama.cpp/tree/master/examples/embedding)。

这是最严格离线方案，但纯“只给路径”用户体验较差。最佳组合是：**支持本地 import/path 为基础；另提供一个用户主动调用、带 hash/license 提示的 install 命令。**

### 3.4 随主二进制打包：本次样本没有采用

在本次会话历史搜索样本中，没有一个把 100–500+ MiB embedding 权重 `include_bytes!` 进主 CLI：

- `ctx`/`Recall` 下载到 cache；
- cass 使用独立 model install/cache；
- hstry 仅预留 embedding 表，实际搜索是双 FTS5；
- sessiongrep 是 FTS 候选 + 应用层 fuzzy rerank；
- claude-historian-mcp 用启发式 `semanticBoosts`，没有真实 embedding。

来源：[ctx model cache](https://github.com/ctxrs/ctx/tree/06bc5ed17ce4d0f6c8255981e009f86802dbe32f/crates/ctx-cli/src/semantic)、[Recall embedding](https://github.com/samzong/Recall/blob/22625bf8979a185d9913c1bfb20290ac466d492b/src/embedding.rs)、[hstry](https://github.com/byteowlz/hstry/tree/88b78b1f3a84723e31a2c444bec326829fd02459)、[sessiongrep](https://github.com/braincompany/sessiongrep/tree/c5c1874f420fb20cc55e484f79bdff3d848b71a8)、[claude-historian-mcp](https://github.com/Vvkmnn/claude-historian-mcp/tree/b627cdfbfe46040368ed9c3f5bb2b531c65a8988)。

原因很现实：主 binary/release 被模型放大几十倍；每个平台重复分发同一权重；Cargo/crates.io 包不适合大模型；模型升级迫使整个 CLI 重发；模型 license notice 和供应链验证也与代码 release 耦合。

### 3.5 明确“为什么不做/为什么撤掉 ONNX”的负面证据

cass 提供了最强负面证据：

1. 旧 `fastembed/ort` 预编译 vendor binary 在 pre-AVX2 Windows CPU 静态初始化时 `STATUS_ILLEGAL_INSTRUCTION`；仅给 Rust 设置 `target-cpu=x86-64-v2` 无法约束已编译的 ORT object。[issue #256](https://github.com/Dicklesworthstone/coding_agent_session_search/issues/256)。
2. 曾经通过无 semantic Cargo feature / lexical baseline artifact 解决老 CPU 兼容问题。[issue #307](https://github.com/Dicklesworthstone/coding_agent_session_search/issues/307)。
3. `ort 2.0.0-rc.12` 与 all-MiniLM ONNX export 出现 LayerNormalization kernel 不兼容，hash 校验通过但模型仍无法推理；最终项目直接移除 ONNX 栈，换 pure-Rust native backend。[issue #308](https://github.com/Dicklesworthstone/coding_agent_session_search/issues/308)、[current Cargo.toml comment](https://github.com/Dicklesworthstone/coding_agent_session_search/blob/aa92a45311e10a3ac9c40c3730664a7366af2fe8/Cargo.toml)。

这不是证明“ORT 一定不能用”，而是证明：**模型 hash 正确不等于 runtime/operator/CPU baseline 正确；一旦 semantic 和 lexical 生命周期耦合，embedding 故障还可能破坏主检索体验。** agent-session-grep 已有 `LexicalFallback` 和独立 `model_id`，应继续保持更强隔离。

---

## 4. 分发策略取舍

| 策略 | 优点 | 缺点 | 实际样本 | 本项目结论 |
|---|---|---|---|---|
| 模型随主二进制/安装包 | 安装后完全离线、版本一致 | 100–500+ MiB 膨胀；每平台重复；升级与 CLI 耦合；Cargo 包不友好 | 本次 session-search 样本无 | 不采用 |
| 首次运行自动下载 | 最低操作门槛；cache 后离线 | 意外联网、代理/企业网络失败、供应链和遥测感知风险；search/sync 变慢 | fastembed、Recall | 不采用“自动”；只允许显式 install |
| 用户提供目录/文件 | 最严格离线；企业可镜像；可审计 | 配置门槛高、错误路径/版本多 | fastembed user-defined、model2vec local-only、llama GGUF | 必须支持，作为基础机制 |
| 显式 `model install` + local import | 意图清晰；可 pin/hash/license；兼顾易用与离线 | 要实现 cache/atomic publish/错误诊断 | ctx 最接近 | **推荐** |

---

## 5. 明确落地建议

### 5.1 Runtime + model

**推荐实验栈：`candle-core/candle-nn/candle-transformers 0.11` + `tokenizers 0.23` + `intfloat/multilingual-e5-small@614241f622f53c4eeff9890bdc4f31cfecc418b3` safetensors。**

理由：

1. 384 维、mean pooling、L2、`query:`/`passage:` 与现有 trait/存储契约一一对应，不需要先改 application/SQLite 维度。
2. CPU 可用且 Candle 上游三平台 CI；没有 ORT 的 build-time download 或 runtime DLL。
3. runtime/crate/model 都是宽松许可（MIT/Apache）；可开源、可让用户自下载、也可未来在独立 model bundle 中再分发。
4. 已有 Recall 的生产形状先例，工程不确定性低于为 Qwen/GTE/BGE 写新架构 glue。
5. 先用成熟基线建立 benchmark，未来更强模型必须以 recall/latency/size 数据替换，而不是预先押注 0.6B/1 GiB 模型。

### 5.2 Cargo feature

建议：

```toml
[features]
default = []
semantic-candle = ["dep:candle-core", "dep:candle-nn", "dep:candle-transformers", "dep:tokenizers", "dep:safetensors"]
```

feature 名用 **`semantic-candle`**，不要只叫 `semantic`：runtime 是用户可观察的构建/许可/体积选择，未来可并存 `semantic-onnx` 或 external provider；同时避免把当前 bigram-hash 模糊词法路径误算为真实 semantic feature。

默认 feature 为空；官方 `cargo install agent-session-grep` 永远保留纯 lexical/bigram 路径。若未来维护 release archive，可单独发布“semantic-enabled binary”，但仍不把模型塞进 binary。

### 5.3 模型获取机制

建议两个入口，网络和离线严格分开：

1. `agent-session-grep model import --dir <bundle>`：完全离线；校验 pinned manifest、每个文件 SHA-256、size、dimension、license 后原子发布到 app data cache。
2. `agent-session-grep model install multilingual-e5-small`：**这是唯一允许联网的命令**；先打印 repo/revision/size/license/目标目录，用户明确执行即授权；下载到 staging，逐文件 hash，写完整 manifest，最后 atomic rename。

search/sync/index/model load 永不联网。模型缺失时：

- 显式 semantic 请求 → 结构化 unavailable + 获取指引；
- hybrid → 现有 `RetrievalMode::LexicalFallback` + warning；
- 不得在后台悄悄触发 install。

可直接借鉴 ctx 的 pinned revision、expected size/hash、staging + verify + publish 机制，但不借它的 ORT runtime discovery 复杂度。[ctx `cpu_model_cache.rs`](https://github.com/ctxrs/ctx/blob/06bc5ed17ce4d0f6c8255981e009f86802dbe32f/crates/ctx-cli/src/semantic/cpu_model_cache.rs)。

### 5.4 `EmbeddingManifest` 怎么填才诚实

以 Candle fp32 E5 bundle 为例：

| 字段 | 建议值/规则 |
|---|---|
| `model_id` | `intfloat-multilingual-e5-small@614241f6-candle-f32-meanpool-l2-qpass-v1`。必须包含 **repo revision + weight precision/format + pooling + normalization + query/passage policy + pipeline revision**。不要只写 `multilingual-e5-small`。换 tokenizer、量化、pooling、prefix 或截断策略都换 id。 |
| `file_hash` | `sha256:1a55775f53449dac10a2bcbc312469fac40b96d53198c407081a831f81c98477`（当前 pinned `model.safetensors` 的 LFS SHA-256）。字段名是单数且现有注释说“模型文件 SHA-256”，不要把 HF commit、ETag 或 LFS pointer text hash 冒充文件 hash。另在 bundle sidecar 校验 tokenizer/config 各自 hash。若以后允许改变语义，可把字段升级为 canonical bundle manifest digest；在未改契约前不要偷偷改变含义。 |
| `dimension` | `384` |
| `license` | `MIT (intfloat/multilingual-e5-small model weights)`。这里只写模型权重许可；Candle/tokenizers 等 third-party licenses 应进入 release 的 third-party notices，而不是混成一个无法解析的 license 字符串。 |

当前 E5 pinned hashes可参考 ctx 已交叉使用的同一 revision/model ONNX bundle以及 HF API；safetensors/tokenizer hash来自 [HF tree](https://huggingface.co/api/models/intfloat/multilingual-e5-small/tree/main?recursive=true)。

如果选择 ORT qint8，必须换成新 id，例如 `...-onnx-qint8-avx512vnni-...`，并把 `file_hash` 换成该 ONNX 的 SHA-256；**不能**因为“同一个基础模型”沿用 fp32 id。

### 5.5 迁移路径

无需迁移或删除 `bigram-hash-v1` 数据：

1. 新实现使用全新 `model_id`；
2. `message_vec` 查询/写入按 active model id 隔离；
3. 旧 bigram rows 自动变成 inert cache；
4. 用户启用真实模型时按新 id rebuild；
5. 以后可提供显式 GC 删除不用的旧 model id rows，但不是正确性前提。

这正是当前代码注释中“换 vectorizer 必须换 model_id”的设计收益。

---

## 6. 风险与“不做”的理由

### P0：现在不能宣传已有 semantic

`bigram-hash-v1` 只能产生共享 bigram 的 fuzzy lexical similarity；应在 CLI/docs/manifest 中继续叫“bigram hash / fuzzy lexical vectorizer”，不能以“384 维/E5 前缀”暗示真实模型。当前 `embedding.rs` 已写得诚实，应保持。

### P0：不要默认引入真实模型

即使选 Candle，用户仍要承受约 465 MiB bundle、模型初始化 RAM、CPU ingest 时间；项目当前尚无 agent transcript 专用语义 recall 证据。默认打开会让一个“轻量离线 CLI”在体积和同步延迟上变成 ML 工具，而收益未证。

### P0：trait 的单条 `embed` 不适合大规模 backfill

真实 encoder 的吞吐依赖 batch padding/矩阵运算。当前 trait 只有单条 `embed`；实现可以内部排队，但同步 API 很难自然批处理。正式实现前应先决定是否扩一个 batch port，或只把 adapter 内部 batch 用于 rebuild。若直接逐 message 调用，benchmark 可能失败。该问题不影响实验 spike，但影响产品化。

### P1：512 token 截断

E5-small 最长 512 tokens；agent message 常含长日志/代码/tool payload。必须定义确定性的 truncation/chunking。改变 chunking/pooling 会改变 vector space/model id；不能悄悄调整。BGE/GTE/Qwen 的长上下文更强，但它们的体积/维度代价目前更大。

### P1：公开 benchmark 不代表本语料

CJK、代码标识符、路径和精确错误串常常更依赖 lexical；纯 semantic 可能丢失关键 token。现有 hybrid RRF 和 CJK bigram FTS 是正确底座。真实模型只应作为第二路召回，lexical 继续默认；promotion gate 至少要分别测：中文自然语言、英文自然语言、跨语 paraphrase、代码/路径/错误串、长 message、typo。

### P1：ORT 的模型/runtime/CPU 三维兼容风险

cass 的 #256/#308 已证明：hash 正确仍可能因 CPU baseline 或 ONNX operator/runtime 组合失败。若未来加入 `semantic-onnx`，必须：

- pin runtime version + model opset/export；
- 在最低 CPU baseline 上跑 release artifact smoke；
- semantic failure 永不影响 lexical index；
- 不允许启动时静态初始化崩溃；
- release archive 与 `cargo install` 的支持矩阵分开陈述。

### 最终 go/no-go

- **Go（仅实验）**：`semantic-candle` optional feature + explicit model import/install + E5-small pinned bundle + benchmark。
- **No-go（默认/正式宣传）**：在 recall benchmark、三平台 release smoke、模型 cache integrity、batch throughput、512-token policy 完成前，不把真实 semantic 设默认，不把模型随 binary 分发。
- **若资源不足**：完全不引入 ML 依赖；保留 `EmbeddingModel` 扩展点，把 bigram-hash 标为 fuzzy lexical，并继续投资 FTS5+CJK+hybrid evidence。这个选择技术上完全合理，且比半成品“semantic”更诚实。

---

## 7. 关键来源索引

### Runtime / crates

- [`ort` crates.io](https://crates.io/crates/ort/2.0.0-rc.13)
- [`ort` Cargo features](https://ort.pyke.io/setup/cargo-features)
- [`ort` linking](https://ort.pyke.io/setup/linking)
- [`ort` prebuilt binaries/provenance](https://ort.pyke.io/misc/prebuilt-binaries)
- [`ort-sys dist.tsv`](https://github.com/pykeio/ort/blob/main/ort-sys/build/download/dist.tsv)
- [ONNX Runtime license](https://github.com/microsoft/onnxruntime/blob/main/LICENSE)
- [Candle repository](https://github.com/huggingface/candle)
- [Candle CI](https://github.com/huggingface/candle/blob/main/.github/workflows/rust-ci.yml)
- [fastembed repository](https://github.com/Anush008/fastembed-rs)
- [fastembed Cargo.toml](https://github.com/Anush008/fastembed-rs/blob/main/Cargo.toml)
- [`llama-cpp-2` repository](https://github.com/utilityai/llama-cpp-rs)
- [`llama-cpp-sys-2` Cargo.toml](https://github.com/utilityai/llama-cpp-rs/blob/main/llama-cpp-sys-2/Cargo.toml)
- [HF tokenizers Rust API](https://docs.rs/tokenizers/latest/tokenizers/tokenizer/struct.Tokenizer.html)

### Models / papers

- [multilingual-e5-small](https://huggingface.co/intfloat/multilingual-e5-small)
- [Multilingual E5 paper](https://arxiv.org/abs/2402.05672)
- [BGE-M3](https://huggingface.co/BAAI/bge-m3) / [paper](https://arxiv.org/abs/2402.03216)
- [gte-multilingual-base](https://huggingface.co/Alibaba-NLP/gte-multilingual-base) / [paper](https://arxiv.org/abs/2407.19669)
- [Qwen3-Embedding-0.6B](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B) / [paper](https://arxiv.org/abs/2506.05176)
- [jina-embeddings-v3](https://huggingface.co/jinaai/jina-embeddings-v3)
- [potion-multilingual-128M](https://huggingface.co/minishlab/potion-multilingual-128M)

### Competitors / negative evidence

- [ctx pinned source](https://github.com/ctxrs/ctx/tree/06bc5ed17ce4d0f6c8255981e009f86802dbe32f)
- [Recall pinned source](https://github.com/samzong/Recall/tree/22625bf8979a185d9913c1bfb20290ac466d492b)
- [cass pinned source](https://github.com/Dicklesworthstone/coding_agent_session_search/tree/aa92a45311e10a3ac9c40c3730664a7366af2fe8)
- [cass #256: ORT prebuilt CPU crash](https://github.com/Dicklesworthstone/coding_agent_session_search/issues/256)
- [cass #307: lexical baseline build](https://github.com/Dicklesworthstone/coding_agent_session_search/issues/307)
- [cass #308: ONNX LayerNorm failure and removal](https://github.com/Dicklesworthstone/coding_agent_session_search/issues/308)
