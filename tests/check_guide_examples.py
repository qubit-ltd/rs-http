#!/usr/bin/env python3
"""Compile Rust examples marked in the English and Chinese user guides.

Requires Python 3.11+ for the standard-library ``tomllib`` module.
"""

from __future__ import annotations

import pathlib
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
import unicodedata
import urllib.parse
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
GUIDES = {
    "en": ROOT / "doc/user_guide.en.md",
    "zh_CN": ROOT / "doc/user_guide.zh_CN.md",
}
QUICK_STARTS = {
    "quick-start-en": ROOT / "doc/user_guide.en.md",
    "quick-start-zh_CN": ROOT / "doc/user_guide.zh_CN.md",
}
READMES = {
    "en": ROOT / "README.md",
    "zh_CN": ROOT / "README.zh_CN.md",
}
SCENARIOS = ("configuration", "async-upload", "redaction", "sse-reconnect", "token-refresh", "tls-roots", "url-join")
FENCE = re.compile(r"```rust\s*\n(.*?)\n```", re.DOTALL)
DEPENDENCIES_START = "<!-- guide-dependencies:start -->"
DEPENDENCIES_END = "<!-- guide-dependencies:end -->"
TOML_FENCE = re.compile(r"```toml\s*\n(.*?)\n```", re.DOTALL)
TOKEN_REFRESH_DECLARATION = "async fn refresh_access_token() -> HttpResult<String> {"
MARKDOWN_LINK = re.compile(r"!?\[[^\]]*\]\((<[^>]+>|[^)]*)\)")
HTML_ID = re.compile(r"<(?:a|span)\b[^>]*\b(?:id|name)\s*=\s*['\"]([^'\"]+)['\"]", re.IGNORECASE)


def markdown_lines_outside_fences(markdown: str):
    """Yield (line number, source) pairs outside CommonMark fenced blocks."""
    fence_character = None
    fence_length = 0
    for line_number, line in enumerate(markdown.splitlines(), start=1):
        if fence_character is None:
            opener = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
            if opener:
                markers, info = opener.groups()
                if markers[0] != "`" or "`" not in info:
                    fence_character = markers[0]
                    fence_length = len(markers)
                    continue
            yield line_number, line
            continue

        closer = re.match(r"^ {0,3}(`+|~+)([ \t]*)$", line)
        if (
            closer
            and closer.group(1)[0] == fence_character
            and len(closer.group(1)) >= fence_length
        ):
            fence_character = None
            fence_length = 0


def markdown_anchors(markdown: str) -> set[str]:
    """Collect GitHub-style anchors generated from ATX headings and HTML IDs."""
    anchors = set()
    counts: dict[str, int] = {}
    for _, line in markdown_lines_outside_fences(markdown):
        anchors.update(HTML_ID.findall(line))
        heading = re.match(r"^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$", line)
        if not heading:
            continue
        title = re.sub(r"[`*_~]", "", heading.group(1))
        title = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", title)
        slug = "".join(
            "-" if char.isspace() else char
            for char in title.lower()
            if not unicodedata.category(char).startswith("P") or char == "-"
        )
        slug = re.sub(r"-+", "-", slug).strip("-")
        occurrence = counts.get(slug, 0)
        counts[slug] = occurrence + 1
        anchors.add(slug if occurrence == 0 else f"{slug}-{occurrence}")
    return anchors


def check_local_links(documents: dict[pathlib.Path, str]) -> list[str]:
    """Return broken local file and Markdown fragment links with source lines."""
    contents = {path.resolve(): markdown for path, markdown in documents.items()}
    errors = []
    for source, markdown in contents.items():
        for line_number, line in markdown_lines_outside_fences(markdown):
            for match in MARKDOWN_LINK.finditer(line):
                destination = match.group(1).strip()
                if destination.startswith("<") and destination.endswith(">"):
                    destination = destination[1:-1]
                destination = re.split(r"\s+['\"]", destination, maxsplit=1)[0]
                if destination.startswith("//"):
                    continue
                parsed = urllib.parse.urlsplit(destination)
                if parsed.scheme or parsed.netloc:
                    continue
                relative_path = urllib.parse.unquote(parsed.path)
                target = source if not relative_path else (source.parent / relative_path).resolve()
                if target not in contents and not target.is_file():
                    errors.append(f"{source}:{line_number}: missing local file for link {destination!r}")
                    continue
                if parsed.fragment and target.suffix.lower() == ".md":
                    target_markdown = contents.get(target)
                    if target_markdown is None:
                        try:
                            target_markdown = target.read_text(encoding="utf-8")
                        except OSError:
                            errors.append(f"{source}:{line_number}: cannot read Markdown target {destination!r}")
                            continue
                    fragment = urllib.parse.unquote(parsed.fragment)
                    if fragment not in markdown_anchors(target_markdown):
                        errors.append(f"{source}:{line_number}: missing heading anchor in link {destination!r}")
    return errors


