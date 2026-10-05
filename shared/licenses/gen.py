#!/usr/bin/env python3
"""Generates THIRD_PARTY_NOTICES at the repository root (stdlib only).

Sources:
- shared/licenses/third_party.json and texts/: components that are not Rust crates (C/C++ libraries
  fetched by CMake, npm runtime dependencies, the Emscripten runtime);
- `cargo metadata` for server/net/wt: every crate linked into the native server (normal
  dependencies on any platform; build scripts' and tests' dependencies are not shipped). Each
  crate's license text is read from its downloaded source; the few crates that publish no license
  file all offer MIT, and get its standard text (texts/MIT-template.txt) with their authors.

For a crate offered under a choice of licenses ("MIT OR Apache-2.0") the most permissive is used
(see PREFERENCE); for combined licenses ("ISC AND Apache-2.0") every license file the crate
ships is included. Identical texts are printed once, with every component that uses them.

Usage: python3 shared/licenses/gen.py [--check]   (--check: fail if the file is out of date)
"""

import json
import os
import re
import subprocess
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
HERE = os.path.join(ROOT, "shared", "licenses")
CRATE = os.path.join(ROOT, "server", "net", "wt")
OUT = os.path.join(ROOT, "THIRD_PARTY_NOTICES")

# Lower is preferred when a crate offers a choice.
PREFERENCE = [
    "MIT", "MIT-0", "ISC", "BSD-2-Clause", "BSD-3-Clause", "Zlib", "Unlicense",
    "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "Unicode-3.0", "CDLA-Permissive-2.0",
]
# File-name hints per license, for crates that ship one file per license.
HINTS = {
    "MIT": ["MIT"], "MIT-0": ["MIT"], "ISC": ["ISC"], "BSD-2-Clause": ["BSD"],
    "BSD-3-Clause": ["BSD"], "Zlib": ["ZLIB"], "Unlicense": ["UNLICENSE"],
    "Apache-2.0": ["APACHE"], "Apache-2.0 WITH LLVM-exception": ["APACHE", "LLVM"],
    "Unicode-3.0": ["UNICODE"], "CDLA-Permissive-2.0": ["CDLA"],
}
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)", re.IGNORECASE)


def rank(lic):
    return PREFERENCE.index(lic) if lic in PREFERENCE else len(PREFERENCE)


def parse(expr):
    """SPDX expression -> nested ('OR'|'AND', [...]) / license id. '/' is the old spelling of OR."""
    tokens = re.findall(r"\(|\)|[^\s()]+", expr.replace("/", " OR "))
    pos = 0

    def atom():
        nonlocal pos
        tok = tokens[pos]
        pos += 1
        if tok == "(":
            node = disjunction()
            pos += 1  # ")"
            return node
        if pos < len(tokens) and tokens[pos] == "WITH":
            tok = f"{tok} WITH {tokens[pos + 1]}"
            pos += 2
        return tok

    def conjunction():
        nonlocal pos
        items = [atom()]
        while pos < len(tokens) and tokens[pos] == "AND":
            pos += 1
            items.append(atom())
        return items[0] if len(items) == 1 else ("AND", items)

    def disjunction():
        nonlocal pos
        items = [conjunction()]
        while pos < len(tokens) and tokens[pos] == "OR":
            pos += 1
            items.append(conjunction())
        return items[0] if len(items) == 1 else ("OR", items)

    return disjunction()


def choose(node):
    """The licenses actually used: every term of an AND, the preferred option of an OR."""
    if isinstance(node, str):
        return [node]
    op, items = node
    if op == "AND":
        return [lic for item in items for lic in choose(item)]
    options = [choose(item) for item in items]
    return min(options, key=lambda lics: max(rank(lic) for lic in lics))


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read().strip() + "\n"


