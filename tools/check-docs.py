#!/usr/bin/env python3
"""Check this field guide's inline local links, generated anchors and SVG files.

Run after `mdbook build docs`. Remote URLs are not fetched. This intentionally
checks the inline Markdown link syntax used by this guide, not all Markdown.
"""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import re
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BOOK = ROOT / "target/docs-book"


class Page(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.ids = set()
        self.feed(path.read_text())

    def handle_starttag(self, tag, attrs):
        attributes = dict(attrs)
        if "id" in attributes:
            self.ids.add(attributes["id"])


def main():
    errors = []
    checked = 0
    for source in [ROOT / "README.md", *sorted((ROOT / "docs").glob("*.md"))]:
        for href in re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", source.read_text()):
            url = urlsplit(href)
            if url.scheme or url.netloc:
                continue
            checked += 1
            target = (source.parent / unquote(url.path)).resolve() if url.path else source
            if not target.exists():
                errors.append(f"{source.name}: missing {href}")
            elif url.fragment and target.suffix == ".md" and target.parent == ROOT / "docs":
                html = BOOK / (target.stem + ".html")
                if not html.exists():
                    errors.append(f"Missing {html.name}; run mdbook build docs first")
                elif unquote(url.fragment) not in Page(html).ids:
                    errors.append(f"{source.name}: missing anchor {href}")

    for svg in sorted((ROOT / "docs/diagrams").glob("*.svg")):
        try:
            root = ET.parse(svg).getroot()
            namespace = {"svg": "http://www.w3.org/2000/svg"}
            for tag in ("title", "desc"):
                element = root.find(f"svg:{tag}", namespace)
                if element is None or not (element.text or "").strip():
                    errors.append(f"{svg.name}: missing accessible {tag}")
            if root.get("role") != "img" or root.get("aria-labelledby") != "title desc":
                errors.append(f"{svg.name}: missing accessible image role or labels")
        except ET.ParseError as error:
            errors.append(f"{svg.name}: {error}")

    # Keep operator diagnostics aligned when firmware adds or renames a fault.
    labels = re.findall(r'Self::\w+ => "([^"]+)"',
                        (ROOT / "crates/shared/src/safety.rs").read_text())
    fault_guide = (ROOT / "docs/FAULTS.md").read_text()
    for label in labels:
        if f"`{label}`" not in fault_guide:
            errors.append(f"Fault guide is missing label: {label}")
    if not labels:
        errors.append("No fault labels found; review the source parser")
    if errors:
        raise SystemExit("\n".join(errors))
    print(f"Passed: {checked} local links/anchors, SVG XML/accessibility, {len(labels)} fault labels.")


if __name__ == "__main__":
    main()
