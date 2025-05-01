#!/usr/bin/env python3
from dataclasses import dataclass
from io import StringIO
import pprint
import subprocess as sp
from pathlib import Path
import tempfile
import argparse
import re
from typing import cast


here = Path(__file__).parent


def generate_bindgen_bindings() -> str:
    """
    Forcefully take bindgen's bindings away from it,
    no matter how much it does not want us to do it with our API.
    """

    BINDGEN_PLEASE_GENERATE_STUBS = """
    #[wasm_bindgen::prelude::wasm_bindgen]
    pub fn a() {
        unsafe {
            crate::rasterize::wgpu::Rasterizer::new(
                std::hint::black_box(std::mem::transmute::<_, &wgpu::Device>(&0).clone()),
                std::hint::black_box(std::mem::transmute::<_, &wgpu::Queue>(&0).clone()),
            );
            sbr_renderer_render(
                std::hint::black_box(std::ptr::null_mut()),
                std::hint::black_box(std::ptr::null()),
                std::hint::black_box(std::ptr::null()),
                0,
                std::hint::black_box(std::ptr::null_mut()),
                1000,
                1000,
                1000,
            );
        }
    }
    """

    worktree = here / "bindgen-stub-worktree"
    sp.check_call(["git", "worktree", "add", "-d", worktree])

    try:
        (worktree / "text-sys/build.rs").unlink()
        with (worktree / "src/capi.rs").open("a") as f:
            f.write(BINDGEN_PLEASE_GENERATE_STUBS)
        sp.check_call(
            ["cargo", "add", "wasm-bindgen"],
            cwd=worktree,
        )
        sp.check_call(
            [
                "cargo",
                "build",
                "--target",
                "wasm32-unknown-unknown",
                "--features",
                "wgpu",
            ],
            cwd=worktree,
        )
        with tempfile.TemporaryDirectory() as dir:
            sp.check_call(
                [
                    "wasm-bindgen",
                    "--target",
                    "web",
                    "target/wasm32-unknown-unknown/debug/subrandr.wasm",
                    "--out-dir",
                    dir,
                ],
                cwd=worktree,
            )
            return (Path(dir) / "subrandr.js").read_text()
    finally:
        sp.check_call(["git", "worktree", "remove", "--force", worktree])


@dataclass
class Bindings:
    # utility function bindgen implements that we must provide ourselves
    # this is because we actually already implement stuff like
    # getStringFromWasm0 in our module interface code anyway
    imports: list[str]
    enums: dict[str, str]
    bindings: dict[str, tuple[str, str]]


def extract_bindgen_funcs(bindings: str) -> Bindings:
    """
    Extract all relevant binding information from bindgen's bindings,
    so that they can be readded into our imports and we can use
    bindgen on wasi no matter how much bindgen doesn't want us to.
    """

    result = Bindings(imports=[], enums={}, bindings={})

    for m in re.finditer(r"function\s+([a-zA-Z0-9]+)\(.*?\)", bindings):
        # this is the dummy function we created above to force bindgen to
        # generate everything we want
        if m.group(1) == "a":
            continue

        # we truly do not care
        if m.group(1) == "initSync":
            continue

        result.imports.append(m.group(1))

    for m in re.finditer(
        r"const (__wbindgen_enum_[a-zA-Z0-9_]+) = (\[.*?\])", bindings
    ):
        result.enums[cast(str, m.group(1))] = cast(str, m.group(2))

    for m in re.finditer(
        r"imports.wbg.([_a-zA-Z0-9]+)\s*=\s*function\s*\((.*?)\)\s+{", bindings
    ):
        name, args = m.group(1), m.group(2)
        # take a block
        depth = 1
        i = m.end() + 1
        while i < len(bindings):
            if bindings[i] == "{":
                depth += 1
            elif bindings[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        body = bindings[m.end() : i]

        result.bindings[name] = (args, body)

    return result


def write_bindgen_module(bindings: Bindings) -> str:
    writer = StringIO()

    for name, value in bindings.enums.items():
        writer.write(f"const {name} = {value};\n")

    writer.write("// inputs must contain: ")
    writer.write(", ".join(bindings.imports))
    writer.write("\n")
    writer.write("function makeBindgenImports(inputs) {\n")
    for name in bindings.imports:
        writer.write(
            f"    if (!inputs.{name}) throw Error('bindgen input missing {name}')\n"
        )
        writer.write(f"    const {name} = inputs.{name};\n")

    writer.write("    const out = {}\n")
    for name, (args, body) in bindings.bindings.items():
        writer.write(f"    out.{name} = function({args}) {{{body}}}\n")
    writer.write("    return out\n")
    writer.write("}\n")

    return writer.getvalue()


parser = argparse.ArgumentParser()
parser.add_argument(
    "--force-rerun-bindgen",
    action="store_true",
    help="Rerun wasm-bindgen step even if bindings already exist",
)
args = parser.parse_args()

output_dir = here / "bindgen"
output_dir.mkdir(exist_ok=True)
raw_bindings_path = output_dir / "wasm_bindgen.js"
if not raw_bindings_path.exists() or args.force_rerun_bindgen:
    raw_bindings_path.write_text(generate_bindgen_bindings())

(output_dir / "output.js").write_text(
    write_bindgen_module(extract_bindgen_funcs(raw_bindings_path.read_text()))
)
