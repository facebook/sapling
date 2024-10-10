use std::str::Lines;

/// Pad `lines` to have `min_count` lines at least.
pub(crate) fn pad_lines<'a>(lines: Lines<'a>, min_count: usize) -> PadLines<'a> {
    PadLines {
        lines,
        index: 0,
        min_count,
    }
}

pub(crate) struct PadLines<'a> {
    lines: Lines<'a>,
    index: usize,
    min_count: usize,
}

impl<'a> Iterator for PadLines<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self.lines.next() {
            Some(line) => {
                self.index += 1;
                Some(line)
            }
            None => {
                if self.index < self.min_count {
                    self.index += 1;
                    Some("")
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_lines() {
        let s = "a\nb\n";
        for i in 0..=2 {
            let p = pad_lines(s.lines(), i);
            assert_eq!(p.concat(), ["a", "b"]);
        }
        let p = pad_lines(s.lines(), 3);
        assert_eq!(p.concat(), ["a", "b", ""]);
        let p = pad_lines(s.lines(), 5);
        assert_eq!(p.concat(), ["a", "b", "", "", ""]);
    }

    impl<'a> PadLines<'a> {
        fn concat(self) -> Vec<&'a str> {
            self.collect::<Vec<_>>()
        }
    }
}
