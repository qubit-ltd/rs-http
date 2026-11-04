#!/usr/bin/env python3
"""Compile Rust examples marked in the English and Chinese user guides.

Requires Python 3.11+ for the standard-library ``tomllib`` module.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
GUIDES = {
    "en": ROOT / "doc/user_guide.en.md",
    "zh_CN": ROOT / "doc/user_guide.zh_CN.md",
}
SCENARIOS = ("configuration", "async-upload", "redaction", "sse-reconnect")
FENCE = re.compile(r"```rust\s*\n(.*?)\n```", re.DOTALL)
DEPENDENCIES_START = "<!-- guide-dependencies:start -->"
DEPENDENCIES_END = "<!-- guide-dependencies:end -->"
TOML_FENCE = re.compile(r"```toml\s*\n(.*?)\n```", re.DOTALL)


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


def generate() -> list[pathlib.Path]:
    destination = ROOT / "tests/fixtures/guide_examples/target/guide_examples/src/bin"
    destination.mkdir(parents=True, exist_ok=True)
    generated = []
    for scenario in SCENARIOS:
        for language, path in GUIDES.items():
            code = extract(path.read_text(encoding="utf-8"), scenario)
            source = destination / f"{scenario}-{language}.rs"
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


class ExtractorTests(unittest.TestCase):
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
        generate()
        subprocess.run(
            ["cargo", "check", "--manifest-path", "tests/fixtures/guide_examples/Cargo.toml", "--all-targets"],
            cwd=ROOT,
            check=True,
        )
