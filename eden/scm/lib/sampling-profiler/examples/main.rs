/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cpython_ext::cpython::*;
use sampling_profiler::BacktraceCollector;
use sampling_profiler::Profiler;

fn native_fib(py: Python, n: u64) -> PyResult<u64> {
    py.allow_threads(|| std::thread::sleep(Duration::from_millis(10)));
    let v = if n <= 1 {
        n
    } else {
        let py_fib = py.eval("py_fib", None, None)?;
        let v1: u64 = py_fib.call(py, (n - 1,), None)?.extract(py)?;
        let v2: u64 = py_fib.call(py, (n - 2,), None)?.extract(py)?;
        v1 + v2
    };
    Ok(v)
}

fn do_some_work(py: Python) {
    let code = r#"
import time, sys
def py_fib(n):
    time.sleep(0.01)
    if n <= 1:
        return n
    return sys.native_fib(n - 1) + sys.native_fib(n - 2)

print(f"{py_fib(11)=}")
"#;
    let sys = py.import("sys").unwrap();
    sys.add(py, "native_fib", py_fn!(py, native_fib(n: u64)))
        .unwrap();
    py.run(code, None, None).unwrap();
}

fn print_traceback(bt: &[String]) {
    static TICK: AtomicUsize = AtomicUsize::new(0);
    println!(
        "Traceback #{} ({} frames, most recent call first):",
        TICK.fetch_add(1, Ordering::AcqRel) + 1,
        bt.len()
    );
    for name in bt {
        println!("  {}", name);
    }
}

fn is_boring(name: &str) -> bool {
    name.contains("cpython[") || name == "__rust_try"
}

fn main() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    println!(
        "Python frame resolution support: {:?}",
        &*backtrace_python::SUPPORTED_INFO
    );

    backtrace_python::init();

    let collector = Arc::new(Mutex::new(BacktraceCollector::default()));
    let profiler = Profiler::new(
        Duration::from_millis(500),
        Box::new({
            let collector = collector.clone();
            move |bt| {
                print_traceback(bt);
                let mut bt: Vec<String> = bt.iter().filter(|n| !is_boring(n)).cloned().collect();
                bt.reverse();
                collector.lock().unwrap().push_backtrace(bt);
            }
        }),
    )
    .unwrap();

    let collector2 = Arc::new(Mutex::new(BacktraceCollector::default()));
    let profiler2 = Profiler::new(
        Duration::from_millis(50),
        Box::new({
            let collector = collector2.clone();
            move |bt| {
                let mut bt: Vec<String> = bt.iter().filter(|n| !is_boring(n)).cloned().collect();
                bt.reverse();
                collector.lock().unwrap().push_backtrace(bt);
            }
        }),
    )
    .unwrap();

    do_some_work(py);
    drop(profiler);
    drop(profiler2);

    let summary = collector.lock().unwrap().ascii_summary();
    println!("\nASCII tree summary (Profiler 1 at 2hz):\n{}", summary);

    let summary = collector2.lock().unwrap().ascii_summary();
    println!("\nASCII tree summary (Profiler 2 at 20hz):\n{}", summary);
}
