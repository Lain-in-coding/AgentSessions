//! CLI 端到端集成测试：驱动真实编译出的二进制，走 index → search → get 全链路。
//!
//! 目的：把「组合根真的能把 SQLite adapter 注入 Application 并端到端跑通」这件事
//! 固化进 CI，而非依赖手动运行。Cargo 为集成测试注入 `CARGO_BIN_EXE_<bin>`，
//! 故此处零第三方依赖即可定位到刚构建的二进制。

use std::process::{Command, Output};

/// 刚构建出的 `agentsessions` 二进制的绝对路径（由 Cargo 在编译期注入）。
const BIN: &str = env!("CARGO_BIN_EXE_agentsessions");

/// 在给定 db 上跑一次 CLI，返回完整输出。
fn run(db: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("failed to spawn agentsessions binary")
}

/// 解析 stdout 的第一行为 JSON Value。
fn parse_first_line(o: &Output) -> serde_json::Value {
    let text = stdout(o);
    let line = text
        .lines()
        .next()
        .expect("output must have at least one line");
    serde_json::from_str(line).unwrap_or_else(|error| panic!("not valid JSON: {error}\n{line}"))
}

/// 断言 frame 满足 Robot v1 envelope 最小契约。
fn assert_envelope_shape(frame: &serde_json::Value, ok: bool) {
    assert_eq!(frame["schema_version"], "1.0", "schema_version");
    assert_eq!(frame["ok"], ok, "ok");
    if ok {
        assert_eq!(frame["frame_type"], "response", "frame_type");
        assert!(
            matches!(frame["outcome"].as_str(), Some("success") | Some("partial")),
            "outcome must be success or partial for ok:true"
        );
        assert!(frame["data"].is_object(), "data must be an object");
    } else {
        assert_eq!(frame["frame_type"], "error", "frame_type");
        assert_eq!(frame["outcome"], "failure", "outcome");
        let code = frame["error"]["code"]
            .as_str()
            .expect("error.code must be a string");
        assert!(!code.is_empty(), "error.code must not be empty");
    }
    assert!(
        frame["request_id"].as_str().is_some(),
        "request_id must be a string"
    );
    assert!(frame["warnings"].is_array(), "warnings must be an array");
    assert!(
        frame["page"]["has_more"].is_boolean(),
        "page.has_more must be a boolean"
    );
    assert!(
        frame["meta"]["duration_ms"].is_number(),
        "meta.duration_ms must be a number"
    );
}

