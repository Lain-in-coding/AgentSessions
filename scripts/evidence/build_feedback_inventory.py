"""Read-only, identity-deduplicated Windows build-cache inventory.

Only NTFS handle measurements can produce a complete receipt. ``roots`` are
known worktrees/build/runtime roots; discovery also lists registered Git
worktrees without changing an index. ``discover=False`` treats roots as
explicit disposable target/artifact roots. Stores must be supplied explicitly;
an absent optional store is not a failed scan. No cache is deleted.
Receipts contain machine-local paths and must not be checked into the project.
"""

from __future__ import annotations

import ctypes
import json
import os
import subprocess
import tempfile
from collections.abc import Iterable
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, NamedTuple

GIB = 1024 ** 3
REVIEW_GROWTH_BYTES = 40 * GIB
HARD_GROWTH_BYTES = 60 * GIB
MINIMUM_FREE_BYTES = 80 * GIB

_CACHE_TAG = b"Signature: 8a477f597d28d172789f06886806bc55"
_SOURCE_DIRECTORIES = frozenset({
    "src", "resources", "fixtures", "tests", "benches", "docs", "schemas",
    "scripts", "node_modules", "vendor", ".agents", ".codex", ".codebuddy",
    ".claude", ".github", ".venv", "__pycache__",
})
_EXCLUDED_DIRECTORIES = frozenset({".git", ".codegraph", "reference", "references"})
_CARGO_DOWNLOAD_CHILDREN = {
    "registry": frozenset({"src", "cache", "index"}),
    "git": frozenset({"db", "checkouts"}),
}
_ARTIFACT_SUFFIXES = frozenset({
    ".exe", ".dll", ".pdb", ".rlib", ".rmeta", ".obj", ".lib", ".o", ".a", ".so",
})
_REPARSE_POINT = 0x400
_SPARSE_FILE = 0x200
_COMPRESSED = 0x800
_ALLOWED_REPARSE_TAGS = frozenset({0xA0000003, 0xA000000C})  # junction / symlink


class UnsupportedMeasurement(OSError):
    """A scan cannot be certified from the available filesystem metadata."""


class _FileIdInfo(ctypes.Structure):
    _fields_ = [("VolumeSerialNumber", ctypes.c_uint64), ("FileId", ctypes.c_ubyte * 16)]


class _FileStandardInfo(ctypes.Structure):
    _fields_ = [
        ("AllocationSize", ctypes.c_int64), ("EndOfFile", ctypes.c_int64),
        ("NumberOfLinks", ctypes.c_uint32), ("DeletePending", ctypes.c_ubyte),
        ("Directory", ctypes.c_ubyte),
    ]


class _FileBasicInfo(ctypes.Structure):
    _fields_ = [
        ("CreationTime", ctypes.c_int64), ("LastAccessTime", ctypes.c_int64),
        ("LastWriteTime", ctypes.c_int64), ("ChangeTime", ctypes.c_int64),
        ("FileAttributes", ctypes.c_uint32),
    ]


class _FileCompressionInfo(ctypes.Structure):
    _fields_ = [
        ("CompressedFileSize", ctypes.c_int64), ("CompressionFormat", ctypes.c_uint16),
        ("CompressionUnitShift", ctypes.c_ubyte), ("ChunkShift", ctypes.c_ubyte),
        ("ClusterShift", ctypes.c_ubyte), ("Reserved", ctypes.c_ubyte * 3),
    ]


class _FileStreamInfo(ctypes.Structure):
    _fields_ = [
        ("NextEntryOffset", ctypes.c_uint32), ("StreamNameLength", ctypes.c_uint32),
        ("StreamSize", ctypes.c_int64), ("StreamAllocationSize", ctypes.c_int64),
    ]


class _Metadata(NamedTuple):
    identity: tuple[str, str]
    directory: bool
    allocated_bytes: int
    logical_bytes: int
    last_write: int
    change_time: int
    attributes: int
    resolved_path: str
    stream_count: int
    links: int

    @property
    def stamp(self) -> tuple:
        return (self.identity, self.directory, self.last_write, self.change_time,
                self.attributes, self.links)


def _stamp() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def _absolute(path: os.PathLike | str) -> Path:
    if not isinstance(path, (str, os.PathLike)) or not os.fspath(path) or "\0" in os.fspath(path):
        raise ValueError("root must be a nonempty filesystem path")
    return Path(os.path.abspath(path))


def _long_path(path: os.PathLike | str) -> str:
    value = os.path.abspath(path)
    if os.name != "nt" or value.startswith("\\\\?\\"):
        return value
    if value.startswith("\\\\"):
        return "\\\\?\\UNC\\" + value[2:]
    return "\\\\?\\" + value


def _display_path(value: str) -> str:
    if value.startswith("\\\\?\\UNC\\"):
        return "\\\\" + value[8:]
    if value.startswith("\\\\?\\"):
        return value[4:]
    return value


def _inside(child: Path, parent: Path) -> bool:
    try:
        child.relative_to(parent)
        return True
    except ValueError:
        return False


