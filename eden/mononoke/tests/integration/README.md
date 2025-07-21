# Mononoke Integration Tests

Mononoke's integration tests run using a fork of [Mercurial's run-tests test
framework](https://wiki.mercurial-scm.org/WritingTests), which is orchestrated
using a wrapper to make it more friendly to TestPilot and provide some added
functionality, such as wiring up dependencies and / or setting up an ephemeral
MySQL shard.

Integration tests are grouped into small targets so that a minimal set of
dependencies are built for each test.

## TL;DR: Running one test with run-test.sh

First, find the target responsible for the test you want to run by checking in
the TARGETS file.

To quickly run a single test use
[run-test.sh](https://www.internalfb.com/code/fbsource/fbcode/eden/mononoke/tests/integration/run-test.sh)
helper script:

```sh
./run-test.sh TEST.t
```

Which behind the scenes identifies the right buck target responsible for the
test and runs: ```buck2 run TARGET -- TEST.t```.

### Interactive mode

Use `--interactive` / `-i` when running your tests this way in order to accept (or reject)
changes to your `.t` files, or `--keep-tmpdir` to be able to see the files edited
by your test after it runs.

```sh
./run-test.sh TEST.t -i
```

But! Keep reading: there are faster ways to run the tests if you're going to be
iterating on something. You might as well read on while you wait for that build
to complete.


## Running Tests Incrementally: a better way

If your code changes trigger rebuild of multiple Mononoke binaries there is a way
to avoid unnecessary rebuilds: our incremental helper scripts.
Using them allows you to skip most build steps, and rebuild only what you need to
re-run your test (e.g. if you're iterating on Mononoke server, then you won't
need to rebuild blobimport more than once).

To do this, you should start by building everything once for your integration
test target:

e.g.
```sh
./incremental_integration_setup.sh server/server
```

Note, `incremental_integration_setup.sh` builds with `@fbcode//mode/dev` by default,
you can add @mode/opt, then it will build in opt mode

```sh
./incremental_integration_setup.sh server/server @mode/opt
```

Then, run the tests by executing the pre-built incremental setup. Notice this
is done per rule, in this case `server/server`:

```sh
./incremental_integration_run.sh server/server test1.t test2.t test3.t
```

If your test rule lives in a subdirectory - for example `facebook/`, simply use name
of subdirectory followed by slash as a prefix, for example:

```sh
./incremental_integration_setup.sh facebook/snapshot
```

Note that you can run this from anywhere in fbsource tree (so you can
run it from the actual tests directory to get autocompletion or globbing on test
names).

Every time you make changes to your code, `buck build` whatever you changed,
then re-run.

## Adding new tests:

Add your new test in this directory (or under `facebook/`) if it's not relevant
to open-source.

Add the test to an existing test target, or add a new one if required.

If your test needs assets to work, then you'll need to:

- Put the asset somewhere under this directory.
- In tests, your asset can be found at `${TEST_FIXTURES}/relative/path`, where
  `relative/path` is the path to your asset relative from
  `.../mononoke/tests/integration`.
- Add your asset to the `test_fixtures` Buck rule in the `mononoke/tests` directory's
  `TARGETS` file. If you skip this step, you can still run tests directly using the runner
  (read on for more details), but it will not work correctly when using Buck / TestPilot.


## Exposing a new binary

Add it to `MONONOKE_TARGETS_TO_ENV` variable in the `fb_manifest_deps.bzl` file
in the `facebook/` directory. The key is the buck target for the dependency and
the value is an environment variable that will be set to the path to this
binary when the tests execute (if you need to customize the environment
variable a bit, you can do so in `facebook/generate_manifest.py`).

## dott_test targets
Tests are grouped into targets of `dott_test` type. Each `dott_test` target specifies:
- The name of the target
- `disable_all_network_access_target` which, if true, will run the test a second time with a wrapper that blocks network access. Enabling this introduces a maintenance cost as well as increasing CI load and diff testing latency. We prefer to enable it only for a small set of high-signal tests.
- A glob expression representing the test files included in the target.
- A list of its dependencies

# Running tests from OSS getdeps build

These examples use the oss github path to getdeps.py.  If running from internal monorepo the path to getdeps is ./opensource/fbcode_builder/getdeps.py

First build dependencies:

```
python3 ./build/fbcode_builder/getdeps.py build --allow-system-packages --no-facebook-internal --src-dir=. --no-tests sapling
python3 ./build/fbcode_builder/getdeps.py build --allow-system-packages --no-facebook-internal --src-dir=. --no-tests mononoke
```

Then build mononoke_integration and repeat if you need .t changes with --no-deps:

```
python3 ./build/fbcode_builder/getdeps.py build --allow-system-packages --no-facebook-internal --no-deps --src-dir=. mononoke_integration
```

And run the tests:

```
python3 ./build/fbcode_builder/getdeps.py test --allow-system-packages --no-facebook-internal --src-dir=. mononoke_integration
```

# How it works

To avoid full rebuilds whenever you make a change, the test runner takes a few
shortcuts to avoid relying on the Buck dependency graph.

Notably, it:

- Uses the actual test source files (and assets) from your fbcode working
  directory when running the runner directly (as documented above). This allows
  `--interactive` to work seamlessly. This works thanks to buck crating symlinks
  to test files rather than copying them for test runs.
- Stores the paths to all its dependencies in a manifest file (which is
  generated from Buck).

Normally, this should all be transparent if you're adding a new test and using
`${TEST_FIXTURES}` to reference it.
