# Mononoke Integration Tests

Mononoke's integration tests run using Mercurial's run-tests test framework,
which is orchestrated using a wrapper to make it more friendly to TestPilot and
provide some added functionality, such as wiring up dependencies and / or
setting up an ephemeral MySQL shard.


## TL;DR: Running one test

Use:

```sh
buck run scm/mononoke/tests/integration:integration_runner -- TEST
```

But! Keep reading. There are better ways to run the tests if you're going to be
iterating on something.


## Running Tests Incrementally: a better way

To run tests locally, a better way is to run the integration runner directly.
This allows you to skip most build steps, and rebuild only what you need to
re-run your test (e.g. if you're iterating on Mononoke server, then you won't
need to rebuild blobimport more than once).

To do this, you should start by building everything once:

```sh
buck build scm/mononoke/tests/integration
```

Then, run the tests by executing the integration runner directly. The
integration runner relies on a manifest to find all the binaries it needs to run
(the ones you built earlier), so you need to point it there:

```
~/fbcode/buck-out/dev/gen/scm/mononoke/tests/integration/integration_runner_real.par \
  ~/fbcode/buck-out/gen/scm/mononoke/tests/integration/manifest/manifest.json \
  test1.t test2.t test3.t
```

If you don't have `~/fbcode` symlink, create it, or update the instructions as
needed. Note that you can run this from anywhere in fbsource tree (so you can
run it from the actual tests directory to get autocompletion or globbing on test
names).

Every time you make changes to your code, `buck build` whatever you changed,
then re-run.

Use `--interactive` when running your tests in order to accept (or reject)
changes to your `.t` files.


## Adding new tests:

Add your new test in this directory (or under `facebook/`) if it's not relevant
to open-source.

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

Add it to `MANIFEST_DEPS` in the `TARGETS` file in this directory. The key is an
environment variable that will be set to the path to this binary when the tests
execute (if you need to customize the environment variable a bit, you can do so
in `generate_manifest.py`).


# How it works

To avoid full rebuilds whenever you make a change, the test runner takes a few
shortcuts to avoid relying on the Buck dependency graph (that is because Buck
doesn't see each individual test's dependencies: it only knows that all the
tests depend on everything).

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