class _WindowsMetadata:
    """FileIdInfo + NTFS allocations, including named data streams.

    AllocationSize is used for ordinary streams, NOT logical length.
    FileCompressionInfo supplies allocation for compressed/sparse streams.
    Filesystem/MFT overhead is not attributable to individual cache files.
    """

    def __init__(self) -> None:
        if os.name != "nt":
            raise UnsupportedMeasurement("Windows NTFS capture is required; no approximate fallback")
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.CreateFileW.argtypes = [
            ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p,
            ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p,
        ]
        self.kernel.CreateFileW.restype = ctypes.c_void_p
        self.kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        self.kernel.CloseHandle.restype = ctypes.c_int
        self.kernel.GetFileInformationByHandleEx.argtypes = [
            ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32,
        ]
        self.kernel.GetFileInformationByHandleEx.restype = ctypes.c_int
        self.kernel.GetFinalPathNameByHandleW.argtypes = [
            ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32,
        ]
        self.kernel.GetFinalPathNameByHandleW.restype = ctypes.c_uint32
        self.kernel.GetVolumeInformationByHandleW.argtypes = [
            ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_uint32,
            ctypes.POINTER(ctypes.c_uint32), ctypes.POINTER(ctypes.c_uint32),
            ctypes.POINTER(ctypes.c_uint32), ctypes.c_wchar_p, ctypes.c_uint32,
        ]
        self.kernel.GetVolumeInformationByHandleW.restype = ctypes.c_int
        self.kernel.GetDiskFreeSpaceExW.argtypes = [
            ctypes.c_wchar_p, ctypes.POINTER(ctypes.c_uint64),
            ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(ctypes.c_uint64),
        ]
        self.kernel.GetDiskFreeSpaceExW.restype = ctypes.c_int
        self.volumes: dict[str, dict] = {}

    @contextmanager
    def _open(self, path: os.PathLike | str):
        # Following reparse points is intentional; their targets are deduplicated.
        handle = self.kernel.CreateFileW(_long_path(path), 0x80, 7, None, 3, 0x02000000, None)
        if handle == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            yield handle
        finally:
            if not self.kernel.CloseHandle(handle):
                raise ctypes.WinError(ctypes.get_last_error())

    def _info(self, handle, information_class: int, structure):
        result = structure()
        if not self.kernel.GetFileInformationByHandleEx(
            handle, information_class, ctypes.byref(result), ctypes.sizeof(result)
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        return result

    def _volume(self, handle, serial: str) -> None:
        if serial in self.volumes:
            return
        name = ctypes.create_unicode_buffer(261)
        flags = ctypes.c_uint32()
        if not self.kernel.GetVolumeInformationByHandleW(
            handle, None, 0, None, None, ctypes.byref(flags), name, len(name)
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        if name.value.upper() != "NTFS" or flags.value & 0x08000000:
            raise UnsupportedMeasurement("only NTFS without shared block extents is supported")
        self.volumes[serial] = {"volume_serial": serial, "filesystem": name.value, "flags": flags.value}

    def _resolved(self, handle) -> str:
        length = 512
        while length <= 32768:
            buffer = ctypes.create_unicode_buffer(length)
            count = self.kernel.GetFinalPathNameByHandleW(handle, buffer, length, 0)
            if count == 0:
                raise ctypes.WinError(ctypes.get_last_error())
            if count < length:
                return _display_path(buffer.value)
            length = count + 1
        raise UnsupportedMeasurement("resolved path exceeds the Windows path limit")

    def _streams(self, handle, directory: bool) -> list[tuple[str, int, int]]:
        length = 4096
        while length <= 16 * 1024 * 1024:
            buffer = ctypes.create_string_buffer(length)
            if self.kernel.GetFileInformationByHandleEx(handle, 7, buffer, length):
                break
            error = ctypes.get_last_error()
            if error == 38 and directory:  # ERROR_HANDLE_EOF: no directory data streams
                return []
            if error not in (122, 234):
                raise ctypes.WinError(error)
            length *= 2
        else:
            raise UnsupportedMeasurement("data-stream metadata exceeds the capture limit")
        result: list[tuple[str, int, int]] = []
        offset = 0
        seen: set[str] = set()
        while True:
            header_size = ctypes.sizeof(_FileStreamInfo)
            if offset + header_size > length:
                raise UnsupportedMeasurement("truncated data-stream metadata")
            header = _FileStreamInfo.from_buffer_copy(buffer.raw, offset)
            size = int(header.StreamNameLength)
            if size == 0 and not result and directory:
                return []
            end = offset + header_size + size
            if size == 0 or size % 2 or end > length:
                raise UnsupportedMeasurement("invalid data-stream name metadata")
            name = buffer.raw[offset + header_size:end].decode("utf-16-le", "strict")
            if not name.startswith(":") or not name.endswith(":$DATA") or name in seen:
                raise UnsupportedMeasurement("unknown or duplicate data stream")
            if header.StreamSize < 0 or header.StreamAllocationSize < 0:
                raise UnsupportedMeasurement("negative data-stream size")
            result.append((name, int(header.StreamSize), int(header.StreamAllocationSize)))
            seen.add(name)
            step = int(header.NextEntryOffset)
            if step == 0:
                return result
            if step < header_size + size or offset + step >= length:
                raise UnsupportedMeasurement("invalid data-stream metadata offset")
            offset += step

    def _allocation(self, handle, standard, basic) -> int:
        if standard.AllocationSize < 0 or standard.EndOfFile < 0 or standard.DeletePending:
            raise UnsupportedMeasurement("unavailable, negative, or delete-pending file metadata")
        if basic.FileAttributes & (_COMPRESSED | _SPARSE_FILE):
            compression = self._info(handle, 8, _FileCompressionInfo)
            if compression.CompressedFileSize < 0:
                raise UnsupportedMeasurement("negative compressed/sparse allocation")
            return int(compression.CompressedFileSize)
        return int(standard.AllocationSize)

    def read(self, path: Path, include_streams: bool = True) -> _Metadata:
        link = os.lstat(_long_path(path))
        if (getattr(link, "st_file_attributes", 0) & _REPARSE_POINT
                and getattr(link, "st_reparse_tag", 0) not in _ALLOWED_REPARSE_TAGS):
            raise UnsupportedMeasurement("unknown reparse-point allocation/identity semantics")
        with self._open(path) as handle:
            file_id = self._info(handle, 18, _FileIdInfo)
            identity = (f"{file_id.VolumeSerialNumber:016x}", bytes(file_id.FileId).hex())
            self._volume(handle, identity[0])
            basic = self._info(handle, 0, _FileBasicInfo)
            standard = self._info(handle, 1, _FileStandardInfo)
            if not any(file_id.FileId) or standard.DeletePending:
                raise UnsupportedMeasurement("unknown identity or delete-pending file")
            directory = bool(standard.Directory)
            resolved = self._resolved(handle)
            allocated = 0 if directory else self._allocation(handle, standard, basic)
            logical = 0 if directory else int(standard.EndOfFile)
            streams = self._streams(handle, directory) if include_streams else []
            if include_streams:
                allocated, logical, unnamed = 0, 0, False
                for name, stream_logical, stream_allocated in streams:
                    if name == "::$DATA":
                        if directory or unnamed or stream_logical != standard.EndOfFile:
                            raise UnsupportedMeasurement("inconsistent unnamed data stream")
                        if (not basic.FileAttributes & (_COMPRESSED | _SPARSE_FILE)
                                and stream_allocated != standard.AllocationSize):
                            raise UnsupportedMeasurement("inconsistent ordinary allocation")
                        allocated += self._allocation(handle, standard, basic)
                        logical += stream_logical
                        unnamed = True
                    else:
                        with self._open(str(path) + name) as stream_handle:
                            stream_id = self._info(stream_handle, 18, _FileIdInfo)
                            if (f"{stream_id.VolumeSerialNumber:016x}", bytes(stream_id.FileId).hex()) != identity:
                                raise UnsupportedMeasurement("data-stream file identity changed")
                            stream_standard = self._info(stream_handle, 1, _FileStandardInfo)
                            stream_basic = self._info(stream_handle, 0, _FileBasicInfo)
                            if stream_standard.EndOfFile != stream_logical:
                                raise UnsupportedMeasurement("data-stream size changed during capture")
                            allocated += self._allocation(stream_handle, stream_standard, stream_basic)
                            logical += stream_logical
                if not directory and not unnamed and (standard.EndOfFile or standard.AllocationSize):
                    raise UnsupportedMeasurement("unnamed data-stream allocation was not captured")
            after = self._info(handle, 0, _FileBasicInfo)
            after_standard = self._info(handle, 1, _FileStandardInfo)
            if (basic.LastWriteTime, basic.ChangeTime, basic.FileAttributes,
                standard.EndOfFile, standard.AllocationSize, standard.NumberOfLinks) != (
                    after.LastWriteTime, after.ChangeTime, after.FileAttributes,
                    after_standard.EndOfFile, after_standard.AllocationSize, after_standard.NumberOfLinks):
                raise UnsupportedMeasurement("file changed during metadata capture")
            return _Metadata(identity, directory, allocated, logical, int(basic.LastWriteTime),
                             int(basic.ChangeTime), int(basic.FileAttributes), resolved,
                             len(streams), int(standard.NumberOfLinks))

    def free_bytes(self) -> int:
        available, total, free = ctypes.c_uint64(), ctypes.c_uint64(), ctypes.c_uint64()
        if not self.kernel.GetDiskFreeSpaceExW(
            "C:\\", ctypes.byref(available), ctypes.byref(total), ctypes.byref(free)
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        return int(available.value)


def _new_metadata_reader():
    return _WindowsMetadata()


class _Inventory:
    def __init__(self, reader) -> None:
        self.reader = reader
        self.roots: list[dict] = []
        self.caches: list[dict] = []
        self.errors: list[dict] = []
        self.aliases: list[dict] = []
        self.relationships: list[dict] = []
        self.exclusions: list[dict] = []
        self._exclusion_by_path: dict[tuple[str, str], int] = {}
        self._root_by_path: dict[str, int] = {}
        self._cache_by_path: dict[str, int] = {}
        self._discovered: dict[tuple, tuple[str, _Metadata]] = {}
        self._directories: dict[tuple, tuple[str, int, _Metadata]] = {}
        self._files: dict[tuple, tuple[str, int, _Metadata]] = {}
        self._aliases_seen: set[tuple] = set()
        self._git_seen: set[tuple] = set()
        self._download_roots = [
            _absolute(Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))) / child)
            for child in ("registry", "git")
        ]

    def error(self, path: Path | str, operation: str, exc: Exception,
              root: int | None = None, cache: int | None = None) -> None:
        error = {"path": str(path), "operation": operation, "code": type(exc).__name__,
                 "message": str(exc), "at": _stamp()}
        for attribute in ("errno", "winerror"):
            value = getattr(exc, attribute, None)
            if value is not None:
                error[attribute] = value
        self.errors.append(error)
        if root is not None:
            self.roots[root]["complete"] = False
            self.roots[root]["errors"].append(error)
        if cache is not None:
            self.caches[cache]["complete"] = False
            self.caches[cache]["errors"].append(error)
            for owner in self.caches[cache]["root_indexes"]:
                if owner != root:
                    self.roots[owner]["complete"] = False
                    self.roots[owner]["errors"].append(error)

    def add_root(self, path: Path, kind: str, origin: str | None = None) -> int:
        key = str(path)
        if key in self._root_by_path:
            index = self._root_by_path[key]
            root = self.roots[index]
            # An explicit store declaration must not be lost when the same
            # path was already supplied as a discovery root. Known roots stay
            # mandatory even if they also appear in the optional-store list.
            if kind == "store":
                root["kind"] = "store"
            else:
                root["required"] = True
            if origin and origin not in root["origins"]:
                root["origins"].append(origin)
            return index
        index = len(self.roots)
        self._root_by_path[key] = index
        self.roots.append({
            "path": key, "kind": kind, "origins": [origin] if origin else [],
            "required": kind != "store", "status": "pending", "complete": True, "errors": [], "cache_indexes": [],
            "allocated_bytes": 0, "logical_bytes": 0,
        })
        return index

    def add_cache(self, path: Path, kind: str, root: int, evidence: str) -> int:
        key = str(path)
        if key in self._cache_by_path:
            index = self._cache_by_path[key]
            if root not in self.caches[index]["root_indexes"]:
                self.caches[index]["root_indexes"].append(root)
            if evidence not in self.caches[index]["evidence"]:
                self.caches[index]["evidence"].append(evidence)
        else:
            index = len(self.caches)
            self._cache_by_path[key] = index
            self.caches.append({
                "path": key, "kind": kind, "root_indexes": [root], "evidence": [evidence],
                "status": "pending", "complete": True, "errors": [], "allocated_bytes": 0,
                "logical_bytes": 0, "file_count": 0, "directory_count": 0,
                "started_at": None, "finished_at": None,
            })
        if index not in self.roots[root]["cache_indexes"]:
            self.roots[root]["cache_indexes"].append(index)
        return index

    def alias(self, path: Path | str, original: str, identity: tuple, kind: str) -> None:
        if str(path) == original:
            return
        key = (str(path), original, kind)
        if key not in self._aliases_seen:
            self._aliases_seen.add(key)
            self.aliases.append({"path": str(path), "alias_of": original, "kind": kind,
                                 "identity": {"volume_serial": identity[0], "file_id": identity[1]}})

    def exclusion_reason(self, path: Path, discovery: bool = False,
                         entries: list | None = None) -> str | None:
        parts = [part.lower() for part in path.parts]
        if any(part == ".git" or part.endswith(".git") for part in parts):
            return "git_internal"
        if any(part in _EXCLUDED_DIRECTORIES for part in parts):
            return "reference_or_index"
        if any(_inside(path, root) for root in self._download_roots):
            return "cargo_download"
        for index in range(len(parts) - 1):
            if parts[index] == ".cargo" and parts[index + 1] in _CARGO_DOWNLOAD_CHILDREN:
                return "cargo_download"
            children = _CARGO_DOWNLOAD_CHILDREN.get(parts[index], ())
            if parts[index + 1] in children:
                return "cargo_download"
        # Historical/runtime Cargo homes need not be the current CARGO_HOME or
        # named .cargo. Recognize their registry/git layout, not arbitrary blobs.
        if entries is not None:
            children = _CARGO_DOWNLOAD_CHILDREN.get(path.name.lower(), ())
            if any(entry.name.lower() in children for entry in entries):
                return "cargo_download"
        if discovery:
            if path.name.lower() in _SOURCE_DIRECTORIES:
                return "source_directory"
            if path.parent.name.lower() == ".trellis" and path.name.lower() != ".runtime":
                return "workflow_source"
        return None

    def excluded(self, path: Path, discovery: bool = False) -> bool:
        return self.exclusion_reason(path, discovery) is not None

    def record_exclusion(self, path: Path, reason: str, root: int | None = None,
                         cache: int | None = None, resolved_path: str | None = None) -> None:
        key = (str(path), reason)
        if key not in self._exclusion_by_path:
            self._exclusion_by_path[key] = len(self.exclusions)
            self.exclusions.append({
                "path": str(path), "reason": reason, "status": "excluded", "at": _stamp(),
                "root_indexes": [], "cache_indexes": [],
            })
        item = self.exclusions[self._exclusion_by_path[key]]
        owners = list(self.caches[cache]["root_indexes"]) if cache is not None else []
        if root is not None:
            owners.append(root)
        for owner in owners:
            if owner not in item["root_indexes"]:
                item["root_indexes"].append(owner)
        if cache is not None and cache not in item["cache_indexes"]:
            item["cache_indexes"].append(cache)
        if resolved_path is not None:
            item["resolved_path"] = resolved_path

    def entries(self, path: Path, root: int | None = None, cache: int | None = None):
        try:
            with os.scandir(_long_path(path)) as handle:
                return sorted(handle, key=lambda entry: entry.name)
        except OSError as exc:
            self.error(path, "list_directory", exc, root, cache)
            return None

    def worktrees(self, path: Path, metadata: _Metadata, entries: list, root: int) -> None:
        if metadata.identity in self._git_seen or not any(entry.name == ".git" for entry in entries):
            return
        self._git_seen.add(metadata.identity)
        environment = dict(os.environ, GIT_OPTIONAL_LOCKS="0")
        try:
            result = subprocess.run(
                ["git", "-C", str(path), "worktree", "list", "--porcelain", "-z"],
                capture_output=True, check=False,
                timeout=30, env=environment,
            )
            if result.returncode != 0:
                raise UnsupportedMeasurement("registered worktree discovery failed: " +
                                             result.stderr.decode("utf-8", "replace").strip())
            found = False
            for field in result.stdout.decode("utf-8", "strict").split("\0"):
                if field.startswith("worktree "):
                    found = True
                    worktree = _absolute(field[9:])
                    reason = self.exclusion_reason(worktree)
                    if reason is not None:
                        # Submodule/bare administration paths from Git are not
                        # user-declared caches. Keep the exclusion, not a failed
                        # known root. Explicit declarations still fail below.
                        self.record_exclusion(worktree, reason, root=root)
                        continue
                    self.add_root(worktree, "registered_worktree", str(path))
            if not found:
                raise UnsupportedMeasurement("Git returned no registered worktree paths")
        except (OSError, ValueError, subprocess.TimeoutExpired) as exc:
            self.error(path, "discover_registered_worktrees", exc, root)

    def marker(self, path: Path, entries: list, root: int) -> str | None:
        names = {entry.name.lower(): entry for entry in entries}
        # A source root cannot become a cache merely by having a target-like name.
        if "cargo.toml" in names or ".git" in names:
            return None
        if path.name.lower() == "target":
            return "cargo_target_name"
        if ".rustc_info.json" in names:
            return "cargo_rustc_info"
        if ".fingerprint" in names and ("deps" in names or "build" in names):
            return "cargo_profile_fingerprints"
        if ".cargo-lock" in names and "deps" in names:
            return "cargo_profile_lock"
        if "cachedir.tag" in names:
            try:
                with open(_long_path(path / names["cachedir.tag"].name), "rb") as handle:
                    if handle.read(len(_CACHE_TAG)) == _CACHE_TAG:
                        return "cache_directory_tag"
            except OSError as exc:
                self.error(path / names["cachedir.tag"].name, "read_cache_marker", exc, root)
        return None

    def discover_root(self, index: int, start_path: Path | None = None,
                      targets_only: bool = False, cache: int | None = None) -> None:
        root = self.roots[index]
        root_path = Path(root["path"])
        pending = [(root_path if start_path is None else start_path, targets_only)]
        while pending:
            path, only_targets = pending.pop()
            explicit = start_path is None and path == root_path
            reason = self.exclusion_reason(path, discovery=True)
            if reason is not None:
                if explicit:
                    self.error(path, "excluded_root", ValueError("source/Git/download reference is outside cache scope"), index, cache)
                    continue
                self.record_exclusion(path, reason, index, cache)
                if reason not in {"source_directory", "workflow_source"}:
                    continue
                only_targets = True
            try:
                metadata = self.reader.read(path, include_streams=False)
                resolved = Path(metadata.resolved_path)
                reason = self.exclusion_reason(resolved, discovery=True)
                if reason is not None:
                    if explicit:
                        raise UnsupportedMeasurement("resolved root is outside cache scope")
                    self.record_exclusion(path, reason, index, cache, metadata.resolved_path)
                    if reason not in {"source_directory", "workflow_source"}:
                        continue
                    only_targets = True
                if explicit:
                    root["identity"] = {"volume_serial": metadata.identity[0], "file_id": metadata.identity[1]}
                    root["resolved_path"] = metadata.resolved_path
                if not metadata.directory:
                    if not only_targets and path.suffix.lower() in _ARTIFACT_SUFFIXES:
                        self.add_cache(path, "runtime_artifact", index, "explicit_file" if explicit else "compiled_artifact_suffix")
                    elif explicit:
                        raise UnsupportedMeasurement("explicit file root is not a compiled runtime artifact")
                    continue
                if metadata.identity in self._discovered:
                    original, previous = self._discovered[metadata.identity]
                    if previous.stamp != metadata.stamp:
                        raise UnsupportedMeasurement("aliased directory changed during cache discovery")
                    self.alias(path, original, metadata.identity, "directory")
                    # An explicit aliased root still needs a cache/coverage record.
                    if explicit:
                        for cache_index, item in enumerate(self.caches):
                            if _inside(Path(item["path"]), Path(original)):
                                if index not in item["root_indexes"]:
                                    item["root_indexes"].append(index)
                                if cache_index not in root["cache_indexes"]:
                                    root["cache_indexes"].append(cache_index)
                    continue
                self._discovered[metadata.identity] = (str(path), metadata)
                entries = self.entries(path, index, cache)
                if entries is None:
                    continue
                reason = (self.exclusion_reason(path, entries=entries) or
                          self.exclusion_reason(resolved, entries=entries))
                if reason is not None:
                    if explicit:
                        raise UnsupportedMeasurement("source/Git/download reference is outside cache scope")
                    self.record_exclusion(path, reason, index, cache, metadata.resolved_path)
                else:
                    if not only_targets:
                        self.worktrees(path, metadata, entries, index)
                    if any(entry.name.lower() in {"cargo.toml", ".git"} for entry in entries):
                        self.record_exclusion(path, "source_worktree", index, cache, metadata.resolved_path)
                        only_targets = True
                    marker = self.marker(path, entries, index)
                    if marker:
                        self.add_cache(path, "cargo_target", index, marker)
                        if cache is not None:
                            for owner in self.caches[cache]["root_indexes"]:
                                self.add_cache(path, "cargo_target", owner, marker)
                    else:
                        # Traverse source directories only to find real target
                        # markers. Never inventory source files/fixture binaries
                        # or query unrelated Git worktrees from an owned cache.
                        for entry in reversed(entries):
                            child = path / entry.name
                            reason = self.exclusion_reason(child)
                            if reason is not None:
                                self.record_exclusion(child, reason, index, cache)
                                continue
                            try:
                                if entry.is_dir(follow_symlinks=True):
                                    pending.append((child, only_targets))
                                elif entry.is_symlink() or entry.stat(follow_symlinks=False).st_mode == 0:
                                    self.reader.read(child, include_streams=False)
                                elif not only_targets and child.suffix.lower() in _ARTIFACT_SUFFIXES:
                                    self.add_cache(child, "runtime_artifact", index, "compiled_artifact_suffix")
                            except OSError as exc:
                                self.error(child, "discover_entry", exc, index, cache)
                after = self.reader.read(path, include_streams=False)
                if metadata.stamp != after.stamp:
                    raise UnsupportedMeasurement("directory changed during cache discovery")
            except (OSError, ValueError) as exc:
                self.error(path, "discover_root", exc, index, cache)
        if start_path is None:
            root["status"] = "discovered" if root["complete"] else "error"

    def scan(self, index: int) -> None:
        cache = self.caches[index]
        root = Path(cache["path"])
        cache["started_at"] = _stamp()
        pending = [(root, None)]
        while pending:
            path, before = pending.pop()
            reason = self.exclusion_reason(path, discovery=path == root)
            if reason is not None:
                if path == root:
                    self.error(path, "excluded_root", UnsupportedMeasurement("explicit cache/store root is outside cache scope"), cache=index)
                else:
                    self.record_exclusion(path, reason, cache=index)
                continue
            try:
                if before is not None:
                    after = self.reader.read(path, include_streams=False)
                    if before.stamp != after.stamp:
                        raise UnsupportedMeasurement("directory changed during allocation scan")
                    continue
                metadata = self.reader.read(path)
                resolved = Path(metadata.resolved_path)
                reason = self.exclusion_reason(resolved, discovery=path == root)
                if reason is not None:
                    if path == root:
                        raise UnsupportedMeasurement("resolved cache/store root is outside cache scope")
                    self.record_exclusion(path, reason, cache=index, resolved_path=metadata.resolved_path)
                    continue
                if path == root:
                    if (not metadata.directory
                            and (cache["kind"] == "store" or path.suffix.lower() not in _ARTIFACT_SUFFIXES)):
                        raise UnsupportedMeasurement("explicit cache/store file root is not a compiled runtime artifact")
                    cache["identity"] = {"volume_serial": metadata.identity[0], "file_id": metadata.identity[1]}
                    cache["resolved_path"] = metadata.resolved_path
                entries = None
                if metadata.directory:
                    entries = self.entries(path, cache=index)
                    if entries is None:
                        continue
                    reason = (self.exclusion_reason(path, entries=entries) or
                              self.exclusion_reason(resolved, entries=entries))
                    source = any(entry.name.lower() in {"cargo.toml", ".git"} for entry in entries)
                    if reason is not None or source:
                        if path == root:
                            raise UnsupportedMeasurement("explicit target/store or junction overlaps a source/Git/download root")
                        self.record_exclusion(path, reason or "source_worktree", cache=index,
                                              resolved_path=metadata.resolved_path)
                        if source and reason is None:
                            self.discover_root(min(cache["root_indexes"]), start_path=path,
                                               targets_only=True, cache=index)
                        after = self.reader.read(path, include_streams=False)
                        if metadata.stamp != after.stamp:
                            raise UnsupportedMeasurement("directory changed during allocation scan")
                        continue
                seen = self._directories if metadata.directory else self._files
                if metadata.identity in seen:
                    original, owner, *previous = seen[metadata.identity]
                    self.alias(path, original, metadata.identity, "directory" if metadata.directory else "file")
                    if previous and (previous[0].stamp != metadata.stamp or
                                     (previous[0].allocated_bytes, previous[0].logical_bytes) !=
                                     (metadata.allocated_bytes, metadata.logical_bytes)):
                        raise UnsupportedMeasurement("aliased entry changed during allocation scan")
                    if path == root:
                        cache["included_in"] = self.caches[owner]["path"]
                    continue
                seen[metadata.identity] = (str(path), index, metadata)
                if metadata.directory:
                    cache["directory_count"] += 1
                    pending.append((path, metadata))
                    if entries is None:
                        raise UnsupportedMeasurement("directory entries unavailable during allocation scan")
                    for entry in reversed(entries):
                        pending.append((path / entry.name, None))
                else:
                    cache["file_count"] += 1
                cache["allocated_bytes"] += metadata.allocated_bytes
                cache["logical_bytes"] += metadata.logical_bytes
            except (OSError, ValueError) as exc:
                self.error(path, "scan_allocation", exc, cache=index)
        cache["finished_at"] = _stamp()
        cache["status"] = "scanned" if cache["complete"] else "error"
        if cache.get("included_in"):
            cache["status"] = "covered" if cache["complete"] else "error"

    def finish(self) -> None:
        for child in self.caches:
            parents = [parent for parent in self.caches if parent is not child and
                       Path(child["path"]) != Path(parent["path"]) and
                       _inside(Path(child["path"]), Path(parent["path"]))]
            if parents:
                parent = max(parents, key=lambda item: len(Path(item["path"]).parts))
                child["parent_cache"] = parent["path"]
                self.relationships.append({"kind": "contains", "parent": parent["path"], "child": child["path"]})
            if child.get("included_in"):
                self.relationships.append({"kind": "covered_by", "parent": child["included_in"], "child": child["path"]})
                owner = self.caches[self._cache_by_path[child["included_in"]]]
                if not owner["complete"]:
                    child["complete"] = False
                    child["status"] = "error"
            if child["root_indexes"]:
                owner = self.roots[min(child["root_indexes"])]
                owner["allocated_bytes"] += child["allocated_bytes"]
                owner["logical_bytes"] += child["logical_bytes"]
        for root in self.roots:
            if any(not self.caches[index]["complete"] for index in root["cache_indexes"]):
                root["complete"] = False
            if root["status"] == "pending":
                root["status"] = "scanned" if root["complete"] else "error"