def check_repository_local_links() -> None:
    """Check local links emitted by both READMEs and both user guides."""
    paths = [*READMES.values(), *GUIDES.values()]
    documents = {path: path.read_text(encoding="utf-8") for path in paths}
    errors = check_local_links(documents)
    if errors:
        raise ValueError("local links:\n" + "\n".join(errors))


def extract_dependencies(markdown: str) -> dict:
    """Read the dependency table from a uniquely marked TOML guide block."""
    if markdown.count(DEPENDENCIES_START) != 1:
        raise ValueError("dependencies: expected exactly one start marker")
    if markdown.count(DEPENDENCIES_END) != 1:
        raise ValueError("dependencies: expected exactly one end marker")
    start = markdown.index(DEPENDENCIES_START) + len(DEPENDENCIES_START)
    end = markdown.index(DEPENDENCIES_END)
    if start > end:
        raise ValueError("dependencies: start marker must precede end marker")
    section = markdown[start:end]
    fences = list(TOML_FENCE.finditer(section))
    if len(fences) != 1:
        raise ValueError("dependencies: expected exactly one TOML fence")
    source = fences[0].group(1).strip()
    if not source:
        raise ValueError("dependencies: TOML fence must not be empty")
    try:
        parsed = tomllib.loads(source)
    except tomllib.TOMLDecodeError as error:
        raise ValueError(f"dependencies: invalid TOML: {error}") from error
    dependencies = parsed.get("dependencies")
    if not isinstance(dependencies, dict):
        raise ValueError("dependencies: TOML must contain a dependencies table")
    return dependencies


def validate_dependencies(guides: dict[str, str], fixture_toml: str) -> None:
    """Require matching bilingual dependencies covered by the compile fixture."""
    parsed_guides = {
        language: extract_dependencies(markdown)
        for language, markdown in guides.items()
    }
    if "en" not in parsed_guides or "zh_CN" not in parsed_guides:
        raise ValueError("dependencies: guides must include en and zh_CN")
    english = parsed_guides["en"]
    chinese = parsed_guides["zh_CN"]
    if english != chinese:
        en_only = sorted(english.keys() - chinese.keys())
        zh_only = sorted(chinese.keys() - english.keys())
        raise ValueError(
            "dependencies: en and zh_CN differ "
            f"(en-only: {en_only}, zh_CN-only: {zh_only}, "
            f"version/feature differences: "
            f"{sorted(key for key in english.keys() & chinese.keys() if english[key] != chinese[key])})"
        )
    try:
        fixture = tomllib.loads(fixture_toml)
    except tomllib.TOMLDecodeError as error:
        raise ValueError(f"fixture: invalid TOML: {error}") from error
    fixture_dependencies = fixture.get("dependencies")
    if not isinstance(fixture_dependencies, dict):
        raise ValueError("fixture: TOML must contain a dependencies table")
    guide_names = set(english)
    fixture_names = set(fixture_dependencies)
    guide_only = sorted(guide_names - fixture_names)
    fixture_only = sorted(fixture_names - guide_names)
    if guide_only:
        raise ValueError(f"en: dependencies missing from fixture: {guide_only}")
    if fixture_only:
        raise ValueError(f"fixture: dependencies missing from guide: {fixture_only}")

    fixture_dir = ROOT / "tests/fixtures/guide_examples"
    local_paths = {
        "qubit-http": ROOT.resolve(),
        "qubit-config": (ROOT.parent / "rs-config").resolve(),
        "qubit-redact": (ROOT.parent / "rs-redact").resolve(),
    }
    for language, dependencies in parsed_guides.items():
        for name, guide_value in dependencies.items():
            guide_dependency = normalize_dependency(guide_value)
            if "path" in guide_dependency:
                raise ValueError(f"{language}: dependency {name} field path is not allowed")
            fixture_dependency = normalize_dependency(fixture_dependencies[name])
            fixture_path = fixture_dependency.pop("path", None)
            if fixture_path is not None:
                expected_path = local_paths.get(name)
                if expected_path is None:
                    raise ValueError(f"fixture: dependency {name} field path is not allowed")
                resolved_path = (fixture_dir / fixture_path).resolve()
                if resolved_path != expected_path:
                    raise ValueError(f"fixture: dependency {name} field path is incorrect")
            differing_fields = sorted(
                field
                for field in guide_dependency.keys() | fixture_dependency.keys()
                if guide_dependency.get(field) != fixture_dependency.get(field)
            )
            if differing_fields:
                raise ValueError(
                    f"{language}: dependency {name} differs in fields {differing_fields}"
                )


def normalize_dependency(value: str | dict) -> dict:
    """Normalize equivalent Cargo dependency syntax for declaration checks."""
    result = {"version": value} if isinstance(value, str) else dict(value)
    result.setdefault("default-features", True)
    result["features"] = sorted(set(result.get("features", [])))
    return result


