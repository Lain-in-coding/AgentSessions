//! Lexical search rank signals（competitor-borrowings #1）：把 bm25 相关性、
//! 时效衰减与 sidechain 惩罚合成为单一评分函数。
//!
//! 借鉴来源：
//! - AgentRecall `smartScore`：30 天指数半衰期衰减（`decay = 0.5^(ageDays/30)`，
//!   经 `(floor + (1-floor)*decay)` 插值保住下限）；
//! - sessiongrep：FTS 召回后应用层重排补充新鲜度/业务信号（本模块同构）；
//! - agentsview：subordinate（sidechain/subagent）命中 rank+5 的 RRF 惩罚精神。
//!
//! 教训（deep-read-claude-historian-mcp.md §11.7）：多层经验参数无测试固化必
//! 回归——本模块是唯一评分入口（单一函数 [`final_score`]），全部参数集中在
//! 下方常量区，且每个参数都有单测锚定（半衰期/下限/钳制/惩罚值/确定性）。
//!
//! 边界（README 明示）：
//! - 只作用于纯 lexical 检索（含 semantic/hybrid 未就绪时的 lexical_fallback）
//!   的命中排序；semantic 命中与 hybrid RRF 融合排序不受影响；
//! - 无 timestamp（缺失/null/非法）的命中按 0 龄处理（衰减恒 1.0）——
//!   无信号不加罚；`is_sidechain` 缺失或 null（歧义）按 false，不惩罚未知；
//! - recency 随注入时钟推移（生产特性），测试必须注入固定时钟保持确定性。

use agent_session_grep_ports::SearchHit;

/// 时效半衰期（天）：30 天前的时间戳衰减到 0.5（AgentRecall smartScore 同款）。
pub const RECENCY_HALF_LIFE_DAYS: f64 = 30.0;

/// 时效衰减下限：无论多老都保留该比例的 bm25 分，防止老消息被完全压没。
pub const RECENCY_DECAY_FLOOR: f64 = 0.3;

/// sidechain 命中固定扣分。与 bm25 量级相称：FTS5 bm25（SQLite ≥3.53 的 idf
/// 口径）实测单/多词命中约 0.5–15；扣 1.0 约等于"一个单词匹配"的分值——
/// 同等相关性下 sidechain 命中让位，强相关 sidechain 仍可胜出。
pub const SIDECHAIN_SCORE_PENALTY: f32 = 1.0;

/// 每天毫秒数（仅内部换算用，非调参常量）。
const MS_PER_DAY: f64 = 86_400_000.0;

/// 指数半衰期衰减：`0.5^(age / half_life)`。
///
/// - `age` 钳制 ≥ 0：未来时间戳按 0 龄处理（衰减 1.0，无惩罚）；
/// - 结果钳制下限 [`RECENCY_DECAY_FLOOR`]：老消息不被完全压没。
pub fn recency_decay(age_ms: i64) -> f64 {
    let age_days = age_ms.max(0) as f64 / MS_PER_DAY;
    let decay = 0.5_f64.powf(age_days / RECENCY_HALF_LIFE_DAYS);
    decay.max(RECENCY_DECAY_FLOOR)
}

/// 单一评分函数：`final = max(0, bm25 × decay(age)) − sidechain_penalty`。
///
/// 展示的 `score` 字段即本函数输出；bm25 原值不保留在命中结构里
/// （why_matched/guidance 不消费 score，语义不变）。
pub fn final_score(bm25: f32, age_ms: i64, is_sidechain: bool) -> f32 {
    let decayed = f64::from(bm25) * recency_decay(age_ms);
    let penalized = if is_sidechain {
        decayed - f64::from(SIDECHAIN_SCORE_PENALTY)
    } else {
        decayed
    };
    // 用 `> 0.0` 而非 `max(0.0)`：钳制的同时把 `-0.0` 与 NaN 归一为 `+0.0`
    // （`max(-0.0, 0.0)` 的符号在不同平台上未定义），保证
    // (final desc, id asc) 排序跨平台逐位稳定。
    if penalized > 0.0 {
        penalized as f32
    } else {
        0.0
    }
}

/// 从 canonical payload 解析 message 时间戳（RFC3339 串，与
/// [`crate::parse_search_instant`] 同一解析器）为 Unix 毫秒；
/// 缺失/非字符串/非法 → `None`（调用方按 0 龄处理）。
fn payload_timestamp_ms(payload: Option<&[u8]>) -> Option<i64> {
    let value: serde_json::Value = serde_json::from_slice(payload?).ok()?;
    let instant = crate::parse_search_instant(value.get("timestamp")?.as_str()?)?;
    Some(
        instant
            .unix_seconds
            .saturating_mul(1_000)
            .saturating_add(i64::from(instant.nanosecond / 1_000_000)),
    )
}

