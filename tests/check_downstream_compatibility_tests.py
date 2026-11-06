"""Behavior tests for the offline downstream compatibility runner."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest
import subprocess
from unittest import mock

SCRIPT = pathlib.Path(__file__).with_name("check_downstream_compatibility.py")
SPEC = importlib.util.spec_from_file_location("downstream_compat", SCRIPT)
runner = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(runner)


class RewriteTests(unittest.TestCase):
    def test_rewrite_preserves_dependency_options(self):
        source = '[dependencies]\nqubit-http = { version = "^0.15", default-features = false, features = ["sse"] }\n'
        rewritten, count = runner.rewrite_http_dependency(source, "0.16.0", "../http")
        self.assertEqual(count, 1)
        dependency = runner.tomllib.loads(rewritten)["dependencies"]["qubit-http"]
        self.assertEqual(dependency, {"version": "0.16.0", "path": "../http", "default-features": False, "features": ["sse"]})

    def test_rejects_missing_replacement_target(self):
        with self.assertRaisesRegex(runner.CompatibilityError, "exactly one"):
            runner.rewrite_http_dependency('[dependencies]\nbytes = "1"\n', "0.16.0", "../http")

    def test_rejects_invalid_toml(self):
        with self.assertRaisesRegex(runner.CompatibilityError, "invalid TOML"):
            runner.rewrite_http_dependency('[dependencies\nqubit-http = "0.15"', "0.16.0", "../http")

    def test_rejects_duplicate_http_packages(self):
        metadata = {"packages": [
            {"name": "qubit-http", "version": "0.16.0", "id": "a", "manifest_path": "/snapshot/http/Cargo.toml"},
            {"name": "qubit-http", "version": "0.15.0", "id": "b", "manifest_path": "/registry/http/Cargo.toml"},
        ], "resolve": {"nodes": [{"id": "sdk", "deps": [{"pkg": "a"}, {"pkg": "b"}]}]}}
        with self.assertRaisesRegex(runner.CompatibilityError, "exactly one"):
            runner.assert_single_http_package(metadata, "sdk", pathlib.Path("/snapshot/http/Cargo.toml"), "0.16.0")

    def test_rejects_original_tree_paths(self):
        metadata = {"packages": [
            {"name": "qubit-http", "version": "0.16.0", "id": "a", "manifest_path": "/original/rs-http/Cargo.toml"}
        ], "resolve": {"nodes": [{"id": "sdk", "deps": [{"pkg": "a"}]}]}}
        with self.assertRaisesRegex(runner.CompatibilityError, "snapshot"):
            runner.assert_single_http_package(metadata, "sdk", pathlib.Path("/snapshot/http/Cargo.toml"), "0.16.0")


class InputSafetyTests(unittest.TestCase):
    def test_rejects_baseline_drift(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "repo").mkdir()
            config = {"schema_version": 1, "repositories": [{"repo_id": "repo", "relative_path": "repo", "commit": "a" * 40}]}
            (root / "baseline.json").write_text(json.dumps(config))
            with mock.patch.object(runner, "git", side_effect=["b" * 40, ""]):
                with self.assertRaisesRegex(runner.CompatibilityError, "baseline"):
                    runner.validate_repositories(root, root / "baseline.json", False)

    def test_rejects_dirty_sdk_tree_without_candidate_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "sdk").mkdir()
            config = {"schema_version": 1, "repositories": [{"repo_id": "llmsdk-core", "relative_path": "sdk", "commit": "a" * 40}]}
            baseline = root / "baseline.json"
            baseline.write_text(json.dumps(config))
            with mock.patch.object(runner, "git", side_effect=["a" * 40, " M Cargo.toml"]):
                with self.assertRaisesRegex(runner.CompatibilityError, "dirty downstream"):
                    runner.validate_repositories(root, baseline, False)

    def test_records_dirty_local_path_dependency(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "repo").mkdir()
            baseline = root / "baseline.json"
            baseline.write_text(json.dumps({"schema_version": 1, "repositories": [{"repo_id": "qubit-config", "relative_path": "repo", "commit": "a" * 40}]}))
            with mock.patch.object(runner, "git", side_effect=["a" * 40, " M src/lib.rs"]):
                record = runner.validate_repositories(root, baseline, False)[0]
            self.assertTrue(record["dirty"])
            self.assertEqual(record["commit"], "a" * 40)

    def test_rejects_external_symlinks(self):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            source = pathlib.Path(directory) / "source"
            source.mkdir()
            (source / "escape").symlink_to(pathlib.Path(outside), target_is_directory=True)
            __import__("subprocess").run(["git", "init", "-q", str(source)], check=True)
            with self.assertRaisesRegex(runner.CompatibilityError, "symlink"):
                runner.snapshot_tree(source, pathlib.Path(directory) / "snapshot")

    def test_rejects_existing_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory) / "out"
            output.mkdir()
            with self.assertRaisesRegex(runner.CompatibilityError, "already exists"):
                runner.prepare_output(output)


class ResultTests(unittest.TestCase):
    def test_successful_metadata_result_with_merged_stream_logs_stdout(self):
        process = subprocess.CompletedProcess(["cargo", "metadata"], 0, stdout='{"packages": []}', stderr=None)
        self.assertEqual(runner.command_output(process), '{"packages": []}')

    def test_rejects_zero_tests(self):
        with self.assertRaisesRegex(runner.CompatibilityError, "zero tests"):
            runner.test_count("running 0 tests")

    def test_rejects_unrecognized_test_count(self):
        with self.assertRaisesRegex(runner.CompatibilityError, "could not identify"):
            runner.test_count("Finished test profile")

    def test_records_failed_command(self):
        with mock.patch.object(runner.subprocess, "run", return_value=subprocess.CompletedProcess(
                ["cargo", "test"], 101, stdout="running 3 tests\ntest a ... FAILED")) as cargo:
            process = runner.run_cargo(["cargo", "test"], pathlib.Path("/snapshot"))
        cargo.assert_called_once()
        record = runner.command_record(["cargo", "test"], process.returncode, "cargo-test.log", process.stdout)
        self.assertEqual(record["exit_code"], 101)
        self.assertEqual(record["tests_run"], 3)
        self.assertEqual(record["log_path"], "cargo-test.log")
