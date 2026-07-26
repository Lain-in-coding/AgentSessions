//! Thread/Branch 选择:对 Canonical threading 事实(parent 指针 + sidechain 标记)
//! 的确定性纯函数规则。纯函数、无 I/O——领域层对"哪条链是主线"的唯一裁决。

use crate::Message;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 会话上下文选择策略(CONTRACT-cli-robot-mcp-draft §1 context_policy)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPolicy {
    /// 主线:排除 sidechain,沿最新叶子的 parent 链回溯。
    Mainline,
    /// 全量:按 seq 序返回全部消息。
    Full,
}

/// 一次分支选择的结果:叶子与 root→leaf 有序消息链。
#[derive(Debug, PartialEq)]
pub struct BranchSelection<'a> {
    pub leaf: &'a Message,
    pub messages: Vec<&'a Message>,
}

/// 主线选择:确定性地选出"最新主线分支"的 root→leaf 消息链。
///
/// 规则(CONTRACT-cli-robot-mcp-draft §1 `context_policy = mainline`):
///
/// 1. 候选集 = `is_sidechain == false` 的消息;候选集为空(全部为
///    sidechain)时诚实退化:候选集 = 全部消息,绝不臆造主线。
/// 2. 叶子 = 候选集中 `seq` 最大者(合法会话内 seq 唯一;畸形输入出现
///    并列时取输入中靠后者,仅为保持确定性)。
/// 3. 自叶子沿 `parent` 链回溯收集:父 id 经 id→消息映射查找;父 id
///    不在映射中即平滑终止(容忍跨文件父指针,如 sidechain 根指向主
///    transcript),已收集的链原样返回。
/// 4. 环安全:回溯以已访问集去重,parent 链成环时遇重访即停——对恶意
///    或畸形输入绝不挂起、绝不 panic。
/// 5. 结果按 root→leaf 排序;不在回溯链上的旁支兄弟(fork 的旧分支)
///    被排除。空输入返回 `None`。
///
/// 确定性:输出完全由 `seq`/`parent`/`is_sidechain` 事实决定,链序来自
/// parent 回溯而非任何映射迭代顺序——同一输入切片必得同一输出。
pub fn select_mainline(messages: &[Message]) -> Option<BranchSelection<'_>> {
    let leaf = messages
        .iter()
        .filter(|m| !m.is_sidechain)
        .max_by_key(|m| m.seq)
        .or_else(|| messages.iter().max_by_key(|m| m.seq))?;

    let by_id: BTreeMap<&str, &Message> = messages.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut chain: Vec<&Message> = vec![leaf];
    let mut visited: BTreeSet<&str> = BTreeSet::new();
    visited.insert(leaf.id.as_str());
    let mut current = leaf;
    while let Some(parent_id) = &current.parent {
        // 父不在映射中:跨文件父指针,平滑终止。
        let Some(&parent) = by_id.get(parent_id.as_str()) else {
            break;
        };
        // 重访即成环:停止回溯,已收集链即结果。
        if !visited.insert(parent.id.as_str()) {
            break;
        }
        chain.push(parent);
        current = parent;
    }
    chain.reverse();
    Some(BranchSelection {
        leaf,
        messages: chain,
    })
}