def toml_value(value) -> str:
    """Serialize the TOML values accepted in Cargo dependency declarations."""
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, list):
        return "[" + ", ".join(toml_value(item) for item in value) + "]"
    if isinstance(value, dict):
        fields = ", ".join(f"{key} = {toml_value(item)}" for key, item in value.items())
        return "{ " + fields + " }"
    raise ValueError(f"unsupported TOML dependency value: {type(value).__name__}")


def render_readme_manifest(
    markdown: str,
    manifest_dir: pathlib.Path,
    project_root: pathlib.Path = ROOT,
) -> str:
    """Render only a README's declared dependencies with a local qubit-http path."""
    dependencies = extract_dependencies(markdown)
    if "qubit-http" not in dependencies:
        raise ValueError("README: dependencies must include qubit-http")
    rendered = []
    for name, value in dependencies.items():
        if name != "qubit-http" and isinstance(value, str):
            rendered.append(f"{name} = {toml_value(value)}")
            continue
        if isinstance(value, str):
            dependency = {"version": value}
        elif isinstance(value, dict):
            dependency = dict(value)
        else:
            raise ValueError(f"README: dependency {name} must be a string or inline table")
        if name == "qubit-http":
            if "path" in dependency:
                raise ValueError("README: qubit-http path override is not allowed")
            dependency["path"] = pathlib.Path(
                os.path.relpath(project_root.resolve(), manifest_dir.resolve())
            ).as_posix()
        rendered.append(f"{name} = {toml_value(dependency)}")
    return "[package]\nname = \"readme-quick-start\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n\n[dependencies]\n" + "\n".join(rendered) + "\n"


def generate_readme_examples() -> list[tuple[str, pathlib.Path]]:
    """Write separate Cargo packages for the two README quick starts."""
    # Keep generated Cargo packages under the repository's root target directory
    # so standard build-artifact cleanup can remove them safely.
    output_root = ROOT / "target/readme_examples"
    generated = []
    for language, path in READMES.items():
        package = output_root / f"readme-{language}"
        source_dir = package / "src"
        source_dir.mkdir(parents=True, exist_ok=True)
        markdown = path.read_text(encoding="utf-8")
        code = extract(markdown, "quick-start")
        (package / "Cargo.toml").write_text(
            render_readme_manifest(markdown, package), encoding="utf-8"
        )
        (source_dir / "main.rs").write_text(code + "\n", encoding="utf-8")
        generated.append((language, package / "Cargo.toml"))
    return generated


def cargo_environment() -> dict[str, str]:
    """Keep all example build outputs under the repository's root target directory."""
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(ROOT / "target/guide_example_build")
    return environment


def check_readme_examples() -> None:
    """Compile each README quick start with exactly its declared dependencies."""
    for language, manifest in generate_readme_examples():
        try:
            subprocess.run(
                ["cargo", "check", "--manifest-path", str(manifest)],
                cwd=ROOT,
                env=cargo_environment(),
                check=True,
            )
        except subprocess.CalledProcessError as error:
            raise RuntimeError(f"README {language} quick-start compilation failed") from error


def extract(markdown: str, scenario: str) -> str:
    start = f"<!-- guide-check:{scenario}:start -->"
    end = f"<!-- guide-check:{scenario}:end -->"
    if markdown.count(start) != 1 or markdown.count(end) != 1:
        raise ValueError(f"{scenario}: expected exactly one start and end marker")
    section = markdown.split(start, 1)[1].split(end, 1)[0]
    if "<!-- guide-check:" in section:
        raise ValueError(f"{scenario}: nested or mismatched marker")
    snippets = [match.group(1).strip() for match in FENCE.finditer(section)]
    if not snippets or any(not code for code in snippets):
        raise ValueError(f"{scenario}: expected non-empty complete Rust fence(s)")
    return "\n\n".join(snippets)


def replace_refresh_declaration(source: str) -> str:
    """Rename only the example's refresh implementation for its test harness."""
    if source.count(TOKEN_REFRESH_DECLARATION) != 1:
        raise ValueError("token-refresh: expected exactly one refresh declaration")
    renamed = TOKEN_REFRESH_DECLARATION.replace("refresh_access_token", "example_refresh_access_token")
    return source.replace(TOKEN_REFRESH_DECLARATION, "#[allow(dead_code)]\n" + renamed, 1)


def generate_quick_starts(
    destination: pathlib.Path, entries: dict[str, pathlib.Path] = QUICK_STARTS
) -> list[pathlib.Path]:
    destination.mkdir(parents=True, exist_ok=True)
    generated = []
    for name, path in entries.items():
        code = extract(path.read_text(encoding="utf-8"), "quick-start")
        source = destination / f"{name}.rs"
        source.write_text(code + "\n", encoding="utf-8")
        generated.append(source)
    return generated


