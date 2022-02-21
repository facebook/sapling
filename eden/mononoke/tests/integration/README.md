# Mononoke Integration Tests

Mononoke's integration tests run using Mercurial's run-tests test framework,
which is orchestrated using a wrapper to make it more friendly to TestPilot and
provide some added functionality, such as wiring up dependencies and / or
setting up an ephemeral MySQL shard.

Integration tests are grouped into small targets so that a minimal set of
dependencies are built for each test.

## TL;DR: Running one test

First, find the target responsible for the test you want to run by checking in
the TARGETS file.

Use:

```sh
buck2 run //eden/mononoke/tests/integration/facebook:some_target -- TEST
```

But! Keep reading: there are faster ways to run the tests if you're going to be
iterating on something. You might as well read on while you wait for that build
to complete.


## Running Tests Incrementally: a better way

To run tests locally, a better way is to our incremental helper scripts.
This allows you to skip most build steps, and rebuild only what you need to
re-run your test (e.g. if you're iterating on Mononoke server, then you won't
need to rebuild blobimport more than once).

To do this, you should start by building everything once for your integration
test target:

e.g.
```sh
./incremental_integration_setup.sh server
```

Then, run the tests by executing the pre-built incremental setup. Notice this
is done per rule, in this case `server`:

```sh
./incremental_integration_run.sh server test1.t test2.t test3.t
```

If your test group lies in the `facebook/` subdirectory, simply use the `facebook/`
prefix, for example:

```sh
./incremental_integration_setup.sh facebook/snapshot
```

Note that you can run this from anywhere in fbsource tree (so you can
run it from the actual tests directory to get autocompletion or globbing on test
names). The script defaults to using `buck2`, but you can set the `USEBUCK1` env
var so it uses `buck1`.

Every time you make changes to your code, `buck2 build` whatever you changed,
then re-run.

Use `--interactive` when running your tests in order to accept (or reject)
changes to your `.t` files, or `--keep-tmpdir` to be able to see the files edited
by your test after it runs.


## Adding new tests:

Add your new test in this directory (or under `facebook/`) if it's not relevant
to open-source.

Add the test to an existing test target, or add a new one if required.

If your test needs assets to work, then you'll need to:

- Put the asset somewhere under this directory.
- In tests, your asset can be found at `${TEST_FIXTURES}/relative/path`, where
  `relative/path` is the path to your asset relative from
  `.../mononoke/tests/integration`.
- Add your asset to the `test_fixtures` Buck rule in this directory's `TARGETS`
  file. If you don't do this, then running tests using the runner directly will
  work (read on to understand why), but it won't work when running through Buck
  / TestPilot.


## Exposing a new binary

Add it to `MONONOKE_TARGETS_TO_ENV` variable in the `fb_manifest_deps.bzl` file
in the `facebook/` directory. The key is the buck target for the dependency and
the value is an environment variable that will be set to the path to this
binary when the tests execute (if you need to customize the environment
variable a bit, you can do so in `facebook/generate_manifest.py`).

# Running tests from OSS getdeps build

These examples use the oss github path to getdeps.py.  If running from internal monorepo the path to getdeps is ./opensource/fbcode_builder/getdeps.py

First build mononoke_integration and its dependencies:

```
python3 ./build/fbcode_builder/getdeps.py build --allow-system-packages --no-facebook-internal mononoke_integration
```

Then to iterate on broken tests,  build mononoke (if you need binary change) or mononoke_integration (it you need .t changes) with --no-deps:

```
python3 ./build/fbcode_builder/getdeps.py build --allow-system-packages --no-facebook-internal --no-deps mononoke_integration
```

And run the tests:

```
python3 ./build/fbcode_builder/getdeps.py test --allow-system-packages --no-facebook-internal mononoke_integration
```

# How it works

To avoid full rebuilds whenever you make a change, the test runner takes a few
shortcuts to avoid relying on the Buck dependency graph.

Notably, it:

- Uses the actual test source files (and assets) from your fbcode working
  directory when running the runner directly (as documented above). This allows
  `--interactive` to work seamlessly.
- Stores the paths to all its dependencies in a manifest file (which is
  generated from Buck).

However, when you're running tests using Buck, then the test runner will not use
source files, and will instead expect files to be managed using Buck. The main
result of this is that while you might have a bunch of files jumbled together in
this directory, when running tests using Buck, they will not.

Normally, this should all be transparent if you're adding a new test and using
`${TEST_FIXTURES}` to reference it.