/// 从 canonical payload 解析 `is_sidechain`；缺失/null（歧义）按 false——
/// 不惩罚未知，与 store 侧 canonical payload 的 `is_sidechain: null` 语义一致。
fn payload_is_sidechain(payload: Option<&[u8]>) -> bool {
    let value: serde_json::Value =
        match payload.and_then(|bytes| serde_json::from_slice(bytes).ok()) {
            Some(value) => value,
            None => return false,
        };
    value
        .get("is_sidechain")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// 对纯 lexical 扫描窗应用排序信号：逐 hit 从 canonical payload 派生
/// 时效与 sidechain 事实，重算 `hit.score` 为最终分，并重排为
/// `(final desc, id asc)` 全序（与 store 层 bm25+id 的钉住排序同一 tiebreak
/// 约定，保证同 query 同 clock 下分页稳定）。
///
/// 输入是已与命中同序配对的 `(hit, payload)` 对——排序发生在配对之后，
/// 结构上排除"重排后 payload 错位"的可能；`now_ms` 来自注入的应用时钟。
pub(crate) fn apply_lexical_signals(
    pairs: Vec<(SearchHit, Option<Vec<u8>>)>,
    now_ms: i64,
) -> Vec<SearchHit> {
    let mut ranked: Vec<SearchHit> = pairs
        .into_iter()
        .map(|(mut hit, payload)| {
            let age_ms = payload_timestamp_ms(payload.as_deref())
                .map_or(0, |timestamp_ms| now_ms.saturating_sub(timestamp_ms));
            hit.score = final_score(hit.score, age_ms, payload_is_sidechain(payload.as_deref()));
            hit
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_session_grep_domain::{IdKind, StableId};

    const DAY_MS: i64 = 86_400_000;

    fn hit(tag: &str, score: f32) -> SearchHit {
        SearchHit {
            id: StableId::native(IdKind::Message, tag),
            score,
            session_id: None,
            text: None,
            why_matched: Vec::new(),
            suggested_next_commands: Vec::new(),
            occurrences: 1,
            resume_available: false,
        }
    }

    // ---- 半衰期：30 天衰减到 0.5（AgentRecall 同款），地板约 52 天处接管 ----

    #[test]
    fn recency_decay_is_one_at_zero_age_and_halves_every_thirty_days() {
        assert_eq!(recency_decay(0), 1.0);
        assert!((recency_decay(30 * DAY_MS) - 0.5).abs() < 1e-9);
        // 60/90 天无地板时本应为 0.25/0.125——RECENCY_DECAY_FLOOR=0.3 在
        // ~52 天处接管，半衰期只作用到地板为止（老消息保留 0.3 下限）。
        assert!((recency_decay(60 * DAY_MS) - 0.3).abs() < 1e-9);
        assert!((recency_decay(90 * DAY_MS) - 0.3).abs() < 1e-9);
    }

    // ---- clamp：未来时间戳按 0 龄处理（不因时钟偏差反向加分）----

    #[test]
    fn recency_decay_clamps_future_timestamps_to_full_score() {
        assert_eq!(recency_decay(-1), 1.0);
        assert_eq!(recency_decay(-365 * DAY_MS), 1.0);
        assert_eq!(recency_decay(i64::MIN), 1.0);
    }

    // ---- 衰减下限：老消息保留 RECENCY_DECAY_FLOOR 比例 ----

    #[test]
    fn recency_decay_never_drops_below_floor() {
        assert!((recency_decay(200 * DAY_MS) - RECENCY_DECAY_FLOOR).abs() < 1e-9);
        assert_eq!(recency_decay(i64::MAX / 2), RECENCY_DECAY_FLOOR);
        // 衰减单调不升：更老的消息绝不比更年轻的衰减更浅。
        assert!(recency_decay(31 * DAY_MS) < recency_decay(30 * DAY_MS));
        assert!(recency_decay(29 * DAY_MS) > recency_decay(30 * DAY_MS));
    }

    // ---- sidechain 固定分惩罚 ----

    #[test]
    fn final_score_applies_decay_then_fixed_sidechain_penalty() {
        // 30 天龄：2.0 × 0.5 = 1.0。
        assert!((final_score(2.0, 30 * DAY_MS, false) - 1.0).abs() < 1e-6);
        // sidechain：1.0 − 1.0 = 0（钳制到 0，绝不为负）。
        assert_eq!(final_score(2.0, 30 * DAY_MS, true), 0.0);
        // 弱命中（0.5）sidechain 惩罚后钳到 0，不产生负分。
        assert_eq!(final_score(0.5, 0, true), 0.0);
        // 强命中（4.0）sidechain 仍保留 3.0——惩罚是固定分值而非比例。
        assert!((final_score(4.0, 0, true) - 3.0).abs() < 1e-6);
        // 同等相关性下 sidechain 恒不高于主线。
        for (bm25, age) in [
            (0.1, 0),
            (1.0, 0),
            (10.0, 90 * DAY_MS),
            (100.0, 1000 * DAY_MS),
        ] {
            assert!(final_score(bm25, age, true) <= final_score(bm25, age, false));
        }
    }

    // ---- 单一函数确定性：同输入同输出 ----

    #[test]
    fn final_score_is_deterministic() {
        for _ in 0..3 {
            assert_eq!(
                final_score(1.5, 45 * DAY_MS, false),
                final_score(1.5, 45 * DAY_MS, false)
            );
            assert_eq!(
                final_score(1.5, 45 * DAY_MS, true),
                final_score(1.5, 45 * DAY_MS, true)
            );
        }
    }

    // ---- apply：payload 事实派生 + 重排 ----

    fn payload(timestamp: Option<&str>, is_sidechain: Option<bool>) -> Vec<u8> {
        let mut map = serde_json::Map::new();
        if let Some(ts) = timestamp {
            map.insert("timestamp".into(), serde_json::json!(ts));
        }
        if let Some(side) = is_sidechain {
            map.insert("is_sidechain".into(), serde_json::json!(side));
        }
        serde_json::Value::Object(map).to_string().into_bytes()
    }

    fn apply(pairs: Vec<(SearchHit, Option<Vec<u8>>)>) -> Vec<SearchHit> {
        // 固定时钟 2026-08-25T00:00:00Z。
        let now_ms = 1_787_616_000_000;
        apply_lexical_signals(pairs, now_ms)
    }

    #[test]
    fn apply_lexical_signals_decays_older_hit_below_newer() {
        let pairs = vec![
            (
                hit("old", 1.0),
                Some(payload(Some("2026-01-01T00:00:00Z"), None)),
            ),
            (
                hit("new", 1.0),
                Some(payload(Some("2026-08-24T00:00:00Z"), None)),
            ),
        ];
        let hits = apply(pairs);
        assert_eq!(hits[0].id.as_str(), "msg_v1_new", "newer must rank first");
        assert_eq!(hits[1].id.as_str(), "msg_v1_old");
        // 1 天龄衰减接近 1.0，高于老命中的地板值。
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn apply_lexical_signals_demotes_sidechain_under_equal_relevance() {
        let pairs = vec![
            (hit("main", 2.0), Some(payload(None, Some(false)))),
            (hit("side", 2.0), Some(payload(None, Some(true)))),
        ];
        let hits = apply(pairs);
        assert_eq!(
            hits[0].id.as_str(),
            "msg_v1_main",
            "sidechain must rank later"
        );
        assert_eq!(hits[1].id.as_str(), "msg_v1_side");
        assert_eq!(hits[0].score, 2.0);
        assert_eq!(hits[1].score, 1.0);
    }

    #[test]
    fn apply_lexical_signals_treats_missing_facts_as_no_signal() {
        // 无 timestamp / is_sidechain null / 非法 timestamp：衰减恒 1.0、
        // 无 sidechain 惩罚——无信号不加罚，顺序保持 bm25 序。
        let pairs = vec![
            (hit("b", 2.0), Some(payload(None, Some(false)))),
            (
                hit("a", 1.0),
                Some(
                    serde_json::json!({ "timestamp": null, "is_sidechain": null })
                        .to_string()
                        .into_bytes(),
                ),
            ),
        ];
        let hits = apply(pairs);
        assert_eq!(hits[0].id.as_str(), "msg_v1_b");
        assert_eq!(hits[0].score, 2.0);
        assert_eq!(hits[1].score, 1.0);
    }

    #[test]
    fn apply_lexical_signals_ties_break_by_wire_id_ascending() {
        // 全零分（惩罚后钳 0）：tiebreak 落到 id 升序，跨运行稳定。
        let pairs = vec![
            (hit("zeta", 0.5), Some(payload(None, Some(true)))),
            (hit("alpha", 0.5), Some(payload(None, Some(true)))),
            (hit("middle", 0.5), Some(payload(None, Some(true)))),
        ];
        let hits = apply(pairs);
        let order: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(order, vec!["msg_v1_alpha", "msg_v1_middle", "msg_v1_zeta"]);
    }

    #[test]
    fn apply_lexical_signals_zero_scores_are_normalized_positive() {
        // `-0.0`/NaN bm25 归一为 +0.0（负零符号在平台上未定义，排序必须逐位稳定）。
        let pairs = vec![(hit("neg", -0.0), None), (hit("zero", 0.0), None)];
        let hits = apply(pairs);
        assert_eq!(hits[0].score.to_bits(), 0.0f32.to_bits());
        assert_eq!(hits[1].score.to_bits(), 0.0f32.to_bits());
    }
}