def inventory(roots: Iterable[os.PathLike | str], store_roots: Iterable[os.PathLike | str] = (),
              discover: bool = True) -> dict[str, Any]:
    """Inventory supplied related roots, with global volume/file-identity dedup.

    Totals, root totals and cache totals are unique contributions, not sums of
    overlapping apparent directory sizes.  ``complete=False`` denotes partial
    observed totals which MUST NOT be used for admission or a fixed baseline.
    Optional missing stores are recorded as absent; missing known roots fail.
    """
    started = _stamp()
    if type(discover) is not bool:
        raise ValueError("discover must be a boolean")
    if isinstance(roots, (str, os.PathLike)):
        roots = [roots]
    if isinstance(store_roots, (str, os.PathLike)):
        store_roots = [store_roots]
    paths = [_absolute(path) for path in roots]
    stores = [_absolute(path) for path in store_roots]
    reader, initialization_error = None, None
    try:
        reader = _new_metadata_reader()
    except OSError as exc:
        initialization_error = exc
    scan = _Inventory(reader)
    for path in paths:
        scan.add_root(path, "known_root" if discover else "explicit_cache")
    for path in stores:
        scan.add_root(path, "store")
    if not paths and not stores:
        scan.error("", "inventory_scope", ValueError("at least one explicitly known root is required"))
    samples = []
    if initialization_error is not None or reader is None:
        scan.error("", "initialize_windows_capture",
                   initialization_error or UnsupportedMeasurement("NTFS metadata reader unavailable"))
        for root in scan.roots:
            root["status"], root["complete"] = "unsupported", False
    else:
        for boundary in ("before", "after"):
            if boundary == "after":
                index = 0
                while index < len(scan.roots):
                    root = scan.roots[index]
                    path = Path(root["path"])
                    if root["kind"] == "store":
                        try:
                            # lstat distinguishes absent stores from dangling links.
                            os.lstat(_long_path(path))
                            scan.add_cache(path, "store", index, "explicit_store")
                        except FileNotFoundError:
                            if root["required"]:
                                scan.error(path, "probe_known_store", FileNotFoundError("known cache/store root is missing"), index)
                            else:
                                root["status"] = "absent"
                                cache = scan.add_cache(path, "store", index, "optional_store_absent")
                                scan.caches[cache]["status"] = "absent"
                        except OSError as exc:
                            scan.error(path, "probe_optional_store", exc, index)
                    elif discover:
                        scan.discover_root(index)
                    else:
                        scan.add_cache(path, "explicit_cache", index, "explicit_cache_root")
                    index += 1
                scanned = set()
                while True:
                    pending = [item for item in range(len(scan.caches))
                               if item not in scanned and scan.caches[item]["status"] != "absent"]
                    if not pending:
                        break
                    # A source fixture found inside a target can expose another
                    # genuine target. Scan new caches too, with the same global
                    # identity tables and parent-first deduplication.
                    cache = min(pending, key=lambda item: (
                        len(Path(scan.caches[item]["path"]).parts), scan.caches[item]["path"]))
                    scanned.add(cache)
                    scan.scan(cache)
                scan.finish()
            try:
                samples.append({"boundary": boundary, "at": _stamp(), "available_bytes": reader.free_bytes()})
            except OSError as exc:
                scan.error("C:\\", "capture_disk_free", exc)
    free = min((sample["available_bytes"] for sample in samples), default=None)
    complete = not scan.errors
    admission_reasons = []
    if not complete:
        admission_reasons.append("inventory_incomplete")
    if free is None:
        admission_reasons.append("disk_free_unknown")
    elif free < MINIMUM_FREE_BYTES:
        admission_reasons.append("disk_free_below_minimum")
    return {
        "schema_version": 1, "kind": "build-feedback-inventory", "started_at": started,
        "finished_at": _stamp(), "platform": os.name, "discover": discover,
        "scope": "explicit known roots, registered worktrees, and explicit optional stores",
        "measurement_method": "windows-ntfs-file-id-and-handle-allocation",
        "allocation_scope": "all file data streams; excludes filesystem/MFT overhead, source, Git objects, Cargo downloads",
        "totals_are_unique_contributions": True,
        "roots": scan.roots, "caches": scan.caches, "aliases": scan.aliases,
        "relationships": scan.relationships, "exclusions": scan.exclusions,
        "errors": scan.errors, "complete": complete,
        "admission_allowed": not admission_reasons, "admission_reasons": admission_reasons,
        "allocated_bytes": sum(cache["allocated_bytes"] for cache in scan.caches),
        "logical_bytes": sum(cache["logical_bytes"] for cache in scan.caches),
        "file_count": len(scan._files), "directory_count": len(scan._directories),
        "volumes": list(getattr(reader, "volumes", {}).values()),
        "disk_root": "C:\\", "free_bytes": free, "disk_free_bytes": free,
        "disk_free_samples": samples,
    }


