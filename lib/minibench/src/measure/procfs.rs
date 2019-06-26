// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Measurement based on procfs (/proc)

use super::{Bytes, Measure};

/// Measure IO.
pub struct IO {
    rchar: u64,
    wchar: u64,
}

#[derive(Debug)]
struct IOSnapshot {
    rchar: u64,
    wchar: u64,
    rchar_overhead: u64,
}

fn read_io() -> Result<IOSnapshot, String> {
    let io_str = std::fs::read_to_string("/proc/self/io").map_err(|_| "(no data)".to_string())?;
    let mut rchar: u64 = 0;
    let mut wchar: u64 = 0;
    const RCHAR_PREFIX: &str = "rchar: ";
    const WCHAR_PREFIX: &str = "wchar: ";
    for line in io_str.lines() {
        if line.starts_with(RCHAR_PREFIX) {
            rchar += line[RCHAR_PREFIX.len()..]
                .parse::<u64>()
                .map_err(|_| "unexpected rchar".to_string())?;
        } else if line.starts_with(WCHAR_PREFIX) {
            wchar += line[WCHAR_PREFIX.len()..]
                .parse::<u64>()
                .map_err(|_| "unexpected wchar".to_string())?;
        }
    }
    // Reading io has side effect on rchar. Record it.
    let rchar_overhead = io_str.len() as u64;
    Ok(IOSnapshot {
        rchar,
        wchar,
        rchar_overhead,
    })
}

impl Measure for IO {
    type FuncOutput = ();

    fn measure(mut func: impl FnMut()) -> Result<Self, String> {
        let before = read_io()?;
        func();
        let after = read_io()?;
        let rchar = after.rchar - before.rchar - before.rchar_overhead;
        let wchar = after.wchar - before.wchar;
        Ok(Self { rchar, wchar })
    }

    fn merge(self, rhs: Self) -> Self {
        Self {
            rchar: self.rchar.max(rhs.rchar),
            wchar: self.wchar.max(rhs.wchar),
        }
    }

    fn need_more(&self) -> bool {
        false
    }

    fn to_string(&self) -> String {
        format!(
            "{}/{}",
            Bytes(self.rchar).to_string(),
            Bytes(self.wchar).to_string()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io() {
        if let Ok(io) = IO::measure(|| {}) {
            // The test runner can run things in multi-thread and breaks the measurement here :/
            if io.rchar == 0 && io.wchar == 0 {
                assert_eq!(io.to_string(), "      0 B /      0 B ");
            }
        }
    }
}
