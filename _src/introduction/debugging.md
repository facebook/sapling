---
sidebar_position: 60
---

import Tags from '@theme/Tabs';
import TabItem from '@theme/TabItem';

# Debugging Sapling SCM

## Logging

Set the `ui.debug` setting to `true` to get debug logging of the Python
interface surface:

```
sl config --user ui.debug true
```

The `tracing`-based logging can be configured with the `EDENSCM_LOG`
environment variable. It follows the
[format of tracing-subscriber](https://docs.rs/tracing-subscriber/0.3.16/tracing_subscriber/struct.EnvFilter.html).

Some examples include:
- `EDENSCM_LOG=debug` to get debug logs for every component.
- `EDENSCM_LOG=warn,configparser=debug` to log `configparser` at debug level,
  and everything else at the warning level.

## Editing Python code

Sapling uses a background service, which can sometimes get in the way of
hacking on Python code. You can run with `CHGDISABLE=1` to stop this.

## Editing Rust code

The Rust components are normally built in release mode, but you can switch to
building in debug mode with `RUST_DEBUG=1`, which will build faster:

```
eden/scm$ make RUST_DEBUG=1 oss
```
