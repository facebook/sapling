/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod procfs;
pub use self::procfs::*;

/// Abstracted measured result for benchmark.
pub trait Measure: Sized {
    /// Expected return value from the function being measured.
    /// Usually just `()`.
    type FuncOutput;

    /// Measure a function.
    fn measure(func: impl FnMut() -> Self::FuncOutput) -> Result<Self, String>;

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

/// Measure both.
///
/// A will be measured in an inner loop. B will be measured in the outer loop.
/// `A::FuncOutput` must be `()`.
pub struct Both<A, B>(Result<A, String>, Result<B, String>);

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

impl<D: Default, A: Measure<FuncOutput = ()>, B: Measure<FuncOutput = D>> Measure for Both<A, B> {
    type FuncOutput = B::FuncOutput;

    fn measure(mut func: impl FnMut() -> Self::FuncOutput) -> Result<Self, String> {
        let mut a = None;
        let mut a_run = false;
        let mut b_run = false;
        let mut b = B::measure(|| {
            b_run = true;
            let mut v = None;
            a = Some(A::measure(|| {
                a_run = true;
                v = Some(func());
            }));
            v.unwrap_or_default()
        });

        if !b_run {
            // B failed without running the function. Try just running A.
            a = Some(A::measure(|| {
                func();
            }));
        } else if !a_run {
            // A failed without running the function.
            // B's result is meaningless. Re-run B.
            b = B::measure(|| func());
        }

        Ok(Self(a.unwrap(), b))
    }

    fn merge(self, rhs: Self) -> Self {
        let (a1, b1) = self.into();
        let (a2, b2) = rhs.into();
        let a = match (a1, a2) {
            (Ok(a1), Ok(a2)) => Ok(a1.merge(a2)),
            (Ok(a1), Err(_)) => Ok(a1),
            (Err(_), Ok(a2)) => Ok(a2),
            (Err(a1), Err(_)) => Err(a1),
        };
        let b = match (b1, b2) {
            (Ok(b1), Ok(b2)) => Ok(b1.merge(b2)),
            (Ok(b1), Err(_)) => Ok(b1),
            (Err(_), Ok(b2)) => Ok(b2),
            (Err(b1), Err(_)) => Err(b1),
        };
        Self(a, b)
    }

    fn need_more(&self) -> bool {
        match (&self.0, &self.1) {
            (Ok(a), Ok(b)) => a.need_more() || b.need_more(),
            _ => false,
        }
    }

    fn to_string(&self) -> String {
        let a = match &self.0 {
            Ok(a) => a.to_string(),
            Err(a) => a.clone(),
        };
        let b = match &self.1 {
            Ok(b) => b.to_string(),
            Err(b) => b.clone(),
        };
        format!("{} {}", a, b)
    }
}

impl<A, B> Into<(Result<A, String>, Result<B, String>)> for Both<A, B> {
    fn into(self) -> (Result<A, String>, Result<B, String>) {
        (self.0, self.1)
    }
}

// For ad-hoc messages
impl Measure for String {
    type FuncOutput = String;

    fn measure(mut func: impl FnMut() -> Self::FuncOutput) -> Result<Self, String> {
        Ok(func())
    }

    fn merge(self, _rhs: Self) -> Self {
        self
    }

    fn need_more(&self) -> bool {
        false
    }

    fn to_string(&self) -> String {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy();
    struct DummyError();
    impl Measure for Dummy {
        type FuncOutput = ();

        fn measure(mut func: impl FnMut() -> Self::FuncOutput) -> Result<Self, String> {
            func();
            Ok(Self())
        }

        fn merge(self, _rhs: Self) -> Self {
            Self()
        }

        fn need_more(&self) -> bool {
            false
        }

        fn to_string(&self) -> String {
            "dummy".to_string()
        }
    }

    impl Measure for DummyError {
        type FuncOutput = ();

        fn measure(mut _func: impl FnMut() -> Self::FuncOutput) -> Result<Self, String> {
            Err("unsupported".to_string())
        }

        fn merge(self, _rhs: Self) -> Self {
            Self()
        }

        fn need_more(&self) -> bool {
            false
        }

        fn to_string(&self) -> String {
            unreachable!()
        }
    }

    #[test]
    fn test_bytes() {
        let measured = Bytes::measure(|| 10).unwrap();
        assert_eq!(measured.to_string(), "     10 B ");
    }

    #[test]
    fn test_string() {
        let measured = String::measure(|| "abc def".to_string()).unwrap();
        assert_eq!(measured, "abc def");
    }

    #[test]
    fn test_both() {
        let measured = Both::<Dummy, Bytes>::measure(|| 10).unwrap();
        assert!(!measured.need_more());
        assert_eq!(measured.to_string(), "dummy      10 B ");
    }

    #[test]
    fn test_chained_both() {
        let measured = Both::<Both<Dummy, Dummy>, Both<Dummy, Bytes>>::measure(|| 10).unwrap();
        assert!(!measured.need_more());
        assert_eq!(measured.to_string(), "dummy dummy dummy      10 B ");
    }

    #[test]
    fn test_both_errors() {
        let measured = Both::<Dummy, DummyError>::measure(|| ()).unwrap();
        assert!(!measured.need_more());
        assert_eq!(measured.to_string(), "dummy unsupported");

        let measured = Both::<DummyError, Dummy>::measure(|| ()).unwrap();
        assert!(!measured.need_more());
        assert_eq!(measured.to_string(), "unsupported dummy");

        let measured = Both::<DummyError, DummyError>::measure(|| ()).unwrap();
        assert!(!measured.need_more());
        assert_eq!(measured.to_string(), "unsupported unsupported");
    }
}
