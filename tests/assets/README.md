This directory contains pointer files to assets required by the layout test suite.

In order to be able to run `-F _layout_tests` tests, fetch the real files with `cargo xxtask ptr pull tests/assets/*.ptr`.

Some of these assets have a license different from this repository's default license, and are provided under their respective licenses below:
- `Ahem.ttf`, from https://www.w3.org/Style/CSS/Test/Fonts/, in the public domain or under a CC0 declaration (see https://www.w3.org/Style/CSS/Test/Fonts/Ahem/COPYING for details)
- `NotoSansArabic-Regular.ttf`, from https://fonts.google.com/noto/specimen/Noto+Sans+Arabic, under the SIL Open Font License (see `OFL-arabic.txt` for details)
- `NotoSerif-Regular.ttf`, from https://fonts.google.com/noto/specimen/Noto+Serif, under the SIL Open Font License (see `OFL-serif.txt` for details)
- `NotoSansJP-Regular.ttf`, from https://fonts.google.com/noto/specimen/Noto+Sans+JP, under the SIL Open Font License (see `OFL-jp.txt` for details)
- `NotoColorEmoji-Subset.ttf`, from https://github.com/googlefonts/noto-emoji/blob/v2.051/fonts/NotoColorEmoji.ttf, under the SIL Open Font License (see `OFL-emoji.txt` for details), subset to only include "😀😭🧱⭕"
