//! 只读源快照读取（RFC-0002 §4 / 计划审查 #6）。
//!
//! 打开后捕获 `(len, mtime_ms, fingerprint)`，只读捕获范围；提交前复核三元组，
//! 任一变化返回 [`PortError::SnapshotChanged`]。
//!
//! **等长异容替换**必须靠 content fingerprint——len+mtime 不足
//! （证据：`spikes/source-snapshot/EVIDENCE.md` assertion D）。

use agentsessions_ports::{PortError, PortResult, SourceSnapshot};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn backend<E: std::fmt::Display>(e: E) -> PortError {
    PortError::SourceIo(e.to_string())
}

fn mtime_ms(meta: &std::fs::Metadata) -> PortResult<i64> {
    let m = meta.modified().map_err(backend)?;
    Ok(m.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64)
}

fn fingerprint_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// 打开路径、捕获快照元数据并读入捕获范围的字节。
///
/// 返回 `(snapshot, bytes)`。调用方在处理完后应再调 [`verify_snapshot`]
/// 做提交前复核（或使用 [`read_verified`] 一次完成）。
pub fn capture(path: &Path) -> PortResult<(SourceSnapshot, Vec<u8>)> {
    let mut file = File::open(path).map_err(backend)?;
    let meta = file.metadata().map_err(backend)?;
    let len = meta.len();
    let mtime = mtime_ms(&meta)?;
    let mut buf = Vec::with_capacity(len as usize);
    file.read_to_end(&mut buf).map_err(backend)?;
    // 固定捕获长度：若读期间文件被追加，仍只认初始 len 范围。
    buf.truncate(len as usize);
    let fingerprint = fingerprint_hex(&buf);
    let snap = SourceSnapshot {
        path: path.to_string_lossy().into_owned(),
        len,
        mtime_ms: mtime,
        fingerprint,
    };
    Ok((snap, buf))
}

/// 提交前复核：重读元数据与捕获范围内容，任一变化即
/// [`PortError::SnapshotChanged`]。
pub fn verify_snapshot(path: &Path, snap: &SourceSnapshot) -> PortResult<()> {
    let meta = std::fs::metadata(path).map_err(backend)?;
    let cur_len = meta.len();
    let cur_mtime = mtime_ms(&meta)?;
    if cur_len != snap.len {
        return Err(PortError::SnapshotChanged(format!(
            "len {} -> {cur_len}",
            snap.len
        )));
    }
    if cur_mtime != snap.mtime_ms {
        return Err(PortError::SnapshotChanged(format!(
            "mtime {} -> {cur_mtime}",
            snap.mtime_ms
        )));
    }
    let mut file = File::open(path).map_err(backend)?;
    let mut buf = Vec::with_capacity(snap.len as usize);
    file.read_to_end(&mut buf).map_err(backend)?;
    buf.truncate(snap.len as usize);
    let cur_fp = fingerprint_hex(&buf);
    if cur_fp != snap.fingerprint {
        return Err(PortError::SnapshotChanged(
            "fingerprint changed (content replaced at same len)".into(),
        ));
    }
    Ok(())
}

/// 捕获并立即复核——适合"读取即提交前"的简单路径。
///
/// 若在 capture 与 verify 之间源被改写，返回 SnapshotChanged。
pub fn read_verified(path: &Path) -> PortResult<(SourceSnapshot, Vec<u8>)> {
    let (snap, bytes) = capture(path)?;
    verify_snapshot(path, &snap)?;
    Ok((snap, bytes))
}

/// 基于已有快照元数据的只读路径发现器（实现 [`SourceDiscovery`] 的最小落地）。
///
/// 构造时给定一组已 capture 的快照；`discover` 返回它们的克隆，
/// `read_verified` 按路径重读并校验三元组。用于测试与组合根装配。
pub struct SnapshotFs {
    snapshots: Vec<SourceSnapshot>,
}

impl SnapshotFs {
    pub fn new(snapshots: Vec<SourceSnapshot>) -> Self {
        Self { snapshots }
    }
}

impl agentsessions_ports::SourceDiscovery for SnapshotFs {
    fn discover(&self) -> PortResult<Vec<SourceSnapshot>> {
        Ok(self.snapshots.clone())
    }

    fn read_verified(&self, snapshot: &SourceSnapshot) -> PortResult<Vec<u8>> {
        let path = Path::new(&snapshot.path);
        verify_snapshot(path, snapshot)?;
        let mut file = File::open(path).map_err(backend)?;
        let mut buf = Vec::with_capacity(snapshot.len as usize);
        file.read_to_end(&mut buf).map_err(backend)?;
        buf.truncate(snapshot.len as usize);
        Ok(buf)
    }
}

// 抑制未使用警告：SystemTime 仅经 mtime_ms 间接使用。
#[allow(dead_code)]
fn _system_time_is_available(_: SystemTime) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(dir: &Path, name: &str, content: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        let mut f = File::create(&p).unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        p
    }

    #[test]
    fn capture_and_verify_stable_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(dir.path(), "a.jsonl", b"hello stable content");
        let (snap, bytes) = capture(&p).unwrap();
        assert_eq!(bytes, b"hello stable content");
        assert_eq!(snap.len, 20);
        verify_snapshot(&p, &snap).unwrap();
    }

    #[test]
    fn append_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(dir.path(), "a.jsonl", b"hello");
        let (snap, _) = capture(&p).unwrap();
        // 追加
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
            f.write_all(b" world").unwrap();
        }
        let err = verify_snapshot(&p, &snap).unwrap_err();
        assert!(matches!(err, PortError::SnapshotChanged(ref m) if m.contains("len")));
    }

    #[test]
    fn truncate_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(dir.path(), "a.jsonl", b"hello world!!!!!");
        let (snap, _) = capture(&p).unwrap();
        {
            let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
            f.set_len(5).unwrap();
        }
        let err = verify_snapshot(&p, &snap).unwrap_err();
        assert!(matches!(err, PortError::SnapshotChanged(ref m) if m.contains("len")));
    }

    #[test]
    fn equal_length_content_replacement_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(dir.path(), "a.jsonl", b"AAAAAAAAAA"); // 10 bytes
        let (snap, _) = capture(&p).unwrap();
        // 等长异容替换：len 相同，fingerprint 必变。
        // 尽量保持 mtime 一致的难度高，但 fingerprint 检查独立于 mtime。
        std::fs::write(&p, b"BBBBBBBBBB").unwrap();
        let err = verify_snapshot(&p, &snap).unwrap_err();
        // 可能先撞上 mtime 变化，也可能直接 fingerprint——两种都是 SnapshotChanged。
        assert!(matches!(err, PortError::SnapshotChanged(_)), "got {err:?}");
    }

    #[test]
    fn snapshot_fs_read_verified_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(dir.path(), "s.jsonl", b"payload-bytes");
        let (snap, _) = capture(&p).unwrap();
        let fs = SnapshotFs::new(vec![snap.clone()]);
        use agentsessions_ports::SourceDiscovery;
        let discovered = fs.discover().unwrap();
        assert_eq!(discovered.len(), 1);
        let bytes = fs.read_verified(&snap).unwrap();
        assert_eq!(bytes, b"payload-bytes");
    }
}
