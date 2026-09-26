"""Integration tests for Lean's frontend-to-LCNF worker boundary."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "worker"
WORKER_BIN = WORKER / ".lake/build/bin/lean2rust-worker"
FIXTURES = ROOT / "compiler-tests/fixtures"


def run_worker(
    fixture: str,
    module: str,
    output: Path,
    lean_git_hash_override: str | None = None,
) -> subprocess.CompletedProcess[str]:
    project = FIXTURES / fixture
    env = os.environ.copy()
    command = ["lake", "env", str(WORKER_BIN), module, str(output)]
    if lean_git_hash_override is not None:
        env["L2R_FORCE_HASH"] = lean_git_hash_override
        command = [
            "lake", "env", "sh", "-c",
            'LEAN_GITHASH="$L2R_FORCE_HASH"; export LEAN_GITHASH; '
            'exec "$L2R_WORKER" "$L2R_MODULE" "$L2R_OUTPUT"',
        ]
        env.update(
            L2R_WORKER=str(WORKER_BIN), L2R_MODULE=module, L2R_OUTPUT=str(output)
        )
    return subprocess.run(
        command,
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


def parse_frame(path: Path) -> dict:
    data = path.read_bytes()
    if len(data) < 8 or data[:4] != b"L2RB":
        raise AssertionError("invalid BIR frame magic")
    (length,) = struct.unpack_from("<I", data, 4)
    if length != len(data) - 8:
        raise AssertionError("invalid BIR frame length")
    payload = json.loads(data[8:])
    if payload["protocolVersion"] != 1 or payload["birVersion"] != 1:
        raise AssertionError("unexpected protocol or BIR version")
    return payload


class WorkerIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(["lake", "build"], cwd=WORKER, check=True)
        for fixture, module in (("simple", "Simple"), ("legacy", "Legacy"), ("invalid", "Invalid")):
            subprocess.run(
                ["lake", "build", f"+{module}:deps"],
                cwd=FIXTURES / fixture,
                check=True,
            )

    def test_new_module_captures_extern_and_erases_proof(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            first = Path(temp) / "first.bir"
            second = Path(temp) / "second.bir"
            result = run_worker("simple", "Simple", first)
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = parse_frame(first)
            self.assertEqual(payload["module"], "Simple")
            declarations = {item["name"]: item for item in payload["declarations"]}
            self.assertIn("twice", declarations)
            self.assertIn("twice._boxed", declarations)
            self.assertNotIn("twice_zero", declarations)
            self.assertEqual(declarations["providerSend"]["value"]["op"], "extern")
            entries = declarations["providerSend"]["value"]["entries"]
            self.assertTrue(any(item.get("symbol") == "provider_send" for item in entries))
            self.assertEqual(
                first.read_bytes(),
                (ROOT / "compiler-tests/golden/simple.bir").read_bytes(),
            )
            self.assertEqual(run_worker("simple", "Simple", second).returncode, 0)
            self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_legacy_module_uses_same_compiler_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "legacy.bir"
            result = run_worker("legacy", "Legacy", output)
            self.assertEqual(result.returncode, 0, result.stderr)
            names = {item["name"] for item in parse_frame(output)["declarations"]}
            self.assertIn("triple", names)
            self.assertNotIn("triple_zero", names)

    def test_invalid_proof_fails_without_publishing_output(self) -> None:
        source = FIXTURES / "invalid/Invalid.lean"
        before = hashlib.sha256(source.read_bytes()).digest()
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "invalid.bir"
            output.write_bytes(b"stale artifact")
            result = run_worker("invalid", "Invalid", output)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Invalid.lean:5:", result.stderr)
            self.assertIn("is false", result.stderr)
            self.assertFalse(output.exists())
            self.assertFalse(output.with_suffix(".bir.partial").exists())
        self.assertEqual(hashlib.sha256(source.read_bytes()).digest(), before)

    def test_toolchain_mismatch_fails_before_loading_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "mismatch.bir"
            result = run_worker("simple", "Simple", output, lean_git_hash_override="wrong")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires Lean commit", result.stderr)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
