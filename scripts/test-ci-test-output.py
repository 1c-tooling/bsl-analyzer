#!/usr/bin/env python3
"""Check the CI test wrappers preserve failures and reject empty selections,
and that the Windows job routes every `cargo test` through one."""

from pathlib import Path
import re
import subprocess
import textwrap


workflow = Path(__file__).resolve().parents[1] / ".github/workflows/ci.yml"
wrappers = re.findall(
    r"^          run_(?:exact|nonempty)\(\) \{\n.*?^          \}",
    workflow.read_text(),
    re.MULTILINE | re.DOTALL,
)
assert wrappers, "no CI test wrappers found"
# Every wrapper must be checked: one whose spelling drifts from the pattern above
# would otherwise drop out of the check silently.
defined = re.findall(
    r"^\s*(?:function\s+)?run_\w+\s*\(\s*\)\s*\{", workflow.read_text(), re.MULTILINE
)
assert len(wrappers) == len(defined), f"checked {len(wrappers)} of {len(defined)} CI wrappers"
for block in wrappers:
    name = block.strip().split("(", 1)[0]
    for passed, status in [(1, 0), (0, 0), (1, 101)]:
        output = f"test result: ok. {passed} passed; diagnostic marker"
        result = subprocess.run(
            ["bash", "-c", "set -euo pipefail\n"
             + f"cargo() {{ echo '{output}' >&2; return {status}; }}\n"
             + textwrap.dedent(block) + f"\n{name} fixture test_name"],
            capture_output=True, text=True, check=False,
        )
        assert (result.returncode == 0) == (passed > 0 and status == 0), result
        assert output in result.stdout, result
# The windows-mcp job is the only place the code runs under Windows, and its coverage
# is a hand-written list of names. A bare `cargo test` there can select nothing and
# stay green; every invocation in the job must go through a checked wrapper.
job = re.search(
    r"^  windows-mcp:$\n.*?(?=^  [a-zA-Z0-9_-]+:$\n|\Z)",
    workflow.read_text(),
    re.MULTILINE | re.DOTALL,
)
assert job, "no windows-mcp job in the workflow"
unwrapped = job.group(0)
for block in wrappers:
    unwrapped = unwrapped.replace(block, "")
bare_test = re.search(r'cargo\s+["\']?\s*test\b', unwrapped)
assert not bare_test, "windows-mcp runs cargo test outside a wrapper"
print(f"PASS: {len(wrappers)} CI wrappers, success/empty/failure output; windows-mcp wraps every cargo test")
