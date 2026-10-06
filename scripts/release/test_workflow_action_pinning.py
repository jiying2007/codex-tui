"""Require immutable GitHub Action references in repository workflows."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
USE_RE = re.compile(
    r"^\s*(?:-\s*)?uses:\s+([^\s#]+)(?:\s+#\s*(.+?))?\s*$"
)
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")


class WorkflowActionPinning(unittest.TestCase):
    def test_external_actions_are_pinned_to_full_commit_sha(self):
        failures = []
        for path in sorted(WORKFLOWS.glob("*.yml")):
            for number, line in enumerate(
                path.read_text(encoding="utf-8").splitlines(),
                start=1,
            ):
                if "uses:" not in line:
                    continue
                match = USE_RE.match(line)
                if not match:
                    failures.append(
                        f"{path.relative_to(ROOT)}:{number}: unsupported uses syntax"
                    )
                    continue

                spec = match.group(1)
                comment = (match.group(2) or "").strip()
                if spec.startswith("./") or spec.startswith("docker://"):
                    continue
                if "@" not in spec:
                    failures.append(
                        f"{path.relative_to(ROOT)}:{number}: missing action ref: {spec}"
                    )
                    continue

                _, ref = spec.rsplit("@", 1)
                if not FULL_SHA.fullmatch(ref):
                    failures.append(
                        f"{path.relative_to(ROOT)}:{number}: external action must use "
                        f"a full commit SHA, got {spec}"
                    )
                if not comment:
                    failures.append(
                        f"{path.relative_to(ROOT)}:{number}: pinned action must retain "
                        "a human-readable tag/version comment for Dependabot review"
                    )

        self.assertEqual([], failures, "\n" + "\n".join(failures))


if __name__ == "__main__":
    unittest.main()
