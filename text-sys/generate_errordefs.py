#!/usr/bin/env python3
# pyright: basic
#
# TODO: Replace this script by using freetype's C macros instead
#
import subprocess as sp
import re
from pathlib import Path

include_dir = (
    sp.check_output(["pkg-config", "freetype2", "--variable=includedir"])
    .decode()
    .strip()
)
ft_errdef_h = f"{include_dir}/freetype/fterrdef.h"


errdef_re = re.compile(
    r'FT_ERRORDEF_\(\s*([A-Za-z_]+),\s*([0-9x]+),\s*"([^"]+)"', re.DOTALL
)

print("pub const FREETYPE_ERRORS: &[(FT_Error, &str)] = &[")

for _ident, code, msg in errdef_re.findall(Path(ft_errdef_h).read_text()):
    print(f'\t({code}, "{msg}"),')

print("];")
