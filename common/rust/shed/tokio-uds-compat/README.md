# tokio-uds-compat

This crate provides a compatible layer for using Tokio's UNIX domain socket
across UNIX and Windows.

This is achieved by lifting async-io's wepoll and bridging it with Tokio's async
traits. As a result, one extra thread will be spawned to host async-io's
runtime. We cannot directly use mio as its UNIX domain socket support isn't
complete yet. For now we sacrifice an extra thread for usability on Windows.

## Example

You can run the provided example with:

```
cargo run --example server
```

and you can test it with curl:

```
$ curl -vvvv --unix-socket '%TEMP%\hello.sock' http://localhost/
```