def generate() -> list[pathlib.Path]:
    destination = ROOT / "target/guide_examples/src/bin"
    destination.mkdir(parents=True, exist_ok=True)
    for language in READMES:
        (destination / f"readme-{language}.rs").unlink(missing_ok=True)
    generated = generate_quick_starts(destination)
    for scenario in SCENARIOS:
        for language, path in GUIDES.items():
            code = extract(path.read_text(encoding="utf-8"), scenario)
            source = destination / f"{scenario}-{language}.rs"
            if scenario == "url-join":
                # This guide block is already a complete program with its own imports and main.
                source.write_text(code + "\n", encoding="utf-8")
                generated.append(source)
                continue
            if scenario == "token-refresh":
                source.write_text(code + "\n", encoding="utf-8")
                generated.append(source)
                regression = destination / f"token-refresh-regression-{language}.rs"
                renamed = replace_refresh_declaration(code)
                harness = (ROOT / "tests/fixtures/guide_examples/token_refresh_tests.rs").read_text(encoding="utf-8")
                regression.write_text(renamed + "\n\n" + harness, encoding="utf-8")
                generated.append(regression)
                continue
            context = "#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {\n"
            if scenario in ("async-upload", "sse-reconnect"):
                context = "use qubit_http::HttpClient;\n\n" + context
                mutable = "mut " if scenario == "async-upload" else ""
                context += f"    let {mutable}client = HttpClient::builder().build()?;\n"
            context += "\n".join(f"    {line}" if line else "" for line in code.splitlines())
            if scenario == "configuration":
                context += "\n    let _ = client;"
            context += "\n    Ok(())\n}\n"
            source.write_text(context, encoding="utf-8")
            generated.append(source)
    return generated


def validate_generated_bins(
    generated: list[pathlib.Path], manifest_toml: str, manifest_dir: pathlib.Path
) -> None:
    """Require each generated guide source to have exactly one matching Cargo bin."""
    try:
        manifest = tomllib.loads(manifest_toml)
    except tomllib.TOMLDecodeError as error:
        raise ValueError(f"fixture: invalid TOML: {error}") from error
    bins = manifest.get("bin", [])
    if not isinstance(bins, list):
        raise ValueError("fixture: [[bin]] declarations must be an array")

    declared: dict[str, pathlib.Path] = {}
    paths: dict[pathlib.Path, str] = {}
    for entry in bins:
        if (
            not isinstance(entry, dict)
            or not isinstance(entry.get("name"), str)
            or not isinstance(entry.get("path"), str)
        ):
            raise ValueError("fixture: each [[bin]] must declare name and path")
        name = entry["name"]
        path = (manifest_dir / entry["path"]).resolve()
        if name in declared:
            raise ValueError(f"fixture: duplicate bin name {name}")
        if path in paths:
            raise ValueError(f"fixture: duplicate bin path {path} ({paths[path]} and {name})")
        declared[name] = path
        paths[path] = name

    expected = {source.stem: source.resolve() for source in generated}
    for name, path in expected.items():
        if declared.get(name) != path:
            if name not in declared:
                raise ValueError(f"fixture: missing bin {name} for generated source {path}")
            raise ValueError(f"fixture: bin {name} points to {declared[name]}, expected {path}")
    for name, path in declared.items():
        if name not in expected:
            raise ValueError(f"fixture: extra bin {name} points to {path}")


class LocalLinkTests(unittest.TestCase):
    def test_local_chinese_heading_and_legacy_anchor(self):
        source = pathlib.Path("guide.md")
        documents = {
            source: "[correct](#header注入器和拦截器)\n[legacy](#header-注入器和拦截器)\n"
            "## Header、注入器和拦截器\n"
        }

        errors = check_local_links(documents)

        self.assertEqual(len(errors), 1)
        self.assertIn("missing heading anchor", errors[0])
        self.assertIn("#header-注入器和拦截器", errors[0])

    def test_relative_files_and_cross_file_headings(self):
        source = pathlib.Path("docs/index.md")
        target = pathlib.Path("docs/guide.md")
        documents = {
            source: "[guide](guide.md#install-%E4%B8%AD%E6%96%87) [missing](absent.md)\n",
            target: "## Install 中文\n",
        }

        errors = check_local_links(documents)

        self.assertEqual(len(errors), 1)
        self.assertIn("missing local file", errors[0])
        self.assertIn("absent.md", errors[0])

    def test_ignores_external_links_and_fenced_code(self):
        source = pathlib.Path("guide.md")
        documents = {
            source: "[external](https://example.com/missing) [protocol-relative](//example.com/missing)\n"
            "```markdown\n[hidden](missing.md#absent)\n```\n"
        }

        self.assertEqual(check_local_links(documents), [])

    def test_fence_scanner_tracks_marker_and_opening_length(self):
        source = pathlib.Path("guide.md")
        markdown = (
            "````markdown\n"
            "```\n"
            "[hidden](inside.md)\n"
            "# Fake Heading\n"
            "````\n"
            "[visible](after.md)\n"
            "~~~text\n"
            "```` not a tilde closer\n"
            "[also hidden](inside.md)\n"
            "~~~~ trailing text\n"
            "~~~~~\n"
            "[also visible](missing.md)\n"
        )
        documents = {source: markdown}

        errors = check_local_links(documents)
        anchors = markdown_anchors(markdown)

        self.assertEqual(len(errors), 2)
        self.assertIn("guide.md:6: missing local file", errors[0])
        self.assertIn("guide.md:12: missing local file", errors[1])
        self.assertNotIn("fake-heading", anchors)

    def test_html_ids_and_duplicate_heading_anchors(self):
        markdown = "<span id=\"custom-section\"></span>\n## Repeated Heading\n## Repeated Heading\n"
        markdown += "```html\n<span id=\"hidden-section\"></span>\n````\n"

        anchors = markdown_anchors(markdown)

        self.assertIn("custom-section", anchors)
        self.assertIn("repeated-heading", anchors)
        self.assertIn("repeated-heading-1", anchors)
        self.assertNotIn("hidden-section", anchors)

    def test_repository_readmes_and_guides_have_valid_local_links(self):
        check_repository_local_links()


