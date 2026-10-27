# [edenfs] bytes.torn-read

Task key: `sha256:6bea56005b9fe4ef6aa6b3bbfe1c30620ad6196edd824b48d6fb49224331b36b` (seeds [7])

Daemon: `abababababababababababababababababababababababababababababababab` built from `cccccccccccccccccccccccccccccccccccccccc`

## Observed in seed 7 (reject)

RunSpec `sha256:d1642bd844d5f10c6c77fa47d2699274022e9f25880dbe85b5e8bbdf0c6e9f92` in `<root>/artifacts/seed-7`

- `bytes.torn-read` (undetermined) on `path:tree/registers/r0.reg` at `r`: bytes/fuse: torn-read: r returned 64 bytes on tree/registers/r0.reg that splice two candidate values and equal neither; witnesses `channel=fuse`, `read-action=r`, `spliced=d430b702fb6c125ef89070ab25c6a997143237b4f3d7527b853cf9446496abc5`, `spliced=d74873eaa0dab90ec0cd324e50e0ef57d8583117df1f9eddea2f6a5cc608b116`

## Properties

- `attributes`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `bytes`: reject: satisfied 1, violated 1, not-evaluated 0
- `checkout`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `core.activity`: accept: satisfied 1, violated 0, not-evaluated 0
- `core.daemon-exit`: accept: satisfied 1, violated 0, not-evaluated 0
- `core.input-legality`: accept: satisfied 2, violated 0, not-evaluated 0
- `core.result-schema`: accept: satisfied 2, violated 0, not-evaluated 0
- `cross-channel`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `differential`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `existence`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `fd`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `journal`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `listing`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `manifest`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `namespace`: accept: satisfied 1, violated 0, not-evaluated 0
- `readdir`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `result-shape`: not-evaluated: satisfied 0, violated 0, not-evaluated 0
- `status`: not-evaluated: satisfied 0, violated 0, not-evaluated 0

## Related keys

- none

## Replay

```
buck2 run @fbcode//mode/opt fbcode//eden/integration/correctness/exploratory:explore -- replay --target edenfs --run <root>/artifacts/seed-7 --artifacts <new dir>
```

## Error set

```
[
  [
    "bytes.torn-read",
    "torn-read",
    [
      [
        "channel",
        "fuse"
      ]
    ]
  ]
]
```
