/// Same `classify` body as `testdata/crap_untested`, with branch coverage.
pub fn classify(n: i32, flag: bool, mode: u8) -> &'static str {
    if n < 0 {
        return "neg";
    }
    if n == 0 {
        return "zero";
    }
    if flag && n > 100 {
        return "bigflag";
    }
    if flag || n > 50 {
        return "mid";
    }
    match mode {
        0 => "a",
        1 => "b",
        2 => "c",
        _ => "d",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_branches() {
        assert_eq!(classify(-1, false, 0), "neg");
        assert_eq!(classify(0, false, 0), "zero");
        assert_eq!(classify(101, true, 0), "bigflag");
        assert_eq!(classify(10, true, 0), "mid");
        assert_eq!(classify(51, false, 0), "mid");
        assert_eq!(classify(10, false, 0), "a");
        assert_eq!(classify(10, false, 1), "b");
        assert_eq!(classify(10, false, 2), "c");
        assert_eq!(classify(10, false, 9), "d");
    }
}