class GeneratedBinTests(unittest.TestCase):
    def setUp(self):
        self.directory = pathlib.Path("/fixture")
        self.sources = [
            pathlib.Path("/generated/url-join-en.rs"),
            pathlib.Path("/generated/url-join-zh_CN.rs"),
        ]
        self.valid = '''[[bin]]
name = "url-join-en"
path = "../generated/url-join-en.rs"
[[bin]]
name = "url-join-zh_CN"
path = "../generated/url-join-zh_CN.rs"
'''

    def test_complete_pair(self):
        validate_generated_bins(self.sources, self.valid, self.directory)

    def test_missing_pair_target(self):
        manifest = self.valid.replace(
            '[[bin]]\nname = "url-join-zh_CN"\npath = "../generated/url-join-zh_CN.rs"\n',
            "",
        )
        with self.assertRaisesRegex(ValueError, "missing bin url-join-zh_CN"):
            validate_generated_bins(self.sources, manifest, self.directory)

    def test_extra_target_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "extra bin unrelated"):
            validate_generated_bins(
                self.sources,
                self.valid + '\n[[bin]]\nname="unrelated"\npath="../generated/other.rs"\n',
                self.directory,
            )

    def test_same_name_wrong_path_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "points to .*expected"):
            validate_generated_bins(
                self.sources,
                self.valid.replace("../generated/url-join-en.rs", "../generated/other.rs"),
                self.directory,
            )

    def test_duplicate_name_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate bin name url-join-en"):
            validate_generated_bins(
                self.sources,
                self.valid + '\n[[bin]]\nname="url-join-en"\npath="../generated/other.rs"\n',
                self.directory,
            )

    def test_duplicate_path_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate bin path"):
            validate_generated_bins(
                self.sources,
                self.valid.replace("url-join-zh_CN.rs", "url-join-en.rs"),
                self.directory,
            )


