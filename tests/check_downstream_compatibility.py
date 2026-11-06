#!/usr/bin/env python3
"""Build tested source snapshots and run the pinned SDK compatibility suite."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tomllib
from datetime import datetime, timezone

ROOT = pathlib.Path(__file__).resolve().parents[1]
BASELINE = ROOT / "tests/downstream_compatibility_baseline.json"
TESTS = (
    ("llmsdk-core", "mod", "remote_engine_parts"),
    ("llmsdk-provider-openai", "mod", "test_call_retry_classifies_rate_limit"),
    ("llmsdk-provider-openai", "mod", "test_quota_and_unclassified_429_abort_without_retry"),
    ("llmsdk-provider-openai", "mod", "openai_chat_engine_tests"),
    ("llmsdk-provider-openai", "mod", "openai_responses_engine_tests"),
    ("llmsdk-provider-openai", "mod", "sse_content_type_contract"),
)
EXCLUDED = {".git", "target", ".idea", ".vscode", "__pycache__"}


class CompatibilityError(Exception):
    """An input or compatibility invariant was not satisfied."""


def git(repo: pathlib.Path, *args: str) -> str:
    result = subprocess.run(["git", "-C", str(repo), *args], check=True, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return result.stdout.strip()


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_digest(root: pathlib.Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        if any(part in EXCLUDED for part in path.relative_to(root).parts):
            continue
        if path.is_file() and not path.is_symlink():
            digest.update(path.relative_to(root).as_posix().encode())
            digest.update(bytes.fromhex(sha256_file(path)))
    return digest.hexdigest()


def rewrite_http_dependency(source: str, version: str, path: str) -> tuple[str, int]:
    """Rewrite one `qubit-http` dependency while preserving its other options."""
    try:
        parsed = tomllib.loads(source)
    except tomllib.TOMLDecodeError as error:
        raise CompatibilityError(f"invalid TOML: {error}") from error
    matches = []
    for table_name in ("dependencies", "dev-dependencies", "build-dependencies"):
        table = parsed.get(table_name, {})
        if isinstance(table, dict) and "qubit-http" in table:
            matches.append((table_name, table["qubit-http"]))
    if len(matches) != 1:
        raise CompatibilityError(f"expected exactly one qubit-http dependency, found {len(matches)}")
    table_name, dependency = matches[0]
    if isinstance(dependency, str):
        value = {"version": version, "path": path}
    elif isinstance(dependency, dict):
        value = dict(dependency)
        value["version"] = version
        value["path"] = path
    else:
        raise CompatibilityError("unsupported qubit-http dependency syntax")
    def toml_value(item: object) -> str:
        if isinstance(item, bool):
            return "true" if item else "false"
        if isinstance(item, str):
            return json.dumps(item)
        if isinstance(item, list) and all(isinstance(part, str) for part in item):
            return "[" + ", ".join(json.dumps(part) for part in item) + "]"
        if isinstance(item, (int, float)):
            return str(item)
        raise CompatibilityError(f"unsupported TOML dependency option: {item!r}")
    encoded = "{ " + ", ".join(f"{key} = {toml_value(item)}" for key, item in value.items()) + " }"
    # Only alter the dependency declaration line and validate the complete result.
    pattern = re.compile(rf"^(\s*qubit-http\s*=\s*)(.+?)\s*$", re.MULTILINE)
    rewritten, count = pattern.subn(lambda match: match.group(1) + encoded, source)
    if count != 1:
        raise CompatibilityError(f"expected exactly one textual qubit-http entry, found {count}")
    try:
        rewritten_table = tomllib.loads(rewritten).get(table_name, {}).get("qubit-http")
    except tomllib.TOMLDecodeError as error:
        raise CompatibilityError(f"rewritten TOML is invalid: {error}") from error
    if rewritten_table != value:
        raise CompatibilityError("rewritten dependency did not preserve its options")
    return rewritten, count


def prepare_output(output: pathlib.Path) -> None:
    if output.exists():
        raise CompatibilityError(f"output directory already exists: {output}")
    output.mkdir(parents=True)


def snapshot_tree(source: pathlib.Path, destination: pathlib.Path) -> None:
    """Copy a repository's tracked and non-ignored files without following links."""
    destination.mkdir(parents=True, exist_ok=True)
    listing = subprocess.run(["git", "-C", str(source), "ls-files", "-co", "--exclude-standard", "-z"],
                             check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    for raw in listing.stdout.split(b"\0"):
        if not raw:
            continue
        rel = pathlib.PurePosixPath(os.fsdecode(raw))
        if any(part in EXCLUDED for part in rel.parts):
            continue
        src = source.joinpath(*rel.parts)
        dst = destination.joinpath(*rel.parts)
        if src.is_symlink():
            target = src.resolve(strict=True)
            try:
                target.relative_to(source.resolve())
            except ValueError as error:
                raise CompatibilityError(f"external symlink in source snapshot: {src}") from error
            raise CompatibilityError(f"symlink inputs are not supported in source snapshot: {src}")
        if src.is_dir():
            dst.mkdir(parents=True, exist_ok=True)
        elif src.is_file():
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)


