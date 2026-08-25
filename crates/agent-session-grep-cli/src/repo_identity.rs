//! Repo identity 派生（schema v16 `session_repo_slugs` 投影的写入侧依赖）。
//!
//! 借鉴来源：
//! - Recall `repo_identity.rs`：`git rev-parse --show-toplevel` +
//!   `remote get-url origin` 两级检测、按目录缓存、normalize 为
//!   host/owner/name 三段 slug；
//! - sessiongrep `find_repo_root`：worktree 感知——我们不自行解析 `.git`
//!   文件，`git rev-parse` 原生处理 worktree/submodule（返回工作树根），
//!   比手工走 `gitdir:` 指针更少出错。
//!
//! 诚实边界：检测失败（目录不存在 / 非 git 仓库 / 无 origin / URL 形状
//! 不认识 / slug 超长）一律 `None`，绝不猜。派生结果只含
//! `host/owner/name` 三段 slug——**绝不落绝对路径**（privacy 契约：
//! `scripts/evidence/privacy_scan.py` 会扫全部 tracked 文本）。
//!
//! 本模块只被 CLI 组合根注入到 [`SqliteStore`]（写路径），测试注入
//! 确定性的 fake；`git` 不在 PATH 或调用失败与"目录不是仓库"同义降级，
//! 不阻塞 sync/index。

use agent_session_grep_adapters_sqlite::RepoSlugResolver;
use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Command;

/// 派生 slug 最大长度（host/owner/name 三段之和）。超长即拒绝（None）——
/// 截断会破坏身份唯一性，把两个仓库并成一个。
pub const REPO_SLUG_MAX_CHARS: usize = 255;

/// 运行一次 git 并取 stdout 首行（trim）；spawn 失败 / 非零退出 /
/// 非 UTF-8 / 空输出一律 `None`。
fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// `git -C <cwd> rev-parse --show-toplevel`：目录在 git 工作树内时返回
/// 工作树根（worktree/submodule 也由 git 自身解析）；否则 None。
fn git_toplevel(cwd: &str) -> Option<String> {
    git_output(&["-C", cwd, "rev-parse", "--show-toplevel"])
}

/// `git -C <toplevel> remote get-url origin`：宿主仓的 origin 远端 URL；
/// 无 origin（本地仓）→ None。
fn git_origin_url(toplevel: &str) -> Option<String> {
    git_output(&["-C", toplevel, "remote", "get-url", "origin"])
}

/// 远端 URL → 三段 slug（`host/owner/name`）。
///
/// 支持五种常见形状（Recall 同款，但 host 通用不限 github.com）：
/// - `https://HOST/OWNER/NAME[.git]`（http 同）；
/// - `ssh://git@HOST/OWNER/NAME[.git]`；
/// - `git@HOST:OWNER/NAME[.git]`（scp-like）；
/// - `HOST:OWNER/NAME[.git]`（scp-like 无用户）；
/// - `HOST/OWNER/NAME[.git]`（无协议前缀）。
///
/// 不认识的形状（本地路径 / `file://` / Windows 盘符 / 带端口 / 多级
/// namespace / 空段）一律 None——诚实拒绝，绝不半猜。
pub fn normalize_remote_url(url: &str) -> Option<String> {
    let raw = url.trim().trim_end_matches('/');
    if raw.is_empty() {
        return None;
    }
    let (host, path) = if let Some(rest) = raw.strip_prefix("https://") {
        split_slash(rest)?
    } else if let Some(rest) = raw.strip_prefix("http://") {
        split_slash(rest)?
    } else if let Some(rest) = raw.strip_prefix("ssh://git@") {
        split_slash(rest)?
    } else if let Some(rest) = raw.strip_prefix("git@") {
        split_scp(rest)?
    } else if let Some((host, path)) = raw.split_once(':') {
        // scp-like 无用户（host:owner/name）：冒号前不得含 '/'，冒号后
        // 不得再含 ':'（多冒号是端口或 Windows 盘符，不猜）。
        if host.contains('/') || path.contains(':') {
            return None;
        }
        (host, path)
    } else {
        split_slash(raw)?
    };
    // 单字符 host 是 Windows 盘符（C:/...）而非宿主，拒绝；带端口拒绝。
    if host.len() < 2 || host.contains(':') {
        return None;
    }
    let path = path
        .trim_start_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path);
    let mut segments = path.split('/');
    let owner = segments.next()?.trim();
    let name = segments.next()?.trim();
    if owner.is_empty() || name.is_empty() || segments.next().is_some() {
        return None;
    }
    if owner.contains('\\') || name.contains('\\') {
        return None;
    }
    let slug = format!("{host}/{owner}/{name}");
    if slug.chars().count() > REPO_SLUG_MAX_CHARS {
        return None;
    }
    Some(slug)
}