def crate_texts(pkg):
    """The license texts a crate is used under, from the files in its source."""
    root = os.path.dirname(pkg["manifest_path"])
    files = sorted(f for f in os.listdir(root)
                   if LICENSE_FILE.match(f) and os.path.isfile(os.path.join(root, f)))
    reuse = os.path.join(root, "LICENSES")  # the REUSE convention: one file per license
    if os.path.isdir(reuse):
        files += sorted(f"LICENSES/{f}" for f in os.listdir(reuse))
    if pkg.get("license_file"):
        files = sorted(set(files) | {pkg["license_file"]})
    expr = pkg.get("license") or ""
    used = choose(parse(expr)) if expr else []
    if not files:
        # A few crates publish no license file. They all offer MIT: use its standard text with the
        # crate's authors (MIT requires the copyright notice and permission notice).
        if "MIT" not in used:
            sys.exit(f"{pkg['name']} {pkg['version']}: no license file in {root} and no MIT option")
        holders = ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in pkg.get("authors") or [])
        template = read(os.path.join(HERE, "texts", "MIT-template.txt"))
        return "MIT", [template.replace("{holders}", holders or f"the {pkg['name']} authors")]
    if "AND" in expr.split() or len(files) == 1:
        picked = files
    else:
        picked = []
        for lic in used:
            match = [f for f in files if any(h in f.upper() for h in HINTS.get(lic, [lic.upper()]))]
            picked += match if match else []
        generic = [f for f in files if re.fullmatch(r"(LICEN[CS]E|COPYING)(\.(md|txt))?", f, re.I)]
        if not picked:
            picked = generic or files
        # Keep NOTICE files: Apache-2.0 requires passing them on.
        picked += [f for f in files if f.upper().startswith("NOTICE") and f not in picked]
    texts = [read(os.path.join(root, f)) for f in dict.fromkeys(picked)]
    return " AND ".join(dict.fromkeys(used)) if used else expr, texts


def cargo_crates():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=CRATE, check=True, capture_output=True, text=True).stdout)
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = next(p["id"] for p in meta["packages"] if p["name"] == "dwell-net")
    seen, stack = set(), [root]
    while stack:
        for dep in nodes[stack.pop()]["deps"]:
            normal = any(k["kind"] is None for k in dep["dep_kinds"])
            if normal and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                stack.append(dep["pkg"])
    return sorted((pkgs[i] for i in seen), key=lambda p: (p["name"], p["version"]))


def generate():
    with open(os.path.join(HERE, "third_party.json"), encoding="utf-8") as f:
        components = json.load(f)["components"]
    entries = []  # (label, license, used_in, url, [texts])
    for c in components:
        entries.append((f"{c['name']} {c['version']}", c["license"], c["used_in"], c["url"],
                        [read(os.path.join(HERE, "texts", c["text"]))]))
    for pkg in cargo_crates():
        lic, texts = crate_texts(pkg)
        url = pkg.get("repository") or f"https://crates.io/crates/{pkg['name']}"
        entries.append((f"{pkg['name']} {pkg['version']}", lic, "native server (Rust crate)",
                        url, texts))

    groups = {}  # text -> [labels], in first-seen order
    for label, _, _, _, texts in entries:
        for text in texts:
            groups.setdefault(text, []).append(label)

    out = [
        "THIRD-PARTY NOTICES",
        "",
        "Dwell's builds include the third-party components listed below, each under its own",
        "license. Dwell itself is not open source: see LICENSE.",
        "",
        "Generated by shared/licenses/gen.py from shared/licenses/third_party.json and",
        "server/net/wt/Cargo.lock. Do not edit by hand.",
        "",
        "=" * 79,
        "COMPONENTS",
        "=" * 79,
        "",
    ]
    for label, lic, used_in, url, _ in entries:
        out += [label, f"  License: {lic}", f"  Used in: {used_in}", f"  Source:  {url}", ""]
    out += ["=" * 79, "LICENSE TEXTS", "=" * 79, ""]
    for text, labels in groups.items():
        out += ["-" * 79, "Applies to: " + ", ".join(labels), "-" * 79, "", text]
    return "\n".join(out).rstrip() + "\n"


def main():
    text = generate()
    if "--check" in sys.argv[1:]:
        current = open(OUT, encoding="utf-8").read() if os.path.exists(OUT) else ""
        if current != text:
            sys.exit("THIRD_PARTY_NOTICES is out of date: run python3 shared/licenses/gen.py")
        return
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"wrote {os.path.relpath(OUT, ROOT)} ({len(text) // 1024} KB)")


if __name__ == "__main__":
    main()
