/// Branchy classifier. Keep this body identical to `testdata/crap_tested`.
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
