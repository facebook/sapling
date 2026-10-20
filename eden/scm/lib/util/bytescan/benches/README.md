# bytescan benchmarks

Compares `bytescan` ("new") with the std expressions it replaced ("old"):
`<[u8]>::contains(&0)` for `is_binary`, and `split(|c| c == &b'\n').count()`
for `count_newlines`.

## Running

```
buck2 run @mode/opt fbcode//eden/scm/lib/util/bytescan/benches:bytescan-benches
```

Pass substrings to run a subset of rows, e.g. `-- is_binary`, `-- mid-NUL` or
`-- 64MiB`.

## Results

On Linux, small-buffer `new` times vary by up to ~35% between launches of the
binary. On macOS they are stable to within ~5%.

### Linux x86_64, AMD EPYC Genoa (AVX-512), 2026-10-14

```
Time per 256 MiB scanned (best of up to 40 runs).

function        input    buffer        old        new  speedup

is_binary       no-NUL     4KiB   15.96 ms    1.60 ms    10.0x
is_binary       no-NUL    64KiB   14.97 ms    1.96 ms     7.7x
is_binary       no-NUL     1MiB   15.05 ms    2.70 ms     5.6x
is_binary       no-NUL    64MiB   15.12 ms    4.79 ms     3.2x

is_binary       mid-NUL    4KiB    7.55 ms    0.90 ms     8.3x
is_binary       mid-NUL   64KiB    7.49 ms    0.89 ms     8.4x
is_binary       mid-NUL    1MiB    7.44 ms    0.95 ms     7.9x
is_binary       mid-NUL   64MiB    7.60 ms    2.03 ms     3.7x

count_newlines  no-NUL     4KiB   94.56 ms    2.52 ms    37.5x
count_newlines  no-NUL    64KiB   92.82 ms    3.81 ms    24.4x
count_newlines  no-NUL     1MiB   94.48 ms    4.01 ms    23.5x
count_newlines  no-NUL    64MiB   96.31 ms    5.28 ms    18.2x
```

### macOS arm64, Apple M5 Max, 2026-10-14

```
Time per 256 MiB scanned (best of up to 40 runs).

function        input    buffer        old        new  speedup

is_binary       no-NUL     4KiB    6.58 ms    2.24 ms     2.9x
is_binary       no-NUL    64KiB    6.09 ms    2.19 ms     2.8x
is_binary       no-NUL     1MiB    6.08 ms    2.19 ms     2.8x
is_binary       no-NUL    64MiB    6.23 ms    2.79 ms     2.2x

is_binary       mid-NUL    4KiB    3.55 ms    1.16 ms     3.1x
is_binary       mid-NUL   64KiB    3.01 ms    1.14 ms     2.6x
is_binary       mid-NUL    1MiB    2.99 ms    1.10 ms     2.7x
is_binary       mid-NUL   64MiB    3.14 ms    1.45 ms     2.2x

count_newlines  no-NUL     4KiB   64.56 ms    1.96 ms    33.0x
count_newlines  no-NUL    64KiB   63.87 ms    1.96 ms    32.6x
count_newlines  no-NUL     1MiB   63.87 ms    2.00 ms    31.9x
count_newlines  no-NUL    64MiB   63.88 ms    2.86 ms    22.3x
```