class ExtractorTests(unittest.TestCase):
    def test_url_join_marker_entries_exist_in_both_guides(self):
        for language, path in GUIDES.items():
            with self.subTest(language=language):
                code = extract(path.read_text(encoding="utf-8"), "url-join")
                self.assertEqual(len(re.findall(r"\bfn\s+main\s*\(", code)), 1)
                self.assertEqual(code.count("use http::Method;"), 1)
                self.assertEqual(code.count("use qubit_http::{HttpClient, HttpClientOptions};"), 1)

    def test_url_join_missing_marker_fails(self):
        with self.assertRaisesRegex(ValueError, "url-join: expected exactly one start and end marker"):
            extract("```rust\nfn main() {}\n```", "url-join")

    def test_quick_start_entries_cover_all_documents(self):
        self.assertEqual(
            {name: path.relative_to(ROOT).as_posix() for name, path in QUICK_STARTS.items()},
            {
                "quick-start-en": "doc/user_guide.en.md",
                "quick-start-zh_CN": "doc/user_guide.zh_CN.md",
            },
        )

    def test_quick_start_missing_marker_fails(self):
        with self.assertRaisesRegex(ValueError, "quick-start: expected exactly one start and end marker"):
            extract("```rust\nfn main() {}\n```", "quick-start")

    def test_quick_start_is_written_raw_with_one_main(self):
        marker = "<!-- guide-check:quick-start:start -->\n```rust\nfn main() {}\n```\n<!-- guide-check:quick-start:end -->"
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "sample.md"
            path.write_text(marker, encoding="utf-8")
            generated = generate_quick_starts(
                pathlib.Path(temporary) / "out", {"sample": path}
            )
            output = generated[0].read_text(encoding="utf-8")
        self.assertEqual(output, extract(marker, "quick-start") + "\n")
        self.assertEqual(len(re.findall(r"\bfn\s+main\s*\(", output)), 1)

    def test_token_refresh_missing_marker(self):
        with self.assertRaisesRegex(ValueError, "exactly one start and end marker"):
            extract("```rust\nfn main() {}\n```", "token-refresh")

    def test_token_refresh_duplicate_marker(self):
        source = "<!-- guide-check:token-refresh:start -->\n<!-- guide-check:token-refresh:start -->\n```rust\nx\n```\n<!-- guide-check:token-refresh:end -->"
        with self.assertRaisesRegex(ValueError, "exactly one start and end marker"):
            extract(source, "token-refresh")

    def test_token_refresh_mismatched_marker(self):
        source = "<!-- guide-check:token-refresh:start -->\n<!-- guide-check:other:end -->\n```rust\nx\n```\n<!-- guide-check:token-refresh:end -->"
        with self.assertRaisesRegex(ValueError, "nested or mismatched marker"):
            extract(source, "token-refresh")

    def test_token_refresh_empty_code(self):
        source = "<!-- guide-check:token-refresh:start -->\n```rust\n\n```\n<!-- guide-check:token-refresh:end -->"
        with self.assertRaisesRegex(ValueError, "non-empty complete Rust"):
            extract(source, "token-refresh")

    def test_refresh_declaration_must_match_once(self):
        for source in ("", TOKEN_REFRESH_DECLARATION + "\n" + TOKEN_REFRESH_DECLARATION):
            with self.assertRaisesRegex(ValueError, "exactly one refresh declaration"):
                replace_refresh_declaration(source)

    def test_refresh_declaration_renames_only_its_definition(self):
        source = TOKEN_REFRESH_DECLARATION + "\n    refresh_access_token().await\n"
        renamed = replace_refresh_declaration(source)
        self.assertIn("async fn example_refresh_access_token()", renamed)
        self.assertIn("refresh_access_token().await", renamed)

    def test_duplicate_marker(self):
        with self.assertRaises(ValueError):
            extract("<!-- guide-check:x:start -->\n<!-- guide-check:x:start -->\n```rust\nx\n```\n<!-- guide-check:x:end -->", "x")

    def test_missing_end_marker(self):
        with self.assertRaises(ValueError):
            extract("<!-- guide-check:x:start -->\n```rust\nx\n```", "x")

    def test_empty_fence(self):
        with self.assertRaises(ValueError):
            extract("<!-- guide-check:x:start -->\n```rust\n\n```\n<!-- guide-check:x:end -->", "x")

    def test_language_difference_is_distinct(self):
        a = "<!-- guide-check:x:start -->\n```rust\na\n```\n<!-- guide-check:x:end -->"
        b = a.replace("\na\n", "\nb\n")
        self.assertNotEqual(extract(a, "x"), extract(b, "x"))


def guide(dependencies: str) -> str:
    return f"{DEPENDENCIES_START}\n```toml\n[dependencies]\n{dependencies}\n```\n{DEPENDENCIES_END}"


