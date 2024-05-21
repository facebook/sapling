Writing Tests
=============

For different languages, or purposes, there are different ways to write tests.

Unit tests, and doctests are generally good choices for Rust. The
``sapling/`` Python API is not stable, and things are coupled too much
(ex. the Python bookmark store ``bmstore`` object cannot be created without an
repo object). Therefore Python unit tests only make more sense for logic
that is relatively independent.

Sapling also has a unique kind of tests - ``.t`` tests. It is a good fit for
testing end-user command-line experience.


``.t`` Tests
------------

``.t`` tests live in ``tests/``. They can be run by
``run-tests.py <.t file name>``.

### Basic

Each test looks like indented blocks of bash scripts with commentary.
For example

```
Test 'echo' works. This line is a comment.

  $ echo A
  A
```

The test engine will execute ``echo A`` and verify its output is ``A``.

The ``.t`` format also supports multi-line commands, Python scripts and
testing exit code:

Multi-line commands (with heredoc):

```
  $ sha1sum << EOF
  > hello
  > EOF
  f572d396fae9206628714fb2ce00f72e94f2258f
```

Python code blocks:

```
  >>> import sys
  >>> for i in "hello world".split():
  ...     sys.stdout.write("%s\n" % i)
  hello
  world
```

Exit code can be tested using [code]:

```
  $ false
  [1]
```

To get started with creating a test, you can set ``PS1='$ '`` in your shell
and experiment with the reproducing commands. When done, just copy them to
a ``.t`` file and prefix them with two spaces.

You can also just edit the ``$`` command lines in ``test-foo.t`` directly, and
use ``run-tests.py -i test-foo.t`` to fill in the output. This is also a good
way to edit tests.


### Best practice

#### Recommended test setup

tl;dr: Write tests as follows:

```
  $ newclientrepo <<'EOS'
  > B
  > |
  > A
  > EOS

This is a comment
  $ touch something
  $ hg st # this is another comment
  ? something
  $ hg go $A -q
```

**The recommended way to create new repos is to use `newclientrepo`**.

By default new tests test against:
- Sapling without Watchman
- Sapling and Watchman
- Sapling and EdenFS

If it's necessary to specify just one of them, `#require eden` / `#require no-eden` / `#require fsmonitor` / `#require no-fsmonitor`
can be added at the top of the file for specifying only one of them.

#### Create a new repo for each sub-test-case

Creating a new repo is a very cheap operation and can help untangle future
issues caused by overusing the same one. It's possible to specify the names of
new repos when using `newclientrepo`; the name of the server for the repo can
also be specified. For example:

```
  $ newclientrepo
  $ pwd # the name of the repo is repo<N> by default
  $TESTTMP/repo1
  $ hg config paths.default # similarly, the repo names are repo<N>
  test:repo1_server
```

#### Running tests against Watchman or EdenFS

Currently these two can only be run through Buck. `-` and `.` in test names have
to be converted to `_`. For instance,

```
# Runs test-rust-checkout.t with Watchman enabled
$ buck2 test '@fbcode//mode/opt' :hg_watchman_run_tests -- test_rust_checkout_t

# Runs test-rust-checkout.t with EdenFS enabled
$ buck2 test '@fbcode//mode/opt' :hg_edenfs_run_tests -- test_rust_checkout_t
```

On EdenFS tests the EdenFS CLI is available throug the `eden` command; it's
recommended for new repos (cloned or newly created) to be created through
`newclientrepo`. See the previous section for an example on how to do this.

#### Silence uninteresting output

Not all output is interesting to the test. For example, when testing
``hg log``, the output of ``hg update`` is not interesting. Use ``-q``
to silence it

```
  $ hg update -q commit-x
```

This makes the test cleaner and easier to codemod ``update`` output.

Similarity, avoid testing revision numbers, or branch names, if they are not
interesting to the test. It will make deprecation of those features easier.

#### Use drawdag to create commits

