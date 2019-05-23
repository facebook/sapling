// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

/// Abstracted measured result for benchmark.
pub trait Measure: Sized {
    /// Expected return value from the function being measured.
    /// Usually just `()`.
    type FuncOutput;

    /// Measure a function.
    fn measure(impl FnMut() -> Self::FuncOutput) -> Result<Self, String>;

    /// Merge with another measure result.
    fn merge(self, _rhs: Self) -> Self;

    /// Return true if another measurement is needed.
    fn need_more(&self) -> bool;

    /// Convert to human-readable result.
    fn to_string(&self) -> String;
}

/// Measure the best wall clock time quickly.
pub struct WallClock {
    pub best: f64,
    pub total: f64,
    pub count: usize,
}

/// Measure bytes. Run only once.
pub struct Bytes(pub u64);

impl Measure for WallClock {
    type FuncOutput = ();

    fn measure(mut func: impl FnMut()) -> Result<Self, String> {
        use std::time::SystemTime;
        let now = SystemTime::now();
        func();
        let elapsed = now.elapsed().unwrap();
        let seconds = elapsed.as_secs() as f64 + elapsed.subsec_nanos() as f64 * 1e-9;
        Ok(Self {
            best: seconds,
            total: seconds,
            count: 1,
        })
    }

    fn merge(self, rhs: Self) -> Self {
        Self {
            best: self.best.min(rhs.best),
            total: self.total + rhs.total,
            count: self.count + rhs.count,
        }
    }

    fn need_more(&self) -> bool {
        // Run for 5 seconds, or 40 times at most.
        self.count < 40 && self.total < 5.0
    }

    fn to_string(&self) -> String {
        let value = self.best;
        if value <= 1.0 {
            format!("{:7.3} ms", value * 1000.0)
        } else {
            format!("{:7.3} s ", value)
        }
    }
}

impl Measure for Bytes {
    type FuncOutput = u64;

    fn measure(mut func: impl FnMut() -> Self::FuncOutput) -> Result<Self, String> {
        Ok(Self(func()))
    }

    fn merge(self, _rhs: Self) -> Self {
        Self(self.0)
    }

    fn need_more(&self) -> bool {
        false
    }

    fn to_string(&self) -> String {
        let value = self.0;
        if value < 1_000 {
            format!("{:7} B ", value)
        } else if value < 1_000_000 {
            format!("{:7.3} KB", (value as f64) / 1000.0)
        } else {
            format!("{:7.3} MB", (value as f64) / 1000_000.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes() {
        let measured = Bytes::measure(|| 10).unwrap();
        assert_eq!(measured.to_string(), "     10 B ");
    }
}
