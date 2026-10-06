import subprocess
import unittest
from unittest import mock

import package_release as p


RUSTC = """rustc 1.98.0 (bbbbbbbbb 2026-09-17)
binary: rustc
commit-hash: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
commit-date: 2026-09-17
host: x86_64-unknown-linux-gnu
release: 1.98.0
LLVM version: 21.1.0
"""

CARGO = """cargo 1.98.0 (ccccccccc 2026-09-10)
release: 1.98.0
commit-hash: cccccccccccccccccccccccccccccccccccccccc
commit-date: 2026-09-10
host: x86_64-unknown-linux-gnu
libgit2: 1.9.1
"""


def completed(stdout):
    return subprocess.CompletedProcess(
        args=["tool"],
        returncode=0,
        stdout=stdout,
        stderr="",
    )


class PackageToolchainProvenance(unittest.TestCase):
    def test_build_toolchain_identity_retains_exact_tools(self):
        with mock.patch.object(
            p.subprocess,
            "run",
            side_effect=[completed(RUSTC), completed(CARGO)],
        ):
            triple, toolchain = p.build_toolchain_identity()

        self.assertEqual(triple, "x86_64-unknown-linux-gnu")
        self.assertEqual(toolchain["rustc"]["release"], "1.98.0")
        self.assertEqual(toolchain["rustc"]["commitHash"], "b" * 40)
        self.assertEqual(toolchain["rustc"]["llvmVersion"], "21.1.0")
        self.assertEqual(toolchain["cargo"]["release"], "1.98.0")
        self.assertEqual(toolchain["cargo"]["commitHash"], "c" * 40)
        self.assertEqual(toolchain["cargo"]["host"], triple)

    def test_verbose_identity_requires_provenance_fields(self):
        broken = CARGO.replace(
            "commit-hash: cccccccccccccccccccccccccccccccccccccccc\n",
            "",
        )
        with mock.patch.object(p.subprocess, "run", return_value=completed(broken)):
            with self.assertRaisesRegex(RuntimeError, "missing fields: commit-hash"):
                p.verbose_tool_identity(["cargo", "-Vv"], "cargo")

    def test_rustc_and_cargo_hosts_must_match(self):
        foreign = CARGO.replace(
            "host: x86_64-unknown-linux-gnu",
            "host: x86_64-pc-windows-msvc",
        )
        with mock.patch.object(
            p.subprocess,
            "run",
            side_effect=[completed(RUSTC), completed(foreign)],
        ):
            with self.assertRaisesRegex(RuntimeError, "rustc/cargo host mismatch"):
                p.build_toolchain_identity()


if __name__ == "__main__":
    unittest.main()