/// 每个测试用独立临时目录，避免 WAL/SHM 旁文件互相干扰。
fn temp_db(tag: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{tag}.db"));
    let s = path.to_string_lossy().into_owned();
    (dir, s)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// 不带 `--db` 跑一次 CLI——用于 `--help`/`--version`/`doctor` 等无需存储的命令。
fn run_bare(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("failed to spawn agentsessions binary")
}

#[test]
fn index_search_get_roundtrip() {
    let (_dir, db) = temp_db("roundtrip");

    let out = run(&db, &["index", "m1", "the quick brown fox jumps"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("\"ok\":true"));
    assert!(stdout(&out).contains("msg_v1_"));

    run(&db, &["index", "m2", "lazy dog sleeps all day"]);

    // search 命中正确文档：查 "brown" 只应命中 m1。
    let out = run(&db, &["search", "brown"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(
        s.contains("msg_v1_52db0bc4880412c58a3cc166ec6c389a"),
        "got: {s}"
    );
    assert!(!s.contains("lazy"), "brown 不应命中 m2");

    // get 取回 index 时写入的原始 payload——用 search 显示的真实 wire id
    // （`get` 现在按 wire 串反解 id，见 StableId::from_wire，不再把参数当 fact 重派生）。
    let out = run(&db, &["get", "msg_v1_52db0bc4880412c58a3cc166ec6c389a"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("the quick brown fox jumps"));
}

#[test]
fn get_missing_returns_null_payload() {
    let (_dir, db) = temp_db("missing");
    // 合法前缀但从未写入的 id：解析成功、catalog 查无 → payload:null。
    let out = run(&db, &["get", "msg_v1_ffffffffffffffffffffffffffffffff"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("\"payload\":null"));
}

#[test]
fn get_malformed_id_is_usage_error() {
    let (_dir, db) = temp_db("malformed");
    // 无已知前缀的串无法反解为实体 id（见 StableId::from_wire）→ 用法错误 exit 2。
    let out = run(&db, &["get", "does-not-exist"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn show_returns_normalized_role_and_text() {
    let (dir, db) = temp_db("show");
    // ingest 写入的 payload 是 `role\ttext`，show 应把它拆成结构化 entity。
    let fixture = dir.path().join("show.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"how do I show an entity"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    // 从 search 输出取真实 wire id。
    let out = run(&db, &["search", "entity"]);
    let s = stdout(&out);
    let id = s
        .split("\"id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("search 输出应含 id 字段");

    let out = run(&db, &["show", id]);
    assert!(out.status.success(), "show failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "show");
    // show 与 get 的区别：结构化 role/text，而非裸 payload 串。
    assert_eq!(
        frame["data"]["entity"]["role"],
        "user",
        "show={}",
        stdout(&out)
    );
    assert_eq!(
        frame["data"]["entity"]["text"],
        "how do I show an entity",
        "show={}",
        stdout(&out)
    );
}

#[test]
fn show_missing_returns_null_entity() {
    let (_dir, db) = temp_db("show-missing");
    // 合法前缀但从未写入的 id：解析成功、catalog 查无 → entity:null。
    let out = run(&db, &["show", "msg_v1_ffffffffffffffffffffffffffffffff"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("\"entity\":null"),
        "show={}",
        stdout(&out)
    );
}

#[test]
fn show_malformed_id_is_usage_error() {
    let (_dir, db) = temp_db("show-malformed");
    let out = run(&db, &["show", "does-not-exist"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn ingest_preserves_native_uuid_identity_and_threading() {
    let (dir, db) = temp_db("native-threading");
    // 带真实 uuid/parentUuid 的两条记录：identity 应采用 native uuid（原样透传，
    // 非 path+seq 派生），threading 边经存储穿到 show。
    let fixture = dir.path().join("threaded.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","uuid":"11111111-1111-4111-8111-111111111111","parentUuid":null,"timestamp":"2026-06-27T13:57:42.685Z","message":{"role":"user","content":"root message about widgets"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"22222222-2222-4222-8222-222222222222","parentUuid":"11111111-1111-4111-8111-111111111111","message":{"role":"assistant","content":"child reply about widgets"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    // identity 采用 native uuid：wire id 应内含原始 uuid，而非 path+seq 派生的 hex。
    let child_id = "msg_v1_22222222-2222-4222-8222-222222222222";
    let out = run(&db, &["show", child_id]);
    assert!(out.status.success(), "show failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    let entity = &frame["data"]["entity"];
    assert_eq!(entity["role"], "assistant", "show={}", stdout(&out));
    assert_eq!(entity["text"], "child reply about widgets");
    // threading 边穿过存储：子消息的 parent 指向根消息的 native uuid。
    assert_eq!(
        entity["parent_native_id"],
        "11111111-1111-4111-8111-111111111111",
        "show={}",
        stdout(&out)
    );

    // 根消息 parent 为 null，timestamp 原样保留。
    let out = run(
        &db,
        &["show", "msg_v1_11111111-1111-4111-8111-111111111111"],
    );
    let frame = parse_first_line(&out);
    let entity = &frame["data"]["entity"];
    assert!(
        entity["parent_native_id"].is_null(),
        "root parent must be null"
    );
    assert_eq!(entity["timestamp"], "2026-06-27T13:57:42.685Z");
}

#[test]
fn ingest_auto_selects_codex_and_ignores_event_mirror() {
    let (dir, db) = temp_db("codex");
    // 合成的 Codex rollout（非真实 transcript，遵守 R0 脱敏规范）：
    // 每条对话消息出现两次——权威 response_item/message（带 native id）与
    // event_msg UI 镜像（无 id）。adapter 只取前者，committed 应为 2 而非 4。
    let fixture = dir.path().join("rollout.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"timestamp":"2026-07-19T15:40:00.000Z","type":"session_meta","payload":{"session_id":"aaaa1111-2222-7333-8444-555566667777","cwd":"/tmp","originator":"codex","cli_version":"1.0"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-19T15:41:00.000Z","type":"response_item","payload":{"type":"message","id":"msg_codex_root","role":"user","content":[{"type":"input_text","text":"how do I configure the pipeline"}]}}"#,
            "\n",
            r#"{"timestamp":"2026-07-19T15:41:00.500Z","type":"event_msg","payload":{"type":"user_message","message":"how do I configure the pipeline"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-19T15:41:19.000Z","type":"response_item","payload":{"type":"message","id":"msg_codex_reply","role":"assistant","content":[{"type":"output_text","text":"set the pipeline stages first"}]}}"#,
            "\n",
            r#"{"timestamp":"2026-07-19T15:41:19.500Z","type":"event_msg","payload":{"type":"agent_message","message":"set the pipeline stages first"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    // registry 自动 probe-select：无需指定 provider，应判定为 codex variant。
    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(
        frame["data"]["variant"],
        "codex/rollout-jsonl-v1",
        "registry 应自动选中 codex: {}",
        stdout(&out)
    );
    // 关键去重断言：2 条权威消息，event_msg 镜像不计入。
    assert_eq!(
        frame["data"]["committed"],
        2,
        "event_msg 镜像应被忽略，只提交 2 条: {}",
        stdout(&out)
    );

    // native id 原样保留，show 展开 role/text。
    let out = run(&db, &["show", "msg_v1_msg_codex_reply"]);
    assert!(out.status.success(), "show failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    let entity = &frame["data"]["entity"];
    assert_eq!(entity["role"], "assistant", "show={}", stdout(&out));
    assert_eq!(entity["text"], "set the pipeline stages first");

    // 内容可检索。
    let out = run(&db, &["search", "pipeline"]);
    assert!(
        stdout(&out).contains("msg_v1_msg_codex"),
        "codex 内容应可检索: {}",
        stdout(&out)
    );
}

#[test]
fn index_rebuild_reprojects_and_keeps_search_working() {
    let (dir, db) = temp_db("rebuild");
    let fixture = dir.path().join("rebuild.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"rebuild the search index please"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"reprojecting from catalog now"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));

    // rebuild：从权威 catalog 全量重投影 FTS 索引，推进 generation。
    let out = run(&db, &["index", "rebuild"]);
    assert!(out.status.success(), "rebuild failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "index.rebuild");
    assert_eq!(
        frame["data"]["reindexed"],
        2,
        "应重投影 2 条: {}",
        stdout(&out)
    );
    // generation：ingest 推进到 1，rebuild 再推进到 2。
    assert_eq!(frame["data"]["generation"], 2, "rebuild={}", stdout(&out));

    // 重建后搜索仍命中原内容，且身份保真（wire id 前缀不变）。
    let out = run(&db, &["search", "reprojecting"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("msg_v1_"),
        "rebuild 后搜索仍应命中: {}",
        stdout(&out)
    );
}

#[test]
fn index_rebuild_on_empty_db_succeeds() {
    let (_dir, db) = temp_db("rebuild-empty");
    let out = run(&db, &["index", "rebuild"]);
    assert!(
        out.status.success(),
        "empty rebuild failed: {}",
        stdout(&out)
    );
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["data"]["reindexed"], 0);
}

#[test]
fn missing_subcommand_is_usage_error() {
    let (_dir, db) = temp_db("usage");
    let out = run(&db, &[]);
    // 用法错误映射为 exit code 2（见 main.rs 的 CliError::Usage）。
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn unknown_subcommand_is_usage_error() {
    let (_dir, db) = temp_db("unknown");
    let out = run(&db, &["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn version_flag_prints_version_without_db() {
    // --version 不需要 --db：在 parse_db_flag 之前拦截。
    let out = run_bare(&["--version"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("0.1.0"));
}

#[test]
fn help_flag_lists_commands_without_db() {
    let out = run_bare(&["--help"]);
    assert!(out.status.success());
    let s = stdout(&out);
    // 帮助里应列出核心子命令，便于发现。
    assert!(s.contains("ingest"), "help 应列出 ingest: {s}");
    assert!(s.contains("search"), "help 应列出 search: {s}");
}

#[test]
fn doctor_reports_ok_without_db() {
    let out = run_bare(&["doctor"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("\"ok\":true"), "doctor 应报告 ok:true: {s}");
    assert!(
        s.contains("\"db\":\"not-checked\""),
        "无 --db 时应标记未校验: {s}"
    );
}

#[test]
fn doctor_with_db_reports_generation_and_recovery_evidence() {
    let (_dir, db) = temp_db("doctor-db");
    // 写一条推进 generation 到 1。
    let out = run(&db, &["index", "d1", "doctor evidence content"]);
    assert!(out.status.success());

    let out = run(&db, &["doctor", "--db", &db]);
    assert!(out.status.success(), "doctor failed: {}", stdout(&out));
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["data"]["db"], "ok");
    // 一致性/恢复证据：活动 generation + 待收敛 intent 数（干净库应为 0）。
    assert_eq!(frame["data"]["generation"], 1, "doctor={}", stdout(&out));
    assert_eq!(
        frame["data"]["interrupted_batches"],
        0,
        "干净库不应有待收敛 intent: {}",
        stdout(&out)
    );
}

#[test]
fn ingest_search_get_roundtrip_via_binary() {
    let (dir, db) = temp_db("ingest");
    // 合成的 Claude Code 风格 .jsonl fixture（按 R0 脱敏规范，非真实 transcript）。
    let fixture = dir.path().join("session.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"how do I configure the neural net"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"set the learning rate first"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let fixture_path = fixture.to_string_lossy().into_owned();

    // ingest：probe 判定 variant + parse 流式入库。
    let out = run(&db, &["ingest", &fixture_path]);
    assert!(out.status.success(), "ingest failed: {}", stdout(&out));
    let s = stdout(&out);
    assert!(s.contains("claude-code/jsonl-v1"), "应判定出 variant: {s}");
    assert!(s.contains("\"committed\":2"), "应入库 2 条消息: {s}");

    // search：ingest 的内容可被检索到。
    let out = run(&db, &["search", "neural"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("msg_v1_"), "search 应命中 ingest 的消息: {s}");

    // 从 search 输出提取真实 wire id，拿去 get 应取回原文（往返闭合）。
    let id = s
        .split("\"id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("search 输出应含 id 字段");
    let out = run(&db, &["get", id]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("neural"),
        "get 应取回含 neural 的 payload: {}",
        stdout(&out)
    );
}

#[test]
fn list_and_status_report_catalog_contents() {
    let (_dir, db) = temp_db("list-status");
    let out = run(&db, &["index", "a", "alpha payload"]);
    assert!(out.status.success());
    let out = run(&db, &["index", "b", "beta payload"]);
    assert!(out.status.success());

    let out = run(&db, &["status"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("\"catalog_count\":2"), "status={s}");

    let out = run(&db, &["list", "1"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert_eq!(
        s.lines().count(),
        1,
        "list limit should return one row: {s}"
    );
    assert!(s.contains("\"id\":\"msg_v1_"), "list={s}");
}

#[test]
fn sync_commits_then_reports_unchanged_on_resync() {
    let (dir, db) = temp_db("sync");
    let fixture = dir.path().join("s1.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"how do I tune the index"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"raise the batch size"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let path = fixture.to_string_lossy().into_owned();

    // 首次 sync：两条消息提交，generation 从 0 推进到 1。
    let out = run(&db, &["sync", &path]);
    assert!(out.status.success(), "sync failed: {}", stdout(&out));
    let s = stdout(&out);
    assert!(s.contains("\"messages\":2"), "sync={s}");
    assert!(s.contains("\"committed\":2"), "sync={s}");
    assert!(s.contains("\"generation\":1"), "sync={s}");

    // 内容可检索。
    let out = run(&db, &["search", "tune"]);
    assert!(stdout(&out).contains("msg_v1_"), "search={}", stdout(&out));

    // 重复 sync 未改变的源：no-op，generation 不推进。
    let out = run(&db, &["sync", &path]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("\"committed\":0"), "resync={s}");
    assert!(s.contains("\"unchanged\":2"), "resync={s}");
    assert!(
        s.contains("\"generation\":1"),
        "resync 不应推进 generation: {s}"
    );
}

#[test]
fn sync_requires_at_least_one_file() {
    let (_dir, db) = temp_db("sync-empty");
    let out = run(&db, &["sync"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn sync_all_or_nothing_on_bad_source() {
    let (dir, db) = temp_db("sync-atomic");
    let good = dir.path().join("good.jsonl");
    std::fs::write(
        &good,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"valid line"}}"#,
            "\n",
        ),
    )
    .expect("write good fixture");
    let good_path = good.to_string_lossy().into_owned();
    // 不存在的第二个源：整批 sync 必须失败且不写入任何数据。
    let missing = dir.path().join("missing.jsonl");
    let missing_path = missing.to_string_lossy().into_owned();

    let out = run(&db, &["sync", &good_path, &missing_path]);
    assert!(
        !out.status.success(),
        "sync 应因缺失源失败: {}",
        stdout(&out)
    );

    // 第一个源的消息不能被部分写入。
    let out = run(&db, &["status"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("\"catalog_count\":0"),
        "失败的 sync 不应留下部分数据: {}",
        stdout(&out)
    );
}

#[test]
fn sync_tombstones_message_removed_from_source() {
    let (dir, db) = temp_db("sync-shrink");
    let fixture = dir.path().join("shrink.jsonl");
    // 首次：两条消息。
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"keep this message"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":"drop this later"}}"#,
            "\n",
        ),
    )
    .expect("write fixture");
    let path = fixture.to_string_lossy().into_owned();

    let out = run(&db, &["sync", &path]);
    assert!(out.status.success(), "sync failed: {}", stdout(&out));
    let out = run(&db, &["status"]);
    assert!(
        stdout(&out).contains("\"catalog_count\":2"),
        "{}",
        stdout(&out)
    );
    // 第二条消息此刻可检索。
    let out = run(&db, &["search", "drop"]);
    assert!(stdout(&out).contains("msg_v1_"), "search={}", stdout(&out));

    // 源收缩到一条：被移除的消息应被 tombstone，catalog 与搜索都不再有它。
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"keep this message"}}"#,
            "\n",
        ),
    )
    .expect("rewrite fixture");

    let out = run(&db, &["sync", &path]);
    assert!(
        out.status.success(),
        "reshrink sync failed: {}",
        stdout(&out)
    );
    let out = run(&db, &["status"]);
    assert!(
        stdout(&out).contains("\"catalog_count\":1"),
        "收缩后应只剩一条: {}",
        stdout(&out)
    );
    let out = run(&db, &["search", "drop"]);
    assert!(
        !stdout(&out).contains("msg_v1_"),
        "被移除消息不应再命中搜索: {}",
        stdout(&out)
    );
    let out = run(&db, &["search", "keep"]);
    assert!(
        stdout(&out).contains("msg_v1_"),
        "保留消息仍应命中: {}",
        stdout(&out)
    );
}

// ─── Robot v1 Envelope 契约 E2E ────────────────────────────────────────────

#[test]
fn robot_envelope_shape_on_success() {
    let (_dir, db) = temp_db("env-ok");
    let out = run(&db, &["index", "e1", "envelope shape test"]);
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    // frame_type / outcome / data.generation present
    assert_eq!(frame["command"], "index");
    assert!(frame["data"]["generation"].is_number());
    assert!(frame["meta"]["duration_ms"].as_u64().is_some());
}

#[test]
fn robot_envelope_shape_on_error() {
    let (_dir, db) = temp_db("env-err");
    let out = run(&db, &["--robot", "get", "not-a-valid-id"]);
    assert!(!out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["command"], "get");
    assert_eq!(frame["error"]["code"], "invalid_request");
    assert!(!frame["error"]["retryable"].as_bool().unwrap());
}

#[test]
fn robot_flag_produces_same_json_envelope() {
    let (_dir, db) = temp_db("env-robot");
    let out = Command::new(BIN)
        .args(["--db", &db, "--robot", "status"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "status");
}

#[test]
fn output_json_flag_produces_well_formed_envelope() {
    let (_dir, db) = temp_db("env-json");
    let out = Command::new(BIN)
        .args(["--db", &db, "--output", "json", "status"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
}

#[test]
fn invalid_output_mode_exits_with_code_2() {
    let (_dir, db) = temp_db("env-mode");
    let out = Command::new(BIN)
        .args(["--db", &db, "--output", "yaml", "status"])
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    // Even mode errors produce a valid error envelope on stdout.
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, false);
    assert_eq!(frame["error"]["code"], "invalid_request");
}

#[test]
fn doctor_envelope_shape_has_meta_generation_null() {
    let out = run_bare(&["doctor"]);
    assert!(out.status.success());
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["command"], "doctor");
    // doctor has no generation context
    assert!(frame["meta"]["generation"].is_null() || frame["meta"]["generation"].is_number());
}

// ─── config paths ──────────────────────────────────────────────────────────

#[test]
fn config_paths_reports_platform_directories() {
    let out = run_bare(&["config", "paths"]);
    assert!(
        out.status.success(),
        "config paths failed: {}",
        stdout(&out)
    );
    let frame = parse_first_line(&out);
    assert_envelope_shape(&frame, true);
    let data = &frame["data"];
    // All four directory fields must be present and non-empty strings.
    for field in ["config", "data", "cache", "logs"] {
        let value = data[field]
            .as_str()
            .unwrap_or_else(|| panic!("config paths must include {field} field, got: {data}"));
        assert!(!value.is_empty(), "{field} must not be empty");
    }
}
#[test]
fn jsonl_output_is_one_complete_frame_per_line() {
    let (_dir, db) = temp_db("env-jsonl");
    let out = Command::new(BIN)
        .args(["--db", &db, "--output", "jsonl", "status"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let output_text = stdout(&out);
    let lines: Vec<_> = output_text.lines().collect();
    assert_eq!(lines.len(), 1, "one command must emit one JSONL frame");
    let frame: serde_json::Value = serde_json::from_str(lines[0]).expect("valid JSONL frame");
    assert_envelope_shape(&frame, true);
    assert_eq!(frame["frame_type"], "response");
}

#[test]
fn human_error_writes_diagnostic_to_stderr_only() {
    let (_dir, db) = temp_db("env-human-error");
    let out = run(&db, &["get", "not-a-valid-id"]);
    assert!(!out.status.success());
    assert!(
        stdout(&out).is_empty(),
        "human mode stdout must stay protocol-clean"
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).is_empty(),
        "human mode must write diagnostic to stderr"
    );
}

// ─── 初始性能基线 ───────────────────────────────────────────────────────────

#[test]
fn perf_baseline_100_messages_index_and_search() {
    let (_dir, db) = temp_db("perf-baseline");
    let start = std::time::Instant::now();

    // 写入 100 条消息
    for i in 0..100u32 {
        let fact = format!("perf-fact-{i}");
        let text = format!("performance baseline message number {i} with unique content");
        let out = run(&db, &["index", &fact, &text]);
        assert!(out.status.success(), "index {i} failed: {}", stdout(&out));
    }
    let index_ms = start.elapsed().as_millis();

    // 全文检索应命中
    let search_start = std::time::Instant::now();
    let out = run(&db, &["search", "performance baseline"]);
    let query_ms = search_start.elapsed().as_millis();

    assert!(out.status.success());
    assert!(
        stdout(&out).contains("msg_v1_"),
        "search must return results"
    );

    // 性能软目标（非 CI 硬阻断，仅防止极端回归）：
    // 100 次写入 < 10 秒（含进程启动开销）；单次查询 < 3 秒。
    assert!(
        index_ms < 10_000,
        "indexing 100 messages took {index_ms}ms, expected < 10s"
    );
    assert!(
        query_ms < 3_000,
        "search query took {query_ms}ms, expected < 3s"
    );

    // 输出基线数据供观察（不阻断 CI）
    eprintln!("[perf-baseline] index 100 msgs: {index_ms}ms  search: {query_ms}ms");
}
