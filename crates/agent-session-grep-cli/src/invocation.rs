//! 本次调用实际使用的二进制名（M4-6）。
//!
//! 同一个二进制以两个名字发布（`agent-session-grep` 与 `asg`，见
//! `Cargo.toml` 的两个 `[[bin]]`），而帮助文本、建议命令与 doctor 指引过去
//! 一律硬编码 `agent-session-grep`。以 `asg` 调用时，用户读到的每条"下一步"
//! 都写着另一个名字——照抄能跑通只是因为两个名字恰好都在 PATH 上，但输出与
//! 用户实际敲的东西不一致，这是产品面的不诚实。
//!
//! 名字取自 argv[0]：basename、去掉可执行扩展名（Windows 的 `.exe`）。
//! argv[0] 不可用或不像一个命令名时退回 [`DEFAULT_NAME`]——绝不打印一个空
//! 名字，也绝不把路径片段当命令名回显。

use std::ffi::OsStr;
use std::sync::OnceLock;

/// argv[0] 不可用时的稳定回退名。
///
/// 取 `asg` 而非 crate 名：`agent-session-grep-cli` 带一个用户不认识的 `-cli`
/// 后缀（crate 名不是命令名），而 README 与安装器主推的短名就是 `asg`。
pub const DEFAULT_NAME: &str = "asg";

/// 回显名的长度上限。argv[0] 由调用方完全控制（`execve` 可以塞任意字节），
/// 一个几 KB 的 argv[0] 不该被原样打进每一行帮助文本。
const MAX_NAME_CHARS: usize = 32;

/// 本进程的调用名。进程生命周期内 argv[0] 不变，故解析一次即缓存。
pub fn name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| resolve(std::env::args_os().next().as_deref()))
}

/// 从 argv[0] 解析调用名。纯函数，与进程环境无关，便于逐平台测试。
///
/// 拒绝（回退 [`DEFAULT_NAME`]）的输入：缺失、非 UTF-8、basename 为空、
/// 含空白或控制字符、超长。这些都不是用户敲得出来的命令名，回显它们只会让
/// 提示更难懂——而回退名一定能跑通（两个名字都随安装器上 PATH）。
pub fn resolve(argv0: Option<&OsStr>) -> String {
    let Some(raw) = argv0.and_then(OsStr::to_str) else {
        return DEFAULT_NAME.to_string();
    };
    // `\` 与 `/` 都当分隔符：Windows 上两者都合法，且 MSYS/Git-Bash 会把
    // 同一次调用写成正斜杠形式。
    let basename = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let stem = strip_executable_extension(basename);
    if stem.is_empty()
        || stem.chars().count() > MAX_NAME_CHARS
        || stem
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '/' || c == '\\')
    {
        return DEFAULT_NAME.to_string();
    }
    stem.to_string()
}

/// 去掉 Windows 可执行扩展名（大小写不敏感）。
///
/// 只去 `.exe`：`.bat`/`.cmd` 包装器的名字就是用户敲的东西，`.com` 已无实际
/// 使用。非 Windows 平台上二进制通常无扩展名，但交叉挂载的 `.exe` 同样按名
/// 处理——判定只看字符串，不看当前平台。
fn strip_executable_extension(basename: &str) -> &str {
    let lowered = basename.to_ascii_lowercase();
    match lowered.strip_suffix(".exe") {
        Some(stem) => &basename[..stem.len()],
        None => basename,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn resolved(argv0: &str) -> String {
        resolve(Some(OsString::from(argv0).as_os_str()))
    }

    #[test]
    fn bare_names_pass_through() {
        assert_eq!(resolved("asg"), "asg");
        assert_eq!(resolved("agent-session-grep"), "agent-session-grep");
    }

    #[test]
    fn basename_is_taken_from_either_separator() {
        assert_eq!(resolved("/usr/local/bin/asg"), "asg");
        assert_eq!(resolved("./asg"), "asg");
        assert_eq!(
            resolved(r"C:\Program Files\asg\agent-session-grep.exe"),
            "agent-session-grep"
        );
        assert_eq!(resolved("C:/tools/asg.EXE"), "asg");
    }

    #[test]
    fn only_the_exe_extension_is_stripped() {
        // `.exe` 去掉；其它后缀是名字的一部分（`asg.bat` 就是用户敲的东西）。
        assert_eq!(resolved("asg.exe"), "asg");
        assert_eq!(resolved("asg.bat"), "asg.bat");
        // 名字本身就叫 `.exe` 时去完为空 → 回退，不打印空名字。
        assert_eq!(resolved(".exe"), DEFAULT_NAME);
    }

    #[test]
    fn unusable_argv0_falls_back_to_the_default_name() {
        assert_eq!(resolve(None), DEFAULT_NAME);
        assert_eq!(resolved(""), DEFAULT_NAME);
        assert_eq!(resolved("/usr/local/bin/"), DEFAULT_NAME);
        // 空白与控制字符不是命令名：回显它们会让整行提示不可读。
        assert_eq!(resolved("my tool"), DEFAULT_NAME);
        assert_eq!(resolved("asg\n--help"), DEFAULT_NAME);
        // 超长 argv[0]（调用方可以任意构造）不进帮助文本。
        assert_eq!(resolved(&"a".repeat(MAX_NAME_CHARS + 1)), DEFAULT_NAME);
        assert_eq!(
            resolved(&"a".repeat(MAX_NAME_CHARS)),
            "a".repeat(MAX_NAME_CHARS)
        );
    }

    #[test]
    fn non_utf8_argv0_falls_back() {
        // 非 UTF-8 的 argv[0] 无法安全回显（有损转换会打印替换字符）。
        #[cfg(windows)]
        let bad = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0xD800]) // 落单的高位代理，不是合法 UTF-16
        };
        #[cfg(unix)]
        let bad = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0x66, 0x80, 0x6F])
        };
        assert_eq!(resolve(Some(bad.as_os_str())), DEFAULT_NAME);
    }

    #[test]
    fn process_name_is_non_empty_and_stable() {
        // 真实进程下的取值：测试二进制名任意，但必须是非空、可回显的一个词。
        let first = name();
        assert!(!first.is_empty());
        assert!(!first.contains(char::is_whitespace));
        assert_eq!(first, name(), "同一进程内必须稳定");
    }
}