class DependencyTests(unittest.TestCase):
    def setUp(self):
        self.valid_guides = {"en": guide('bytes = "1.11"'), "zh_CN": guide('bytes = "1.11"')}
        self.fixture = '[dependencies]\nbytes = "1.11"\n'

    def test_valid_bilingual_input(self):
        validate_dependencies(self.valid_guides, self.fixture)

    def test_missing_bytes_in_one_language(self):
        guides = dict(self.valid_guides, en=guide(""))
        with self.assertRaisesRegex(ValueError, "differ"):
            validate_dependencies(guides, self.fixture)

    def test_bytes_missing_from_both_guides_even_when_fixture_has_it(self):
        guides = {"en": guide(""), "zh_CN": guide("")}
        with self.assertRaisesRegex(ValueError, "fixture: dependencies missing from guide.*bytes"):
            validate_dependencies(guides, self.fixture)

    def test_fixture_phantom_dependency_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "fixture: dependencies missing from guide.*phantom"):
            validate_dependencies(self.valid_guides, self.fixture + 'phantom = "0.1"\n')

    def test_bilingual_version_difference(self):
        guides = dict(self.valid_guides, zh_CN=guide('bytes = "1.12"'))
        with self.assertRaisesRegex(ValueError, "version/feature differences.*bytes"):
            validate_dependencies(guides, self.fixture)

    def test_bilingual_feature_difference(self):
        guides = {"en": guide('bytes = { version = "1.11", features = ["serde"] }'),
                  "zh_CN": guide('bytes = { version = "1.11", features = ["std"] }')}
        with self.assertRaisesRegex(ValueError, "version/feature differences.*bytes"):
            validate_dependencies(guides, self.fixture)

    def test_fixture_version_drift(self):
        guides = {lang: guide('tokio = "0.1"') for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "tokio.*version"):
            validate_dependencies(guides, '[dependencies]\ntokio = "1.54"\n')

    def test_fixture_enables_extra_feature(self):
        guides = {lang: guide('tokio = { version = "1", features = ["macros"] }')
                  for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "tokio.*features"):
            validate_dependencies(guides,
                '[dependencies]\ntokio = { version = "1", features = ["macros", "full"] }')

    def test_fixture_removes_feature(self):
        guides = {lang: guide('tokio = { version = "1", features = ["macros", "sync"] }')
                  for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "tokio.*features"):
            validate_dependencies(guides,
                '[dependencies]\ntokio = { version = "1", features = ["macros"] }')

    def test_fixture_changes_default_features(self):
        guides = {lang: guide('tokio = { version = "1", default-features = false }')
                  for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "tokio.*default-features"):
            validate_dependencies(guides, '[dependencies]\ntokio = "1"\n')

    def test_fixture_extra_cargo_field_is_rejected(self):
        guides = {lang: guide('bytes = "1.11"') for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "bytes.*optional"):
            validate_dependencies(guides,
                '[dependencies]\nbytes = { version = "1.11", optional = true }')

    def test_guide_only_dependency_is_rejected(self):
        guides = {lang: guide('bytes = "1.11"\nextra = "1"') for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "en.*extra"):
            validate_dependencies(guides, self.fixture)

    def test_fixture_only_dependency_is_rejected(self):
        guides = self.valid_guides
        with self.assertRaisesRegex(ValueError, "fixture.*phantom"):
            validate_dependencies(guides, self.fixture + 'phantom = "0.1"\n')

    def test_guide_path_is_rejected(self):
        guides = {lang: guide('bytes = { path = "../bytes", version = "1.11" }')
                  for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "en.*bytes.*path"):
            validate_dependencies(guides, self.fixture)

    def test_unapproved_fixture_path_is_rejected(self):
        guides = {lang: guide('bytes = "1.11"') for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "fixture.*bytes.*path"):
            validate_dependencies(guides,
                '[dependencies]\nbytes = { path = "../bytes", version = "1.11" }')

    def test_known_local_dependency_path_must_resolve_to_expected_directory(self):
        guides = {lang: guide('qubit-http = "0.16.0"') for lang in ("en", "zh_CN")}
        with self.assertRaisesRegex(ValueError, "fixture.*qubit-http.*path"):
            validate_dependencies(guides,
                '[dependencies]\nqubit-http = { path = "../wrong", version = "0.16.0" }')

    def test_equivalent_forms_features_and_known_local_paths(self):
        guides = {lang: guide('''qubit-http = "0.16.0"
qubit-config = { version = "0.15", default-features = false }
qubit-redact = "0.10"
tokio = { version = "1", features = ["sync", "macros", "sync"] }
bytes = { version = "1.11", default-features = true, features = [] }''')
                  for lang in ("en", "zh_CN")}
        fixture = f'''[dependencies]
qubit-http = {{ path = "../../../", version = "0.16.0" }}
qubit-config = {{ path = "../../../../rs-config", version = "0.15", default-features = false }}
qubit-redact = {{ path = "../../../../rs-redact", version = "0.10" }}
tokio = {{ version = "1", features = ["macros", "sync"] }}
bytes = "1.11"
'''
        validate_dependencies(guides, fixture)

    def test_fixture_missing_dependency(self):
        with self.assertRaisesRegex(ValueError, "fixture: dependencies missing from guide.*http"):
            validate_dependencies(self.valid_guides, '[dependencies]\nbytes = "1.11"\nhttp = "1"\n')

    def test_missing_marker(self):
        with self.assertRaisesRegex(ValueError, "exactly one start marker"):
            extract_dependencies("```toml\n[dependencies]\nbytes = '1'\n```")

    def test_missing_end_marker(self):
        source = guide('bytes = "1"').replace(DEPENDENCIES_END, "")
        with self.assertRaisesRegex(ValueError, "exactly one end marker"):
            extract_dependencies(source)

    def test_duplicate_marker(self):
        source = guide('bytes = "1"').replace(DEPENDENCIES_START, DEPENDENCIES_START * 2)
        with self.assertRaisesRegex(ValueError, "exactly one start marker"):
            extract_dependencies(source)

    def test_reversed_markers(self):
        source = guide('bytes = "1"').replace(DEPENDENCIES_START, "TEMP").replace(DEPENDENCIES_END, DEPENDENCIES_START).replace("TEMP", DEPENDENCIES_END)
        with self.assertRaisesRegex(ValueError, "start marker must precede end marker"):
            extract_dependencies(source)

    def test_multiple_fences(self):
        source = guide('bytes = "1"').replace(DEPENDENCIES_END, '```toml\n[dependencies]\nother = "1"\n```\n' + DEPENDENCIES_END)
        with self.assertRaisesRegex(ValueError, "exactly one TOML fence"):
            extract_dependencies(source)

    def test_empty_fence(self):
        with self.assertRaisesRegex(ValueError, "must not be empty"):
            extract_dependencies(
                f"{DEPENDENCIES_START}\n```toml\n\n```\n{DEPENDENCIES_END}"
            )

    def test_invalid_toml(self):
        with self.assertRaisesRegex(ValueError, "invalid TOML"):
            extract_dependencies(guide('bytes = "unterminated'))

    def test_missing_dependencies_table(self):
        source = f"{DEPENDENCIES_START}\n```toml\n[package]\nname = 'x'\n```\n{DEPENDENCIES_END}"
        with self.assertRaisesRegex(ValueError, "dependencies table"):
            extract_dependencies(source)

    def test_fixture_invalid_toml(self):
        with self.assertRaisesRegex(ValueError, "fixture: invalid TOML"):
            validate_dependencies(self.valid_guides, "[dependencies\n")

    def test_fixture_missing_dependencies_table(self):
        with self.assertRaisesRegex(ValueError, "fixture: TOML must contain a dependencies table"):
            validate_dependencies(self.valid_guides, "[package]\nname = 'x'\n")