/// `https://host/rest` 形状的 (host, rest) 拆分；host 为空 → None。
fn split_slash(rest: &str) -> Option<(&str, &str)> {
    let (host, path) = rest.split_once('/')?;
    if host.is_empty() {
        return None;
    }
    Some((host, path))
}

/// `host:owner/name` 形状的 (host, rest) 拆分；缺冒号 → None。
fn split_scp(rest: &str) -> Option<(&str, &str)> {
    let (host, path) = rest.split_once(':')?;
    if host.contains('/') || path.contains(':') {
        return None;
    }
    Some((host, path))
}

/// 带两级缓存的 git 解析器（Recall 同款）：
/// - 按 cwd → toplevel：同一目录的重复派生不重跑 `rev-parse`；
/// - 按 toplevel → slug：同仓库多个子目录共享一次 `remote get-url`。
///
/// 失败（None）同样入缓存——重建大批会话时"仓库已不存在"的 cwd 只
/// 探测一次。
#[derive(Default)]
pub struct GitRepoSlugResolver {
    by_directory: RefCell<HashMap<String, Option<String>>>,
    by_toplevel: RefCell<HashMap<String, Option<String>>>,
}

impl RepoSlugResolver for GitRepoSlugResolver {
    fn resolve(&self, cwd: &str) -> Option<String> {
        if cwd.trim().is_empty() {
            return None;
        }
        let toplevel = if let Some(cached) = self.by_directory.borrow().get(cwd) {
            cached.clone()
        } else {
            let toplevel = git_toplevel(cwd);
            self.by_directory
                .borrow_mut()
                .insert(cwd.to_string(), toplevel.clone());
            toplevel
        };
        let toplevel = toplevel?;
        if let Some(cached) = self.by_toplevel.borrow().get(&toplevel) {
            return cached.clone();
        }
        let slug = git_origin_url(&toplevel)
            .as_deref()
            .and_then(normalize_remote_url);
        self.by_toplevel.borrow_mut().insert(toplevel, slug.clone());
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// 测试专用组合：cwd → 检测本机 git 仓库并归一远端 URL，任何一步失败
    /// → None。生产路径走带缓存的 [`GitRepoSlugResolver`]（两级缓存把同一
    /// 仓库的多个目录共享一次 remote get-url），本函数锚定组合语义。
    /// 空/纯空白 cwd 与生产解析器同一门禁：直接拒绝（`git -C ""` 会退化为
    /// 当前目录，绝不能把调用者未提供目录时的宿主仓身份派生出来）。
    fn derive_repo_slug(cwd: &str) -> Option<String> {
        if cwd.trim().is_empty() {
            return None;
        }
        let toplevel = git_toplevel(cwd)?;
        let url = git_origin_url(&toplevel)?;
        normalize_remote_url(&url)
    }

    fn temp_git_repo(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("asg-repo-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .expect("git init");
        assert!(init.success(), "git init failed");
        root
    }

    fn add_origin(repo: &Path, url: &str) {
        let status = Command::new("git")
            .args(["remote", "add", "origin", url])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        assert!(status.success(), "git remote add failed");
    }

    // ---- 失败测试先行：检测失败绝不猜 ----

    #[test]
    fn derive_none_for_nonexistent_directory() {
        let missing = std::env::temp_dir().join(format!("asg-no-such-dir-{}", std::process::id()));
        assert_eq!(derive_repo_slug(missing.to_str().unwrap()), None);
    }

    #[test]
    fn derive_none_for_non_git_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(derive_repo_slug(dir.path().to_str().unwrap()), None);
    }

    #[test]
    fn derive_none_for_git_repo_without_origin() {
        let repo = temp_git_repo("no-origin");
        assert_eq!(derive_repo_slug(repo.to_str().unwrap()), None);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn derive_none_for_empty_cwd() {
        assert_eq!(derive_repo_slug(""), None);
        assert_eq!(derive_repo_slug("   "), None);
    }

    // ---- 成功路径：真实临时 git 仓库 ----

    #[test]
    fn derive_slug_from_git_directory_with_origin() {
        let repo = temp_git_repo("origin");
        add_origin(&repo, "git@github.com:synthetic-owner/synthetic-repo.git");
        assert_eq!(
            derive_repo_slug(repo.to_str().unwrap()).as_deref(),
            Some("github.com/synthetic-owner/synthetic-repo")
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn derive_slug_from_nested_subdirectory() {
        let repo = temp_git_repo("nested");
        add_origin(&repo, "https://gitlab.example.com/team/project.git");
        let sub = repo.join("packages").join("app");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            derive_repo_slug(sub.to_str().unwrap()).as_deref(),
            Some("gitlab.example.com/team/project")
        );
        let _ = fs::remove_dir_all(&repo);
    }

    // ---- 归一化：每种 URL 形状一个参数 ----

    #[test]
    fn normalizes_each_supported_url_shape() {
        for url in [
            "https://github.com/samzong/Recall.git",
            "https://github.com/samzong/Recall/",
            "http://github.com/samzong/Recall",
            "ssh://git@github.com/samzong/Recall.git",
            "git@github.com:samzong/Recall.git",
            "github.com:samzong/Recall",
            "github.com/samzong/Recall",
            "gitlab.example.com/team/notes.git",
        ] {
            let slug = normalize_remote_url(url).unwrap_or_else(|| panic!("{url:?}"));
            assert_eq!(
                slug.split('/').count(),
                3,
                "{url:?} -> {slug:?} 必须恒为 host/owner/name 三段"
            );
        }
        assert_eq!(
            normalize_remote_url("https://github.com/samzong/Recall.git").as_deref(),
            Some("github.com/samzong/Recall")
        );
        assert_eq!(
            normalize_remote_url("git@github.com:samzong/Recall.git").as_deref(),
            Some("github.com/samzong/Recall")
        );
    }

    #[test]
    fn rejects_unknown_shapes_instead_of_guessing() {
        for url in [
            "",                                   // 空
            "   ",                                // 空白
            "/srv/git/local-repo.git",            // 本地绝对路径
            "file:///srv/git/local-repo.git",     // file:// 传输
            "ssh://git@host:2222/owner/name.git", // 带端口
            "https://github.com:443/owner/name",  // 带端口
            "github.com/owner/sub/name",          // 多级 namespace
            "github.com/owner",                   // 缺 name 段
            "github.com//name",                   // 空 owner 段
            "https:///owner/name",                // 空 host
            "git@",                               // 无路径
            "owner/name.git",                     // 单段（无 host）
        ] {
            assert_eq!(
                normalize_remote_url(url),
                None,
                "{url:?} 必须诚实拒绝而不是半猜"
            );
        }
    }

    #[test]
    fn rejects_windows_drive_paths_as_hosts() {
        // 盘符路径会被误读成 host:path；单字符 host 必须拒绝。
        assert_eq!(normalize_remote_url("C:/repo/app"), None);
        assert_eq!(normalize_remote_url("D:\\repo\\app"), None);
    }

    #[test]
    fn rejects_slug_over_length_bound() {
        let owner = "o".repeat(REPO_SLUG_MAX_CHARS + 1);
        let url = format!("https://github.com/{owner}/name");
        assert_eq!(normalize_remote_url(&url), None);
        let owner = "o".repeat(REPO_SLUG_MAX_CHARS - "github.com//name".len());
        let url = format!("https://github.com/{owner}/name");
        assert!(normalize_remote_url(&url).is_some());
    }

    // ---- 解析器缓存：失败也缓存，同目录不重跑 git ----

    #[test]
    fn resolver_caches_failures_and_successes() {
        let resolver = GitRepoSlugResolver::default();
        let missing = std::env::temp_dir().join(format!("asg-no-such-dir-{}", std::process::id()));
        let missing = missing.to_str().unwrap().to_string();
        // 同一不存在目录解析两次：两次都 None（缓存命中，无 spawn）。
        assert_eq!(resolver.resolve(&missing), None);
        assert_eq!(resolver.resolve(&missing), None);

        let repo = temp_git_repo("cache");
        add_origin(&repo, "https://github.com/owner/cached.git");
        let sub = repo.join("sub");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            resolver.resolve(repo.to_str().unwrap()).as_deref(),
            Some("github.com/owner/cached")
        );
        assert_eq!(
            resolver.resolve(sub.to_str().unwrap()).as_deref(),
            Some("github.com/owner/cached")
        );
        // 删掉仓库后缓存仍返回已缓存结果（派生数据按重建批次的快照语义）。
        let _ = fs::remove_dir_all(&repo);
        assert_eq!(
            resolver.resolve(repo.to_str().unwrap()).as_deref(),
            Some("github.com/owner/cached")
        );
    }

    #[test]
    fn resolver_rejects_empty_cwd_without_spawning() {
        let resolver = GitRepoSlugResolver::default();
        assert_eq!(resolver.resolve(""), None);
        assert_eq!(resolver.resolve("  "), None);
    }
}
