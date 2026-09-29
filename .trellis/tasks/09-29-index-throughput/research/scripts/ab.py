#!/usr/bin/env python3
"""Interleaved A/B driver around the screening harness."""
from __future__ import annotations

import argparse
import json
import pathlib
import shutil
import statistics
import subprocess
import sys

SCREEN = pathlib.Path(__file__).with_name('screen.py')


def run(label: str, workspace: str, binary: str, dataset: str, scratch_root: pathlib.Path,
        commit: str, tag: str, noop: int, searches: int) -> dict:
    scratch = scratch_root / f'{tag}-{label}'
    shutil.rmtree(scratch, ignore_errors=True)
    command = [sys.executable, '-B', str(SCREEN), '--workspace', workspace, '--binary', binary,
               '--dataset', dataset, '--scratch', str(scratch), '--label', label,
               '--expected-commit', commit, '--noop-passes', str(noop),
               '--search-samples', str(searches)]
    out = subprocess.run(command, capture_output=True, text=True, check=True)
    return json.loads(out.stdout.strip().splitlines()[-1])


def med(rows: list[dict], key: str) -> float:
    return statistics.median([r[key] for r in rows])


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('--a-workspace', required=True)
    parser.add_argument('--a-binary', required=True)
    parser.add_argument('--a-commit', required=True)
    parser.add_argument('--b-workspace', required=True)
    parser.add_argument('--b-binary', required=True)
    parser.add_argument('--b-commit', required=True)
    parser.add_argument('--dataset', required=True)
    parser.add_argument('--scratch-root', required=True)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--pairs', type=int, default=2)
    parser.add_argument('--noop-passes', type=int, default=3)
    parser.add_argument('--search-samples', type=int, default=20)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()

    root = pathlib.Path(args.scratch_root)
    root.mkdir(parents=True, exist_ok=True)
    records = {'tag': args.tag, 'a_commit': args.a_commit, 'b_commit': args.b_commit,
               'pairs': args.pairs, 'a': [], 'b': []}
    for index in range(args.pairs):
        for label, side in (('a', 'a'), ('b', 'b')):
            result = run(label, getattr(args, f'{side}_workspace'), getattr(args, f'{side}_binary'),
                         args.dataset, root, getattr(args, f'{side}_commit'),
                         f'{args.tag}-p{index}', args.noop_passes, args.search_samples)
            records[side].append(result)
            print(json.dumps(result))
    summary = {'tag': args.tag, 'pairs': args.pairs}
    for side in ('a', 'b'):
        rows = records[side]
        summary[side] = {
            'initial_ms': med(rows, 'initial_ms'),
            'noop_median_ms': med(rows, 'noop_median_ms'),
            'rss_mb': med(rows, 'initial_rss_max_mb'),
            'search_median_ms': med(rows, 'search_median_ms') if 'search_median_ms' in rows[0] else None,
            'db_bytes': rows[0]['db_bytes'],
        }
    delta = {}
    for key in ('initial_ms', 'noop_median_ms', 'rss_mb', 'search_median_ms'):
        a, b = summary['a'][key], summary['b'][key]
        delta[key] = None if not a or not b else round(100.0 * (b - a) / a, 2)
    summary['delta_pct'] = delta
    pathlib.Path(args.out).write_text(json.dumps({'summary': summary, 'records': records},
                                                 ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(summary))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
