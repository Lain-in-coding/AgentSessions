#!/usr/bin/env python3
"""Memory diagnostic: run one sync batch with RSS sampling and stage tracing.

Prints an RSS curve plus the trace stage boundaries so the peak can be located.
"""
from __future__ import annotations

import argparse
import ctypes
import json
import os
import pathlib
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path('.trellis/tasks/09-29-index-throughput/research/harness').resolve()))

class ProcessMemoryCounters(ctypes.Structure):
    _fields_ = [
        ('cb', ctypes.c_ulong),
        ('PageFaultCount', ctypes.c_ulong),
        ('PeakWorkingSetSize', ctypes.c_size_t),
        ('WorkingSetSize', ctypes.c_size_t),
        ('QuotaPeakPagedPoolUsage', ctypes.c_size_t),
        ('QuotaPagedPoolUsage', ctypes.c_size_t),
        ('QuotaPeakNonPagedPoolUsage', ctypes.c_size_t),
        ('QuotaNonPagedPoolUsage', ctypes.c_size_t),
        ('PagefileUsage', ctypes.c_size_t),
        ('PeakPagefileUsage', ctypes.c_size_t),
    ]

def sample(handle) -> tuple[float, float]:
    counters = ProcessMemoryCounters()
    counters.cb = ctypes.sizeof(counters)
    ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(handle), ctypes.byref(counters), counters.cb)
    return counters.WorkingSetSize / (1024 * 1024), counters.PeakWorkingSetSize / (1024 * 1024)

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    parser.add_argument('--db', required=True)
    parser.add_argument('--dataset', required=True)
    parser.add_argument('--trace', required=True)
    parser.add_argument('--interval-ms', type=float, default=20.0)
    args = parser.parse_args()
    pathlib.Path(args.trace).unlink(missing_ok=True)
    files = sorted(pathlib.Path(args.dataset).glob('session-*.jsonl'))
    command = [args.binary, '--db', args.db, '--output', 'json', 'sync', *(str(p) for p in files)]
    env = dict(os.environ, ASG_INDEX_TRACE=str(pathlib.Path(args.trace).resolve()))
    started = time.perf_counter()
    process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
    samples = []
    while process.poll() is None:
        current, peak = sample(process._handle)
        samples.append((round((time.perf_counter() - started) * 1000.0, 1), round(current, 1), round(peak, 1)))
        time.sleep(args.interval_ms / 1000.0)
    total_ms = round((time.perf_counter() - started) * 1000.0, 1)
    # downsample
    step = max(1, len(samples) // 40)
    print(json.dumps({'total_ms': total_ms, 'peak_mb': max(s[2] for s in samples),
                      'samples': samples[::step]}))
    print('trace records:')
    for line in pathlib.Path(args.trace).read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['label'] in {'cli:sync', 'adapter:catalog', 'adapter:commit', 'adapter:fts'}:
            print(f"  t={record['t_ms']:9.1f} {record['label']:18s} {json.dumps(record['stages_ms'])}")
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
