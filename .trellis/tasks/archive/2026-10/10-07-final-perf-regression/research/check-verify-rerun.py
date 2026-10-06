"""B7 independent re-run comparison: deliverable after-measurements.json vs
the reviewer's own check-after-measurements.json (fresh fixture directory).
Tolerance: median within +/-30%, probe counts identical, exit codes all zero.
"""
import json
from pathlib import Path

research = Path(r"C:\AgentSessions\.trellis\tasks\10-07-final-perf-regression\research")
deliv = json.loads((research / "after-measurements.json").read_text(encoding="utf-8"))
rerun = json.loads((research / "check-after-measurements.json").read_text(encoding="utf-8"))


def median(v):
    o = sorted(v)
    n = len(o)
    return o[n // 2] if n % 2 else (o[n // 2 - 1] + o[n // 2]) / 2.0


lines = ["=== B7 independent hotpath re-run (check-measure.ps1, fresh fixture dir) ==="]
ok = True
lines.append(f"deliverable binary sha256: {deliv['binary_sha256']}")
lines.append(f"rerun       binary sha256: {rerun['binary_sha256']}")
lines.append(f"deliverable commit: {deliv['commit']} | rerun commit: {rerun['commit']}")
lines.append(f"deliverable wire_id: {deliv['wire_id']} | rerun wire_id: {rerun['wire_id']}")
if rerun["binary_sha256"] != deliv["binary_sha256"]:
    ok = False
    lines.append("FAIL: binary sha256 differs")
if rerun["wire_id"] != deliv["wire_id"]:
    ok = False
    lines.append("FAIL: wire id differs (fixture not deterministic)")
lines.append("")
lines.append(f"{'variant/cmd':<38}{'deliv med':>10}{'rerun med':>10}{'ratio':>8}{'deliv probes':>14}{'rerun probes':>14}  verdict")
for variant in deliv["variants"]:
    for cmd in deliv["variants"][variant]:
        d = deliv["variants"][variant][cmd]
        r = rerun["variants"][variant][cmd]
        dm, rm = median(d["raw_ms"]), median(r["raw_ms"])
        ratio = rm / dm
        probe_ok = r["probe_counts"] == d["probe_counts"] and len(set(r["probe_counts"])) == 1
        med_ok = abs(ratio - 1.0) <= 0.30
        exit_ok = r["exit_codes"] == [0]
        verdict = "PASS" if (probe_ok and med_ok and exit_ok) else "FAIL"
        if verdict == "FAIL":
            ok = False
        note = "" if med_ok else " [median drift >30%]"
        if not probe_ok:
            note += " [probe mismatch]"
        lines.append(f"{variant + '/' + cmd:<38}{dm:>10.3f}{rm:>10.3f}{ratio:>8.2f}"
                     f"{str(d['probe_counts']):>14}{str(r['probe_counts']):>14}  {verdict}{note}")
lines.append("")
lines.append("RESULT: " + ("PASS - probe counts identical, medians within +/-30%" if ok else "FAIL"))
text = "\n".join(lines)
print(text)
(research / "check-rerun.log").write_text(text + "\n", encoding="utf-8")
raise SystemExit(0 if ok else 1)
