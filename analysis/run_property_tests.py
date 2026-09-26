"""Runs the Rust test suites and records the counts the thesis quotes.

  cargo test                 default suite   (property tests print PROPERTY_SUMMARY lines)
  cargo test -- --ignored    the 4 tests that need real data or system tools
  market_sim test engine all the 37-case runtime checklist

Output: analysis/output/verification_tests.json
"""
from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

SRC = Path(__file__).resolve().parents[1] / "src"
OUT = Path(__file__).parent / "output"


def run(cmd):
    return subprocess.run(cmd, cwd=SRC, capture_output=True, text=True, timeout=3600)


def main():
    default = run(["cargo", "test", "--", "--nocapture"])
    text = default.stdout + default.stderr
    m = re.search(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", text)
    ignored = run(["cargo", "test", "--", "--ignored"])
    mi = re.search(r"test result: (\w+)\. (\d+) passed; (\d+) failed", ignored.stdout + ignored.stderr)
    checklist = run(["cargo", "run", "--quiet", "--", "test", "engine", "all"])
    ck = re.findall(r"RESULT: (\d+)/(\d+) passed", re.sub(r"\x1b\[[0-9;]*m", "", checklist.stdout))
    summaries = {}
    for line in text.splitlines():
        if line.startswith("PROPERTY_SUMMARY"):
            parts = line.split()
            summaries[parts[1]] = {k: int(v) for k, v in (p.split("=") for p in parts[2:])}
    out = {
        "cargo_default": {"status": m.group(1), "passed": int(m.group(2)), "failed": int(m.group(3)), "ignored": int(m.group(4))},
        "cargo_ignored": {"status": mi.group(1), "passed": int(mi.group(2)), "failed": int(mi.group(3))},
        "checklist": {"exit_code": checklist.returncode, "suites": [[int(a), int(b)] for a, b in ck]},
        "property_summaries": summaries,
    }
    (OUT / "verification_tests.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
