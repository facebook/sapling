# EdenFS stats Counters and Duration

There are EdenFS stats all over eden layers. You can check the stats value with
any of the followings way:

- You can see the list of all the EdenFS counters and their current values by
  running

```
$ eden debug thrift getCounters --json
```

- You can limit the output of this command by using getRegexCounters

```
eden debug thrift getRegexCounters "^inodemap.*" --json
```

Also the following eden commands show some stats

- `eden rage` : divide them into EdenFS Counters and Thrift Counters
- `eden stats` : show some of the inodes dynamic counters
- `eden top` : show some live and pending dynamic counters for fuse and object
  requests

There are three set of stats in Eden:

1. [Stats which are listed in EdenStats.h - Most common](./EdenStats.md)
2. [Dynamic Counters that are registered with a callback. Usually in EdenServer.cpp](./DynamicStats.md)
3. [EdenFS exposed Sapling counters](./EdenExposedSaplingCounters.md)

## Testing stats

There are some integration tests for the availability and the value of the
stats. e.g.

- [stats_test.py](/fbcode/eden/integration/stats_test.py) For some of
  EdenStats.h and dynamic counters
- [filteredhg_test.py](/fbcode/eden/integration/hg/files_test.py) for FilteredFS
  Sapling counters
- Also, there is .t test for Sapling counters. e.g.
  [test-remotefilelog-prefetch.t](/fbcode/eden/scm/tests/test-remotefilelog-prefetch.t)