def validate_repositories(source_root: pathlib.Path, baseline_path: pathlib.Path, candidate: bool) -> list[dict]:
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
    if baseline.get("schema_version") != 1 or not isinstance(baseline.get("repositories"), list):
        raise CompatibilityError("invalid baseline schema")
    seen_ids, seen_paths = set(), set()
    records = []
    for item in baseline["repositories"]:
        repo_id, relative, expected = item.get("repo_id"), item.get("relative_path"), item.get("commit")
        if not repo_id or repo_id in seen_ids or not relative or relative in seen_paths or not re.fullmatch(r"[0-9a-f]{40}", expected or ""):
            raise CompatibilityError("invalid or duplicate baseline repository entry")
        seen_ids.add(repo_id); seen_paths.add(relative)
        repo = (source_root / relative).resolve()
        try:
            repo.relative_to(source_root.resolve())
        except ValueError as error:
            raise CompatibilityError(f"repository path escapes source root: {relative}") from error
        if not repo.is_dir():
            raise CompatibilityError(f"missing repository: {relative}")
        head = git(repo, "rev-parse", "HEAD")
        status = git(repo, "status", "--porcelain", "--untracked-files=all")
        dirty = bool(status)
        if head != expected:
            raise CompatibilityError(f"baseline drift for {repo_id}: expected {expected}, found {head}")
        if dirty and repo_id in {"llmsdk-core", "llmsdk-provider-openai", "llmsdk-agent"} and not candidate:
            raise CompatibilityError(f"dirty downstream repository {repo_id}; use --candidate-source to record this input")
        records.append({"repo_id": repo_id, "relative_path": relative, "commit": head,
                        "dirty": dirty, "tree_sha256": tree_digest(repo)})
    return records


def assert_single_http_package(metadata: dict, root_id: str, expected_manifest: pathlib.Path, version: str) -> dict:
    packages = {item["id"]: item for item in metadata.get("packages", []) if item.get("name") == "qubit-http"}
    nodes = {item["id"]: item for item in (metadata.get("resolve") or {}).get("nodes", [])}
    reachable, pending = set(), [root_id]
    while pending:
        current = pending.pop()
        if current in reachable:
            continue
        reachable.add(current)
        pending.extend(dep["pkg"] for dep in nodes.get(current, {}).get("deps", []))
    found = [packages[package_id] for package_id in reachable if package_id in packages]
    if len(found) != 1:
        raise CompatibilityError(f"expected exactly one resolved qubit-http package, found {len(found)}")
    package = found[0]
    if package["version"] != version or pathlib.Path(package["manifest_path"]).resolve() != expected_manifest.resolve():
        raise CompatibilityError("resolved qubit-http package does not point to the tested snapshot")
    return {"id": package["id"], "manifest_path": package["manifest_path"], "version": package["version"]}


def test_count(output: str) -> int:
    matches = re.findall(r"^running (\d+) tests?$", output, re.MULTILINE)
    if not matches:
        raise CompatibilityError("could not identify test count from cargo output")
    count = sum(map(int, matches))
    if count == 0:
        raise CompatibilityError("cargo command ran zero tests")
    return count


def command_record(argv: list[str], exit_code: int, log_path: str, output: str) -> dict:
    try:
        count = test_count(output)
    except CompatibilityError:
        count = None
    return {"argv": argv, "exit_code": exit_code, "tests_run": count, "log_path": log_path}