``hg debugdrawdag`` (or ``drawdag`` defined in ``tinit.sh``) can be used to
create commits in a more readable, and efficient way. `newclientrepo` (also
defined on `tinit.sh`) can also take the same input as `drawdag`. See
[the drawdag page](../internals/drawdag) for more info.

#### Avoid depending on context

As the test file grows longer, it could become difficult to follow or modify.
It's often caused by commands depending on the context (ex. the current repo
state, or the current directory) and the context is not obvious by just
reading the code. Here are some tips to make tests easier to understand:

- Avoid ``..`` in filesystem paths. Instead of ``cd ../repo1``,
  use ``cd $TESTTMP/repo1``.
- Avoid using a list of ``hg commit``, ``hg update`` to create a repo.
  Use drawdag if possible. If drawdag cannot be used, insert a ``hg log -G``
  command to print the repo content out.


### Advanced features

#### Test environment

A test starts inside a temporary directory, which can be obtained using
``TESTTMP`` environment variable. The ``TESTDIR`` environment variable contains
the path to the ``tests/`` directory, which can be handy to refer to other
scripts.

``tests/tinit.sh`` is sourced. Functions defined in it can be used to make
tests shorter. For example

```
Use functions in tinit.sh:
  $ setconfig lfs.url=file://$TESTTMP/lfs lfs.threshold=10B
  $ enable lfs rebase
  $ newrepo
```

Equivalent to:

```
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > rebase=
  > [lfs]
  > url=file://$TESTTMP/lfs
  > threshold=10B
  > EOF
  $ hg init repo1
  $ cd repo1
```

A particularly important function is `newclientrepo`. It also allows specifying
different repo and server names:

```
  $ newclientrepo foo
  $ newclientrepo bar test:customservername
  $ cd $TESTTMP/foo # this goes back to the foo repo
```

Similarly, there are some default Sapling configs defined in
`tests/default_hgrc.py`. These defaults change depending on whether tests are
compatible with `debugruntest` or not.

#### Conditional logic

Certain tests might require some features (ex. POSIX, case insensitive
filesystem, or certain programs to be installed). Run ``python tests/hghave
--list`` to get a list of features that can be tested. Example use in ``.t``
looks like

```
#require fsmonitor icasefs
The test will be skipped if any of the requirement is not sastified.

#if symlink
This block is skipped if symlink is not supported.
  $ ln -s a b
#else
This block is skipped if symlink is supported.
  $ cp a b
#endif
```

"If" statements can be nested as well and multiple statements can be put in a
single statement:

```
#if symlink no-osx
This block will only be run if symlinks are supported and macOS is not being used
  $ ln -s a b

#if execbit
This block will only be run if symlinks are supported, macOS is not being used, and execbit is supported
  $ chmod +x a
#endif

#else
This block will only be run if symlinks are not supported or macOS is being used
  $ cp a b
#endif
```

Features can be prefixed with ``no-`` meaning it should not be selected

```
#require no-fsmonitor
Skip this test on 'run-tests.py --watchman'.
```

#### Multiple test cases

Sometimes it's feasible to reuse the most of the test code for different code
paths. ``#testcases`` can be used to define test case names that can be used
for feature testing

```
#testcases innodb rocksdb

#if innodb
  $ setconfig db.engine=inno
#else
  $ setconfig db.engine=rocks
#endif
```

This runs the test once for each test case.

#### Hybrid Python code blocks

If using `debugruntest`, it's possible to combine Python code blocks with
shell-like input. For instance,

```
    if True:
      $ echo 1
      1
```

#### Processing previous command output in Python

Sometimes it can be useful to process some command's output on Python rather
than just to expect some value. If `debugruntest` is used, last command's output
can is stored in the `_` variable in Python. For example,

```
  $ echo 123
  123
  >>> _ == "123\n"
  True
  >>> assert _ == "True\n"
```

#### Matching dynamic output

