#!/usr/bin/env python3

from io import StringIO
from pathlib import Path
import re


build_dir = Path(__file__).parent / "build"
build_dir.mkdir(exist_ok=True)

writer = StringIO()
tswriter = StringIO()

writer.write("export function makeBindgenImports(wasm) {\n")

depth = 0
skip_all = True
for line in (build_dir / "bindgen/subrandr.js").read_text().splitlines():
    if depth == 0:
        skip_all = False
        if line.lstrip().startswith("import"):
            continue

        if line.lstrip().startswith("export"):
            skip_all = True

        if re.match(r"let\s+wasm", line):
            continue

        if re.match(r"async\s+function", line) or re.search(
            r"function\s+(initSync|__wbg_init_memory|__wbg_finalize_init)", line
        ):
            skip_all = True

    if re.search(r"__wbg_star\d+", line):
        continue

    depth += line.count("{") - line.count("}")

    if not skip_all:
        writer.write(f"    {line}\n")

writer.write("    return __wbg_get_imports().wbg;\n")
writer.write("}\n")

tswriter.write("export function makeBindgenImports(wasmExports: any): any;\n")

(build_dir / "output.js").write_text(writer.getvalue())
(build_dir / "output.d.ts").write_text(tswriter.getvalue())