def run_cargo(argv: list[str], cwd: pathlib.Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(argv, cwd=cwd, env=env, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT)


def command_output(process: subprocess.CompletedProcess) -> str:
    """Return captured output for merged or separate stdout/stderr streams."""
    return (process.stdout or "") + (process.stderr or "")


def run(args: argparse.Namespace) -> int:
    output = args.output_dir.resolve()
    result = {"schema_version": 1, "started_at": datetime.now(timezone.utc).isoformat(),
              "status": "failed", "commands": [], "repositories": [], "errors": []}
    output_created = False
    try:
        source_root = args.source_root.resolve(strict=True)
        records = validate_repositories(source_root, args.baseline.resolve(), args.candidate_source)
        result["repositories"] = records
        prepare_output(output)
        output_created = True
        snapshot_root = output / "source"
        for item in records:
            snapshot_tree(source_root / item["relative_path"], snapshot_root / item["relative_path"])
        http_snapshot = output / "http"
        snapshot_tree(ROOT, http_snapshot)
        http_head = git(ROOT, "rev-parse", "HEAD")
        http_dirty = bool(git(ROOT, "status", "--porcelain", "--untracked-files=all"))
        http_manifest = tomllib.loads((http_snapshot / "Cargo.toml").read_text(encoding="utf-8"))["package"]
        version = http_manifest["version"]
        for repo_id in ("llmsdk-core", "llmsdk-provider-openai"):
            record = next(row for row in records if row["repo_id"] == repo_id)
            manifest = snapshot_root / record["relative_path"] / "Cargo.toml"
            original = manifest.read_text(encoding="utf-8")
            path = os.path.relpath(http_snapshot, manifest.parent)
            rewritten, _ = rewrite_http_dependency(original, version, path)
            manifest.write_text(rewritten, encoding="utf-8")
        target = output / "target"
        package_info = {}
        for repo_id in ("llmsdk-core", "llmsdk-provider-openai"):
            record = next(row for row in records if row["repo_id"] == repo_id)
            manifest = snapshot_root / record["relative_path"] / "Cargo.toml"
            metadata_argv = ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(manifest)]
            meta = run_cargo(metadata_argv, cwd=manifest.parent)
            log = output / f"{repo_id}-metadata.log"
            log.write_text(command_output(meta), encoding="utf-8")
            result["commands"].append({"argv": metadata_argv, "exit_code": meta.returncode,
                                       "tests_run": None, "log_path": log.name})
            if meta.returncode:
                raise CompatibilityError(f"cargo metadata failed for {repo_id}; see {log}")
            metadata = json.loads(meta.stdout)
            root_id = next(p["id"] for p in metadata["packages"] if pathlib.Path(p["manifest_path"]).resolve() == manifest.resolve())
            package_info[repo_id] = assert_single_http_package(metadata, root_id, http_snapshot / "Cargo.toml", version)
            filters = [filter_name for rid, _, filter_name in TESTS if rid == repo_id]
            for index, filter_name in enumerate(filters, 1):
                argv = ["cargo", "test", "--manifest-path", str(manifest), "--test", "mod", filter_name]
                env = dict(os.environ, CARGO_TARGET_DIR=str(target))
                proc = run_cargo(argv, cwd=manifest.parent, env=env)
                log = output / f"{repo_id}-test-{index}.log"
                log.write_text(proc.stdout, encoding="utf-8")
                result["commands"].append(command_record(argv, proc.returncode, log.name, proc.stdout))
                if proc.returncode:
                    raise CompatibilityError(f"cargo test failed for {repo_id}/{filter_name}; see {log}")
        result["http_packages"] = package_info
        result["http_source_sha256"] = tree_digest(http_snapshot)
        result["http_source"] = {"commit": http_head, "dirty": http_dirty,
                                  "snapshot_type": "working-tree" if http_dirty else "commit"}
        cargo_version = subprocess.run(["cargo", "--version"], text=True, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, check=True).stdout.strip()
        rustc_version = subprocess.run(["rustc", "--version"], text=True, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, check=True).stdout.strip()
        result["toolchain"] = {"cargo": cargo_version, "rustc": rustc_version}
        result["lockfile_sha256"] = {
            str(path.relative_to(output)): sha256_file(path)
            for path in sorted(snapshot_root.rglob("Cargo.lock")) if path.is_file()
        }
        result["status"] = "passed"
    except Exception as error:
        result["errors"].append(str(error))
        print(f"compatibility check failed: {error}", file=sys.stderr)
    finally:
        if output_created:
            (output / "result.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return 0 if result["status"] == "passed" else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=pathlib.Path, required=True)
    parser.add_argument("--output-dir", type=pathlib.Path, required=True)
    parser.add_argument("--baseline", type=pathlib.Path, default=BASELINE)
    parser.add_argument("--candidate-source", action="store_true", help="allow dirty baseline trees and record their hashes")
    return run(parser.parse_args())


if __name__ == "__main__":
    sys.exit(main())
