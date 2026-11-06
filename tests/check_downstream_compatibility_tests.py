"""Behavior tests for the offline downstream compatibility runner."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import subprocess
import tempfile
import types
import unittest
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
    def test_input_digest_matches_snapshot_file_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = pathlib.Path(directory) / "repo"
            repo.mkdir()
            (repo / ".gitignore").write_text("ignored.txt\n", encoding="utf-8")
            (repo / "Cargo.toml").write_text("[package]\nname='sample'\n", encoding="utf-8")
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "add", ".gitignore", "Cargo.toml"], check=True)
            subprocess.run(["git", "-C", str(repo), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "baseline"], check=True)
            initial = runner.tree_digest(repo)
            (repo / "ignored.txt").write_text("ignored input\n", encoding="utf-8")
            self.assertEqual(runner.tree_digest(repo), initial)
            (repo / "candidate.rs").write_text("fn candidate() {}\n", encoding="utf-8")
            self.assertNotEqual(runner.tree_digest(repo), initial)

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

    def test_candidate_accepts_dirty_sdk_at_baseline_and_records_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo = root / "sdk"
            repo.mkdir()
            (repo / "Cargo.toml").write_text("[package]\nname='sdk'\n", encoding="utf-8")
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "add", "Cargo.toml"], check=True)
            subprocess.run(["git", "-C", str(repo), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "baseline"], check=True)
            head = runner.git(repo, "rev-parse", "HEAD")
            baseline = root / "baseline.json"
            baseline.write_text(json.dumps({"schema_version": 1, "repositories": [
                {"repo_id": "llmsdk-core", "relative_path": "sdk", "commit": head}
            ]}), encoding="utf-8")
            clean_record = runner.validate_repositories(root, baseline, True)[0]
            self.assertFalse(clean_record["dirty"])
            self.assertEqual(clean_record["tracked_diff_sha256"], runner.hashlib.sha256(b"").hexdigest())
            (repo / "Cargo.toml").write_text("[package]\nname='sdk-candidate'\n", encoding="utf-8")
            record = runner.validate_repositories(root, baseline, True)[0]
            self.assertTrue(record["dirty"])
            self.assertEqual(record["commit"], head)
            self.assertEqual(len(record["tree_sha256"]), 64)
            self.assertEqual(len(record["tracked_diff_sha256"]), 64)
            self.assertNotEqual(record["tracked_diff_sha256"], clean_record["tracked_diff_sha256"])

    def test_records_dirty_local_path_dependency(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "repo").mkdir()
            baseline = root / "baseline.json"
            baseline.write_text(json.dumps({"schema_version": 1, "repositories": [{"repo_id": "qubit-config", "relative_path": "repo", "commit": "a" * 40}]}))
            with mock.patch.object(runner, "git", side_effect=["a" * 40, " M src/lib.rs"]), mock.patch.object(runner, "tracked_diff_digest", return_value="d" * 64):
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

    def test_rejects_output_equal_to_inside_or_ancestor_of_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            source = pathlib.Path(directory) / "source"
            child = source / "nested-output"
            ancestor = pathlib.Path(directory)
            source.mkdir()
            for output in (source, child, ancestor):
                with self.subTest(output=output), self.assertRaisesRegex(runner.CompatibilityError, "overlaps"):
                    runner.ensure_output_disjoint(output, [source])

    def test_rejects_path_package_outside_snapshot(self):
        metadata = {"packages": [
            {"name": "sdk", "source": None, "manifest_path": "/snapshot/sdk/Cargo.toml"},
            {"name": "local-dependency", "source": None, "manifest_path": "/original/repo/Cargo.toml"},
            {"name": "registry-dependency", "source": "registry+https://example.invalid", "manifest_path": "/cargo/registry/pkg/Cargo.toml"},
        ]}
        with self.assertRaisesRegex(runner.CompatibilityError, "path dependency escapes"):
            runner.assert_path_packages_within(metadata, pathlib.Path("/snapshot"))


class RunnerIntegrationTests(unittest.TestCase):
    def make_repo(self, path: pathlib.Path, manifest: str) -> str:
        path.mkdir(parents=True)
        (path / "Cargo.toml").write_text(manifest, encoding="utf-8")
        subprocess.run(["git", "init", "-q", str(path)], check=True)
        subprocess.run(["git", "-C", str(path), "add", "Cargo.toml"], check=True)
        subprocess.run(["git", "-C", str(path), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "baseline"], check=True)
        return runner.git(path, "rev-parse", "HEAD")

    def run_with_cargo_output(self, test_output: str, mutate_source_manifest: bool = False) -> dict:
        with tempfile.TemporaryDirectory() as directory:
            temp = pathlib.Path(directory)
            source_root = temp / "qubit"
            repo_specs = [
                ("llmsdk-core", "rust-llmsdk/rs-llmsdk-core", '[package]\nname="llmsdk-core"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nqubit-http="^0.15"\n'),
                ("llmsdk-provider-openai", "rust-llmsdk/rs-llmsdk-provider-openai", '[package]\nname="llmsdk-provider-openai"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nqubit-http="^0.15"\n'),
                ("llmsdk-agent", "rust-llmsdk/rs-llmsdk-agent", '[package]\nname="llmsdk-agent"\nversion="0.1.0"\nedition="2021"\n'),
                ("qubit-config", "rust-common/rs-config", '[package]\nname="qubit-config"\nversion="0.15.0"\nedition="2021"\n'),
                ("qubit-json", "rust-common/rs-json", '[package]\nname="qubit-json"\nversion="0.12.0"\nedition="2021"\n'),
                ("qubit-redact", "rust-common/rs-redact", '[package]\nname="qubit-redact"\nversion="0.10.0"\nedition="2021"\n'),
                ("qubit-value", "rust-common/rs-value", '[package]\nname="qubit-value"\nversion="0.13.0"\nedition="2021"\n'),
            ]
            baseline = {"schema_version": 1, "repositories": []}
            for repo_id, relative, manifest in repo_specs:
                head = self.make_repo(source_root / relative, manifest)
                baseline["repositories"].append({"repo_id": repo_id, "relative_path": relative, "commit": head})
            http_repo = temp / "http"
            self.make_repo(http_repo, '[package]\nname="qubit-http"\nversion="0.16.0"\nedition="2021"\n')
            baseline_path = temp / "baseline.json"
            baseline_path.write_text(json.dumps(baseline), encoding="utf-8")
            output = temp / "result"
            args = types.SimpleNamespace(source_root=source_root, output_dir=output,
                                         baseline=baseline_path, candidate_source=False)
            real_run = runner.subprocess.run

            def fake_cargo(argv, **kwargs):
                if argv[0] != "cargo":
                    return real_run(argv, **kwargs)
                if argv[1] == "metadata":
                    sdk_manifest = pathlib.Path(argv[-1]).resolve()
                    http_manifest = (output / "source/rust-common/rs-http/Cargo.toml").resolve()
                    metadata = {"packages": [
                        {"id": "sdk", "name": "sdk", "version": "0.1.0", "source": None, "manifest_path": str(sdk_manifest)},
                        {"id": "http", "name": "qubit-http", "version": "0.16.0", "source": None, "manifest_path": str(http_manifest)},
                    ], "resolve": {"nodes": [{"id": "sdk", "deps": [{"pkg": "http"}]}, {"id": "http", "deps": []}]}}
                    return subprocess.CompletedProcess(argv, 0, stdout=json.dumps(metadata), stderr=None)
                if mutate_source_manifest:
                    source_manifest = source_root / "rust-llmsdk/rs-llmsdk-core/Cargo.toml"
                    source_manifest.write_text(source_manifest.read_text(encoding="utf-8") + "# unexpected source mutation\n", encoding="utf-8")
                return subprocess.CompletedProcess(argv, 0, stdout=test_output, stderr=None)

            with mock.patch.object(runner, "ROOT", http_repo), mock.patch.object(runner.subprocess, "run", side_effect=fake_cargo):
                exit_code = runner.run(args)
            self.assertEqual(exit_code, 1)
            self.assertTrue((output / "result.json").is_file())
            return json.loads((output / "result.json").read_text(encoding="utf-8"))

    def test_successful_cargo_with_zero_tests_is_rejected(self):
        result = self.run_with_cargo_output("running 0 tests\n")
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["commands"][-1]["exit_code"], 0)
        self.assertEqual(result["commands"][-1]["tests_run"], 0)
        self.assertIn("zero or an unrecognized number", result["errors"][-1])
        self.assertEqual(result["source_manifest_lock_sha256_before"], result["source_manifest_lock_sha256_after"])

    def test_successful_cargo_with_unrecognized_test_count_is_rejected(self):
        result = self.run_with_cargo_output("Finished `test` profile\n")
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["commands"][-1]["exit_code"], 0)
        self.assertIsNone(result["commands"][-1]["tests_run"])

    def test_source_manifest_mutation_is_detected(self):
        result = self.run_with_cargo_output("running 1 test\ntest test_case ... ok\n", mutate_source_manifest=True)
        self.assertEqual(result["status"], "failed")
        self.assertIn("source manifests or lockfiles changed", result["errors"][-1])
        self.assertNotEqual(result["source_manifest_lock_sha256_before"], result["source_manifest_lock_sha256_after"])


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