To filter noisy output that changes on each run (ex. timestamps), use glob
patterns and put a space and ``(glob)`` at the end of the output line

```
  $ hg parents -r null --time
  time: real * secs (user * sys *) (glob)
```

In the same vein, regular expressions can be also used with `(re)`:

```
  $ echo "   3"
  \s*3 (re)
```

Escape sequences can be expected as well:
```
  $ hg debugtemplate '{label(\"test.test\", \"output\n\")}' --config color.test.test=blue
  \x1b[34moutput\x1b[39m (esc)
```

You can match different output based on which features are available. Use
``(feature-name !)`` to mark a line as required if the feature was turned on,
or optional otherwise.

```
  $ hg debugfsinfo | grep eden
  fstype: eden (eden !)
```

More than one feature can be expected here (all of them will be "and"-end), and
globs can be used as well:
```
  $ hg go $B
  update failed to remove foo: Can't remove file "*foo": The process cannot access the file because it is being used by another process. (os error 32)! (glob) (windows !) (no-eden !)
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
```

Use ``(?)`` to mark output as optional unconditionally

```
  $ maybe-output-foobar
  foobar (?)
```

There is an additional mechanism for matching output more or less arbitrarily;
this is done through `registerfallbackmatch`, and this is what `.t` tests to be
ok with non-EdenFS and EdenFS outputs from `hg goto`. That makes

```
  $ hg goto foo
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
```

work without having to resort to

```
#if no-eden
  $ hg goto foo
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
#else
  $ hg goto foo
  update complete
#endif
```

#### Available commands and binaries

For making a test only run if a certain binary is available, the `#require`
statements, `#if` blocks, or `( !)` line matching can be used. For instance,

```
#require git

  $ git --version | wc -l 1
  1

#if node
  $ node --version | wc -l 1
  1
#endif

  $ lldb -P 2>&1 | wc -l 1
  1 (lldb !)
  [255] (no-lldb !)
```

As mentioned previously, there are two different engines for .t tests.

On the old test engine, commands are run using Bash on macOS and Linux, and
all usual commands (`ls`, `echo`) are the ones that the new test engine
implements. For other binaries (e.g., `git`, `unzip`, etc.) the ones provided
by the system are used; whichever commands are in `PATH` can actually be used.

In the case of `debugruntest` tests, Bash is not actually used and Shell
builtins / coreutils are implemented by the test runner. Additionally, certain
commands such as `unzip` are actually implemented within the test runner itself.
This is done for improving compatibility with non-POSIX OSes and for performance
reasons.

#### Test-level settings

Currently we have four test-level settings:

- `#debugruntest-incompatible` :: Makes the test use the legacy test engine.
- `#inprocess-hg-incompatible` :: To be used on `debugruntest` tests. Without
  this, a new Sapling process is used every time Sapling is invoked in `.t`
  tests. There are a few more caveats, see the documentation under
  `scm/sapling/testing`.
- `#chg-compatible` ::  To be used on non-`debugruntest` tests. This is similar
  to *not* using `#inprocess-hg-incompatible` from above, making `.t` tests use
  the `chg` daemon for Sapling processes.
- `#modern-config-incompatible` :: Only compatible with `debugruntest` tests and
  **not*** intended te be used on new tests. This exists for legacy reasons,
  making tests use much older configs by default.


Rust tests
----------

Follow the Rust community standard.

For modules that are likely to be used by other developers, Rustdoc is a good
choice to show examples about how to use a function. Especially when it's not
obvious.

For native Rust code, prefer unit tests inside modules

```
/* module code */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_x() {
        assert!(...);
    }
}
```

Use ``tests/`` for independent integration tests, and ``benches/`` for
benchmarks.


Python tests
------------
``run-tests.py`` supports not only ``.t`` tests, but also standard Python unit
tests in ``.py`` files. See ``test-lock.py`` for an example.

Python functions can have doctests, run by ``run-tests.py test-doctest.py``.
See D8221079 for an example.
