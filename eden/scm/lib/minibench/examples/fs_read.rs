/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::thread;

use futures::future::join_all;
use futures::stream;
use futures::StreamExt;
use minibench::bench;
use minibench::elapsed;

const FILE: &str = "/bin/sleep"; // 38KB

fn do_std_fs_read() {
    std::fs::read(FILE).unwrap();
}

async fn do_tokio_fs_read() {
    tokio::fs::read(FILE).await.unwrap();
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    const N: usize = 10000;

    bench("tasks.join_all std::fs::read", || {
        elapsed(|| {
            let tasks: Vec<_> = (0..N)
                .map(|_| {
                    runtime.spawn_blocking(move || {
                        do_std_fs_read();
                    })
                })
                .collect();
            runtime.block_on(join_all(tasks));
        })
    });

    bench("tasks.join_all tokio::fs::read", || {
        elapsed(|| {
            let tasks: Vec<_> = (0..N).map(|_| do_tokio_fs_read()).collect();
            runtime.block_on(join_all(tasks));
        })
    });

    let sizes = [1, 2, 5, 10, 20];
    for x in sizes {
        bench(
            format!("stream.buffer_unordered({x}) std::fs::read"),
            || {
                elapsed(|| {
                    let mut stream = StreamExt::buffer_unordered(
                        stream::iter(0..N).map(|_| {
                            runtime.spawn_blocking(move || {
                                do_std_fs_read();
                            })
                        }),
                        x,
                    );
                    runtime.block_on(async move { while let Some(_) = stream.next().await {} });
                })
            },
        );
    }

    for x in sizes {
        bench(
            format!("stream.buffer_unordered({x}) tokio::fs::read"),
            || {
                elapsed(|| {
                    let mut stream = StreamExt::buffer_unordered(
                        stream::iter(0..N).map(|_| do_tokio_fs_read()),
                        x,
                    );
                    runtime.block_on(async move { while (stream.next().await).is_some() {} });
                })
            },
        );
    }

    for x in sizes {
        bench(format!("{x} threaded std::fs::read via channel"), || {
            elapsed(|| {
                let (send, recv) = crossbeam::channel::unbounded();
                let threads: Vec<_> = (0..x)
                    .map(|_| {
                        let recv = recv.clone();
                        thread::spawn(move || {
                            while recv.recv().is_ok() {
                                do_std_fs_read();
                            }
                        })
                    })
                    .collect();
                drop(recv);
                for _ in 0..N {
                    send.send(()).unwrap();
                }
                drop(send);
                for t in threads {
                    t.join().unwrap();
                }
            })
        });
    }
}