/// 全量选择:按 `seq` 升序返回全部消息(sidechain 一并包含)。
///
/// 输入允许乱序——以稳定排序按 `seq` 排列,`seq` 并列时保持输入相对
/// 顺序,确保同一输入必得同一输出。
pub fn select_full(messages: &[Message]) -> Vec<&Message> {
    let mut all: Vec<&Message> = messages.iter().collect();
    all.sort_by_key(|m| m.seq);
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IdKind, Role, Stability, StableId};

    fn mid(tag: &str) -> StableId {
        StableId::derive(IdKind::Message, Stability::Reconstructed, &[tag.as_bytes()])
    }

    /// 构造测试消息:tag 决定 id,parent 以 tag 引用,span 恒为 None。
    fn msg(tag: &str, seq: u32, parent: Option<&str>, is_sidechain: bool) -> Message {
        Message {
            id: mid(tag),
            parent: parent.map(mid),
            role: Role::User,
            text: format!("m{seq}"),
            seq,
            timestamp: None,
            is_sidechain,
            span: None,
        }
    }

    fn seqs(msgs: &[&Message]) -> Vec<u32> {
        msgs.iter().map(|m| m.seq).collect()
    }

    #[test]
    fn linear_chain_returns_root_to_leaf() {
        let msgs = vec![
            msg("r", 0, None, false),
            msg("a", 1, Some("r"), false),
            msg("b", 2, Some("a"), false),
        ];
        let sel = select_mainline(&msgs).unwrap();
        assert_eq!(sel.leaf.id, mid("b"));
        assert_eq!(seqs(&sel.messages), vec![0, 1, 2]);
    }

    #[test]
    fn fork_newest_branch_wins_and_sibling_excluded() {
        // r → a → a2 为旧分支;r → b 为编辑重试的新分支(seq 最大)。
        let msgs = vec![
            msg("r", 0, None, false),
            msg("a", 1, Some("r"), false),
            msg("a2", 2, Some("a"), false),
            msg("b", 3, Some("r"), false),
        ];
        let sel = select_mainline(&msgs).unwrap();
        assert_eq!(sel.leaf.id, mid("b"));
        // 旁支 a/a2 不在回溯链上,被排除。
        assert_eq!(seqs(&sel.messages), vec![0, 3]);
    }

    #[test]
    fn sidechain_excluded_from_mainline_but_present_in_full() {
        let msgs = vec![
            msg("r", 0, None, false),
            msg("m1", 1, Some("r"), false),
            msg("s1", 2, Some("m1"), true),
            msg("m2", 3, Some("m1"), false),
        ];
        let sel = select_mainline(&msgs).unwrap();
        assert_eq!(sel.leaf.id, mid("m2"));
        assert_eq!(seqs(&sel.messages), vec![0, 1, 3]);
        // 全量视图包含 sidechain。
        assert_eq!(seqs(&select_full(&msgs)), vec![0, 1, 2, 3]);
    }

    #[test]
    fn all_sidechain_falls_back_to_all_messages() {
        let msgs = vec![msg("s0", 0, None, true), msg("s1", 1, Some("s0"), true)];
        let sel = select_mainline(&msgs).unwrap();
        assert_eq!(sel.leaf.id, mid("s1"));
        assert_eq!(seqs(&sel.messages), vec![0, 1]);
    }

    #[test]
    fn orphan_parent_stops_walk_without_panic() {
        // "ghost" 不在切片内(跨文件父),链在 m 处平滑终止。
        let msgs = vec![
            msg("m", 0, Some("ghost"), false),
            msg("c", 1, Some("m"), false),
        ];
        let sel = select_mainline(&msgs).unwrap();
        assert_eq!(sel.leaf.id, mid("c"));
        assert_eq!(seqs(&sel.messages), vec![0, 1]);
    }

    #[test]
    fn parent_cycle_terminates() {
        // a ⇄ b 成环:遇重访即停,不挂起不 panic。
        let msgs = vec![msg("a", 0, Some("b"), false), msg("b", 1, Some("a"), false)];
        let sel = select_mainline(&msgs).unwrap();
        assert_eq!(sel.leaf.id, mid("b"));
        assert_eq!(seqs(&sel.messages), vec![0, 1]);
    }

    #[test]
    fn self_parent_cycle_terminates() {
        // 自指父指针:链只含叶子自身。
        let msgs = vec![msg("x", 0, Some("x"), false)];
        let sel = select_mainline(&msgs).unwrap();
        assert_eq!(seqs(&sel.messages), vec![0]);
    }

    #[test]
    fn empty_input_returns_none() {
        assert!(select_mainline(&[]).is_none());
        assert!(select_full(&[]).is_empty());
    }

    #[test]
    fn select_full_sorts_unsorted_input_by_seq() {
        let msgs = vec![
            msg("c", 2, None, false),
            msg("a", 0, None, false),
            msg("b", 1, None, true),
        ];
        assert_eq!(seqs(&select_full(&msgs)), vec![0, 1, 2]);
    }

    #[test]
    fn same_input_yields_identical_output() {
        let msgs = vec![
            msg("r", 0, None, false),
            msg("s", 1, Some("r"), true),
            msg("a", 2, Some("r"), false),
            msg("b", 3, Some("a"), false),
        ];
        assert_eq!(select_mainline(&msgs), select_mainline(&msgs));
        assert_eq!(select_full(&msgs), select_full(&msgs));
    }

    #[test]
    fn context_policy_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&ContextPolicy::Mainline).unwrap(),
            "\"mainline\""
        );
        assert_eq!(
            serde_json::from_str::<ContextPolicy>("\"full\"").unwrap(),
            ContextPolicy::Full
        );
    }
}
