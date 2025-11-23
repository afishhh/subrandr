use super::common::*;

check_test! {
    name = empty,
    size = (16, 16),
    inline.ahem {}
}

check_test! {
    name = empty_string,
    size = (16, 16),
    inline.ahem {
        text ""
    }
}

check_test! {
    name = newlines,
    size = (16, 48),
    inline.ahem {
        text "\n\n"
    }
}

check_test! {
    name = chars_and_newlines,
    size = (16, 96),
    inline.ahem {
        text "\na\n\nb\n"
    }
}
