#!/usr/bin/env python3
"""Generate a fresh synthetic corpus with ids offset past an existing catalog."""
from __future__ import annotations

import argparse
import importlib.util
import json
import pathlib
import sys

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', required=True)
    parser.add_argument('--messages', type=int, default=200000)
    parser.add_argument('--per-file', type=int, default=5000)
    parser.add_argument('--offset', type=int, default=1000000)
    args = parser.parse_args()
    sys.path.insert(0, str(pathlib.Path('scripts/evidence').resolve()))
    import core_beta_benchmark as bench
    root = pathlib.Path(args.out)
    root.mkdir(parents=True, exist_ok=True)
    index = args.offset
    for file_index, first in enumerate(range(0, args.messages, args.per_file)):
        path = root / f'session-{file_index:06d}.jsonl'
        with path.open('x', encoding='utf-8', newline='\n') as handle:
            for local in range(first, min(first + args.per_file, args.messages)):
                record = bench.synthetic_record(index, local)
                handle.write(json.dumps(record, ensure_ascii=False, separators=(',', ':')) + '\n')
                index += 1
    print(f'wrote {args.messages} messages to {root}')
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