class ReadmeDependencyTests(unittest.TestCase):
    def test_both_readmes_have_independent_dependency_blocks(self):
        for language, path in READMES.items():
            with self.subTest(language=language):
                dependencies = extract_dependencies(path.read_text(encoding="utf-8"))
                self.assertIn("qubit-http", dependencies)

    def test_readme_manifest_preserves_declared_dependencies_and_local_http_path(self):
        source = guide('''qubit-http = "0.17"
http = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }''')
        manifest_dir = pathlib.Path("/tmp/readme-example")
        project_root = pathlib.Path("/workspace/rs-http")
        manifest = render_readme_manifest(source, manifest_dir, project_root)
        dependencies = tomllib.loads(manifest)["dependencies"]

        self.assertEqual(dependencies["qubit-http"]["version"], "0.17")
        dependency_path = pathlib.Path(dependencies["qubit-http"]["path"])
        self.assertFalse(dependency_path.is_absolute())
        self.assertEqual((manifest_dir / dependency_path).resolve(), project_root.resolve())
        self.assertEqual(dependencies["http"], "1")
        self.assertEqual(dependencies["tokio"]["features"], ["macros", "rt-multi-thread"])
        self.assertEqual(set(dependencies), {"qubit-http", "http", "tokio"})

    def test_readme_manifest_does_not_invent_an_undeclared_dependency(self):
        manifest = render_readme_manifest(
            guide('qubit-http = "0.17"'),
            pathlib.Path("/tmp/readme-example"),
            pathlib.Path("/workspace/rs-http"),
        )

        self.assertNotIn("http", tomllib.loads(manifest)["dependencies"])

    def test_readme_manifest_rejects_document_supplied_qubit_http_path(self):
        source = guide('qubit-http = { version = "0.17", path = "../other" }')

        with self.assertRaisesRegex(ValueError, "qubit-http path override is not allowed"):
            render_readme_manifest(
                source,
                pathlib.Path("/tmp/readme-example"),
                pathlib.Path("/workspace/rs-http"),
            )


def run_self_tests() -> bool:
    suite = unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__])
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return result.wasSuccessful()


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(0 if run_self_tests() else 1)
    else:
        if not run_self_tests():
            sys.exit(1)
        validate_dependencies(
            {language: path.read_text(encoding="utf-8") for language, path in GUIDES.items()},
            (ROOT / "tests/fixtures/guide_examples/Cargo.toml").read_text(encoding="utf-8"),
        )
        check_readme_examples()
        generated = generate()
        fixture_manifest = ROOT / "tests/fixtures/guide_examples/Cargo.toml"
        validate_generated_bins(
            generated,
            fixture_manifest.read_text(encoding="utf-8"),
            fixture_manifest.parent,
        )
        subprocess.run(
            ["cargo", "check", "--manifest-path", "tests/fixtures/guide_examples/Cargo.toml", "--all-targets"],
            cwd=ROOT,
            env=cargo_environment(),
            check=True,
        )
        for target in ("url-join-en", "url-join-zh_CN"):
            subprocess.run(
                ["cargo", "run", "--manifest-path", str(fixture_manifest), "--bin", target],
                cwd=ROOT,
                env=cargo_environment(),
                check=True,
            )
        subprocess.run(
            [
                "cargo", "test", "--manifest-path", "tests/fixtures/guide_examples/Cargo.toml",
                "--bin", "token-refresh-regression-en", "--bin", "token-refresh-regression-zh_CN",
                "--", "--test-threads=1",
            ],
            cwd=ROOT,
            env=cargo_environment(),
            check=True,
        )