def budget_status(baseline_bytes: int, current_bytes: int, free_bytes: int,
                  predicted_growth_bytes: int = 0) -> dict[str, Any]:
    """Fixed-B admission; headroom is AFTER the proposed growth reservation.

    The soft threshold requires a separate owner review, not a reset of B.
    Equality at B+60 GiB stops; remaining disk space of exactly 80 GiB is valid.
    Invalid/unknown sizes cannot accidentally become an allowed plan.
    """
    values = {"baseline_bytes": baseline_bytes, "current_bytes": current_bytes,
              "free_bytes": free_bytes, "predicted_growth_bytes": predicted_growth_bytes}
    invalid = ["invalid_" + key for key, value in values.items() if type(value) is not int or value < 0]
    result = {"allowed": False, "review_required": False, "reasons": invalid,
              "project_headroom_bytes": None, "disk_headroom_bytes": None,
              "usable_headroom_bytes": None, **values}
    if invalid:
        return result
    projected = current_bytes + predicted_growth_bytes
    projected_free = free_bytes - predicted_growth_bytes
    review = baseline_bytes + REVIEW_GROWTH_BYTES
    hard = baseline_bytes + HARD_GROWTH_BYTES
    reasons = []
    if projected >= review:
        reasons.append("project_review_threshold_reached")
    if current_bytes >= hard:
        reasons.append("project_hard_limit_reached")
    elif projected >= hard:
        reasons.append("projected_project_hard_limit_reached")
    if free_bytes < MINIMUM_FREE_BYTES:
        reasons.append("disk_free_below_minimum")
    elif projected_free < MINIMUM_FREE_BYTES:
        reasons.append("projected_disk_free_below_minimum")
    project_headroom = hard - projected
    disk_headroom = projected_free - MINIMUM_FREE_BYTES
    result.update({
        "allowed": projected < hard and projected_free >= MINIMUM_FREE_BYTES,
        "review_required": projected >= review, "reasons": reasons,
        "projected_bytes": projected, "projected_free_bytes": projected_free,
        "review_threshold_bytes": review, "hard_threshold_bytes": hard,
        "minimum_free_bytes": MINIMUM_FREE_BYTES,
        "growth_since_baseline_bytes": current_bytes - baseline_bytes,
        "project_headroom_bytes": project_headroom, "disk_headroom_bytes": disk_headroom,
        "usable_headroom_bytes": max(0, min(project_headroom, disk_headroom)),
    })
    return result


def atomic_json(path: os.PathLike | str, payload: Any) -> None:
    """Fsync then atomically publish a NEW immutable JSON receipt.

    Hard-link publication cannot overwrite an existing file on either Windows
    or POSIX.  Only this invocation's private pending file is unlinked.  Existing
    baselines/receipts, caches and failures are never modified or removed.
    """
    destination = _absolute(path)
    content = (json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True, allow_nan=False) + "\n").encode("utf-8")
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix="." + destination.name + ".pending-", dir=destination.parent)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.link(temporary, destination)
    finally:
        os.unlink(temporary)
