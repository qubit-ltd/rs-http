#!/usr/bin/env python3
"""Compile Rust examples marked in the English and Chinese user guides."""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
GUIDES = {
    "en": ROOT / "doc/user_guide.en.md",
    "zh_CN": ROOT / "doc/user_guide.zh_CN.md",
}
SCENARIOS = ("configuration", "async-upload", "redaction", "sse-reconnect")
FENCE = re.compile(r"```rust\s*\n(.*?)\n```", re.DOTALL)


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
                context = "use qubit_http::HttpClientBuilder;\n\n" + context
                mutable = "mut " if scenario == "async-upload" else ""
                context += f"    let {mutable}client = HttpClientBuilder::new().create_default()?;\n"
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


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        unittest.main(argv=[sys.argv[0]])
    else:
        generate()
        subprocess.run(
            ["cargo", "check", "--manifest-path", "tests/fixtures/guide_examples/Cargo.toml", "--all-targets"],
            cwd=ROOT,
            check=True,
        )
