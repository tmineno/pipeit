#!/usr/bin/env python3
"""
Generate standard library reference documentation from std_*.h headers.

Parses Doxygen-style /// comments and ACTOR() macro signatures to produce
a flat markdown specification at doc/spec/standard-library-spec-v0.3.0.md.

Usage:
    python3 scripts/gen-stdlib-doc.py
"""

import re
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
INCLUDE_DIR = PROJECT_ROOT / "runtime" / "libpipit" / "include"
OUTPUT = PROJECT_ROOT / "doc" / "spec" / "standard-library-spec-v0.3.0.md"

# Regex patterns
RE_FILE = re.compile(r"/// @file")
RE_DEFGROUP = re.compile(r"/// @defgroup\s+\S+\s+(.*)")
RE_GROUP_END = re.compile(r"/// @}")
RE_BRIEF = re.compile(r"/// @brief\s+(.*)")
RE_PARAM = re.compile(r"/// @param\s+(\S+)\s+(.*)")
RE_RETURN = re.compile(r"/// @return\s+(.*)")
RE_CODE_START = re.compile(r"/// @code\{\.pdl\}")
RE_CODE_END = re.compile(r"/// @endcode")
RE_DOC_LINE = re.compile(r"/// ?(.*)")
RE_ACTOR = re.compile(
    r"(?:template\s*<[^>]+>\s*)?ACTOR\((\w+),\s*IN\((\w+),\s*(\w+(?:\(\w+\))?)\),\s*OUT\((\w+),\s*(\w+(?:\(\w+\))?)\)"
)


def parse_actors(src: str) -> list[dict]:
    """Parse a std_*.h header and extract actor documentation."""
    lines = src.splitlines()
    actors = []
    current_group = None
    i = 0

    in_file_block = False

    while i < len(lines):
        line = lines[i]

        # Skip @file doc blocks (file-level documentation)
        if RE_FILE.match(line.strip()):
            in_file_block = True
            i += 1
            continue
        if in_file_block:
            if line.strip().startswith("///"):
                i += 1
                continue
            in_file_block = False

        # Track group
        m = RE_DEFGROUP.match(line.strip())
        if m:
            current_group = m.group(1)
            i += 1
            continue

        if RE_GROUP_END.match(line.strip()):
            i += 1
            continue

        # Start of actor doc block
        m = RE_BRIEF.match(line.strip())
        if m:
            actor = {
                "group": current_group,
                "brief": m.group(1),
                "description": [],
                "params": [],
                "returns": "",
                "example": [],
                "signature": "",
                "name": "",
                "in_type": "",
                "in_count": "",
                "out_type": "",
                "out_count": "",
            }
            i += 1
            in_code = False

            # Parse doc comment block
            while i < len(lines):
                line = lines[i].strip()

                if not line.startswith("///") and not line == "":
                    break

                if line == "":
                    i += 1
                    continue

                if RE_CODE_START.match(line):
                    in_code = True
                    i += 1
                    continue
                if RE_CODE_END.match(line):
                    in_code = False
                    i += 1
                    continue
                if in_code:
                    dm = RE_DOC_LINE.match(line)
                    if dm:
                        actor["example"].append(dm.group(1))
                    i += 1
                    continue

                pm = RE_PARAM.match(line)
                if pm:
                    actor["params"].append((pm.group(1), pm.group(2)))
                    i += 1
                    continue

                rm = RE_RETURN.match(line)
                if rm:
                    actor["returns"] = rm.group(1)
                    i += 1
                    continue

                dm = RE_DOC_LINE.match(line)
                if dm:
                    text = dm.group(1)
                    if text and text != "Example usage:":
                        actor["description"].append(text)
                    i += 1
                    continue

                i += 1

            # Find ACTOR macro line
            while i < len(lines):
                line = lines[i]
                am = RE_ACTOR.search(line.strip())
                if am:
                    actor["name"] = am.group(1)
                    actor["in_type"] = am.group(2)
                    actor["in_count"] = am.group(3)
                    actor["out_type"] = am.group(4)
                    actor["out_count"] = am.group(5)
                    # Capture full signature up to opening brace
                    sig_lines = []
                    while i < len(lines) and "{" not in lines[i]:
                        sig_lines.append(lines[i].strip())
                        i += 1
                    if i < len(lines):
                        sig_line = lines[i].strip()
                        sig_lines.append(sig_line.split("{")[0].strip())
                    actor["signature"] = " ".join(sig_lines).strip()
                    # Clean trailing )
                    if actor["signature"].endswith(")"):
                        actor["signature"] = actor["signature"]
                    break
                i += 1

            if actor["name"]:
                actors.append(actor)

        i += 1

    return actors


def generate_markdown(actors: list[dict]) -> str:
    """Generate flat markdown from parsed actor data."""
    lines = [
        "# Pipit Standard Library Reference",
        "",
        "<!-- Auto-generated from std_*.h by scripts/gen-stdlib-doc.py -->",
        "<!-- Do not edit manually -->",
        "",
        "## Quick Reference",
        "",
        "| Actor | Input | Output | Description |",
        "|-------|-------|--------|-------------|",
    ]

    for a in actors:
        in_sig = f"{a['in_type']}[{a['in_count']}]" if a["in_type"] != "void" else "void"
        out_sig = (
            f"{a['out_type']}[{a['out_count']}]"
            if a["out_type"] != "void"
            else "void"
        )
        lines.append(f"| `{a['name']}` | {in_sig} | {out_sig} | {a['brief']} |")

    lines.append("")

    # Group actors by category
    groups: dict[str, list[dict]] = {}
    for a in actors:
        g = a["group"] or "Other"
        groups.setdefault(g, []).append(a)

    for group_name, group_actors in groups.items():
        lines.append(f"## {group_name}")
        lines.append("")

        for a in group_actors:
            lines.append(f"### {a['name']}")
            lines.append("")
            # Brief + description as a single paragraph
            desc = " ".join(a["description"]).strip()
            if desc:
                lines.append(f"**{a['brief']}** — {desc}")
            else:
                lines.append(a["brief"])
            lines.append("")

            # Signature
            lines.append("**Signature:**")
            lines.append("")
            lines.append("```cpp")
            lines.append(a["signature"])
            lines.append("```")
            lines.append("")

            # Parameters
            if a["params"]:
                lines.append("**Parameters:**")
                lines.append("")
                for pname, pdesc in a["params"]:
                    lines.append(f"- `{pname}` - {pdesc}")
                lines.append("")

            # Return
            if a["returns"]:
                lines.append(f"**Returns:** {a['returns']}")
                lines.append("")

            # Example
            if a["example"]:
                lines.append("**Example:**")
                lines.append("")
                lines.append("```pdl")
                for eline in a["example"]:
                    lines.append(eline)
                lines.append("```")
                lines.append("")

            lines.append("---")
            lines.append("")

    return "\n".join(lines)


def main() -> int:
    headers = sorted(INCLUDE_DIR.glob("std_*.h"))
    if not headers:
        print(f"Error: no std_*.h files found in {INCLUDE_DIR}", file=sys.stderr)
        return 1

    actors: list[dict] = []
    for header in headers:
        src = header.read_text()
        found = parse_actors(src)
        actors.extend(found)
        if found:
            print(f"  {header.name}: {len(found)} actors")

    if not actors:
        print("Error: no actors found in any std_*.h header", file=sys.stderr)
        return 1

    md = generate_markdown(actors)

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(md)

    print(f"Generated {OUTPUT} ({len(actors)} actors from {len(headers)} headers)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
