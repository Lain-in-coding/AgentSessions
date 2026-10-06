#!/usr/bin/env python3
"""Fast paired A/B screener for the index-throughput task.

Reuses the task harness's process/memory plumbing (evidence.run_process) but
measures only the sync stages (initial + noop + optional search samples) on a
fresh catalog, so interleaved before/after runs stay affordable.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import statistics
import sys
import time

HARNESS = pathlib.Path('.trellis/tasks/09-29-index-throughput/research/harness').resolve()
sys.path.insert(0, str(HARNESS))
import evidence as ev  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('--workspace', required=True)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--dataset', required=True)
    parser.add_argument('--scratch', required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--max-sources', type=int, default=40)
    parser.add_argument('--noop-passes', type=int, default=3)
    parser.add_argument('--search-samples', type=int, default=0)
    parser.add_argument('--expected-commit', required=True)
    args = parser.parse_args()

    workspace = ev.absolute_path(args.workspace, 'workspace')
    binary = ev.absolute_path(args.binary, 'binary')
    scratch = pathlib.Path(args.scratch).resolve()
    scratch.mkdir(parents=True, exist_ok=False)
    dataset = pathlib.Path(args.dataset).resolve()
    helpers = ev.load_helpers(workspace)
    commit = ev.check_workspace(workspace, args.expected_commit)
    db = scratch / 'catalog.db'
    files = sorted(dataset.glob('session-*.jsonl'))
    batches = [files[i:i + args.max_sources] for i in range(0, len(files), args.max_sources)]
    roots = {'workspace': workspace, 'scratch': scratch, 'binary': binary}
    deadline = time.monotonic() + 3600.0
    base = [str(binary), '--db', str(db), '--output', 'json', 'sync']

    def run_pass(label: str) -> dict:
        started = time.perf_counter()
        events = []
        for index, batch in enumerate(batches):
            command = [*base, *(str(p) for p in batch)]
            event, output = ev.run_process(command, workspace=workspace, scratch=scratch,
                                           label=f'{label}-b{index}', timeout=900.0,
                                           deadline=deadline, helpers=helpers, roots=roots)
            frame = json.loads(output)
            data = frame.get('data', {})
            events.append({'status': event['status'], 'ms': event['duration_ms'],
                           'rss_mb': event.get('peak_rss_mb'),
                           'committed': data.get('committed'),
                           'unchanged': data.get('unchanged')})
        total = (time.perf_counter() - started) * 1000.0
        return {'label': label, 'total_ms': round(total, 1), 'batches': events,
                'rss_max_mb': max((e['rss_mb'] or 0) for e in events)}

    result = {'label': args.label, 'commit': commit, 'binary': ev.artifact(binary, helpers),
              'file_count': len(files), 'batch_count': len(batches)}
    initial = run_pass('initial')
    noops = [run_pass(f'noop-{i}') for i in range(args.noop_passes)]
    result['initial'] = initial
    result['noop'] = noops
    result['noop_ms'] = [p['total_ms'] for p in noops]
    result['noop_median_ms'] = statistics.median(result['noop_ms'])
    if args.search_samples:
        fixture = ev.load_fixture()
        pairs = [(q['id'], q['text']) for q in fixture['queries'] if q['id'] in
                 ('substring', 'cjk_two', 'snake', 'namespace', 'error')]
        events = []
        for index in range(args.search_samples):
            query_id, text = pairs[index % len(pairs)]
            command = [str(binary), '--db', str(db), '--output', 'json', 'search', text,
                       '--max-items', '20']
            event, output = ev.run_process(command, workspace=workspace, scratch=scratch,
                                           label=f'search-{index:03d}', timeout=600.0,
                                           deadline=deadline, helpers=helpers, roots=roots)
            frame = json.loads(output)
            events.append({'status': event['status'], 'ms': event['duration_ms'],
                           'query_id': query_id, 'hits': len(frame.get('data', {}).get('hits', []))})
        result['search'] = events
        result['search_median_ms'] = statistics.median([e['ms'] for e in events])
    out = scratch / 'screener.json'
    out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
    summary = {
        'label': result['label'],
        'commit': result['commit'],
        'binary_sha256': result['binary']['sha256'][:16],
        'initial_ms': result['initial']['total_ms'],
        'initial_batches_ms': [b['ms'] for b in result['initial']['batches']],
        'initial_rss_max_mb': result['initial']['rss_max_mb'],
        'noop_ms': result['noop_ms'],
        'noop_median_ms': result['noop_median_ms'],
        'db_bytes': (scratch / 'catalog.db').stat().st_size if (scratch / 'catalog.db').exists() else None,
    }
    if 'search_median_ms' in result:
        summary['search_median_ms'] = result['search_median_ms']
    print(json.dumps(summary))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
