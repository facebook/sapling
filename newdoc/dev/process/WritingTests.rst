Writing Tests
=============

For different languages, or purposes, there are different ways to write tests.

Unit tests, and doctests are generally good choices for Rust. The
``mercurial/`` Python API is not stable, and things are coupled too much
(ex. the Python bookmark store ``bmstore`` object cannot be created without an
repo object). Therefore Python unit tests only make more sense for logic
that is relatively independent.

Mercurial also has a unique kind of tests - ``.t`` tests. It is a good fit for
testing end-user command-line experience.


``.t`` Tests
------------

``.t`` tests live in ``tests/``. They can be run by
``run-tests.py <.t file name>``.

Basic
~~~~~

Each test looks like indented blocks of bash scripts with commentary.
For example::

  Test 'echo' works. This line is a comment.

    $ echo A
    A

The test engine will execute ``echo A`` and verify its output is ``A``.

The ``.t`` format also supports multi-line commands, Python scripts and
testing exit code::

  Multi-line commands (with heredoc):

    $ sha1sum << EOF
    > hello
    > EOF
    f572d396fae9206628714fb2ce00f72e94f2258f

  Python:

    >>> import sys
    >>> for i in "hello world".split():
    ...     sys.stdout.write("%s\n" % i)
    hello
    world

  Exit code can be tested using [code]:

    $ false
    [1]

To get started with creating a test, you can set ``PS1='$ '`` in your shell
and experiment with the reproducing commands. When done, just copy them to
a ``.t`` file and prefix them with two spaces.

You can also just edit the ``$`` command lines in ``test-foo.t`` directly, and
use ``run-tests.py -i test-foo.t`` to fill in the output. This is also a good
way to edit tests.


Advanced features
~~~~~~~~~~~~~~~~~

Test environment
""""""""""""""""
A test starts inside a temporary directory, which can be obtained using
``TESTTMP`` environment variable. The ``TESTDIR`` environment variable contains
the path to the ``tests/`` directory, which can be handy to refer to other
scripts.

``tests/tinit.sh`` is sourced. Functions defined in it can be used to make
tests shorter. For example::

  Use functions in tinit.sh:

    $ setconfig lfs.url=file://$TESTTMP/lfs lfs.threshold=10B
    $ enable lfs rebase
    $ newrepo

  Equvilent to:

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


Conditional logic
"""""""""""""""""
Certain tests might require some features (ex. POSIX, case insenstive
filesystem, or certain programs to be installed). Run ``tests/hghave --list``
to get a list of features that can be tested. Example use in ``.t`` looks
like::

  #require fsmonitor icasefs
  The test will be skipped if any of the requirement is not sastified.

  #if symlink
  This block is skipped if symlink is not supported.
    $ ln -s a b
  #else
  This block is skipped if symlink is supported.
    $ cp a b
  #endif

Features can be prefixed with ``no-`` meaning it should not be selected::

  #require no-fsmonitor
  Skip this test on 'run-tests.py --watchman'.

Multiple test cases
"""""""""""""""""""

Sometimes it's feasible to reuse the most of the test code for different code
paths. ``#testcases`` can be used to define test case names that can be used
for feature testing::

  #testcases innodb rocksdb

  #if innodb
    $ setconfig db.engine=inno
  #else
    $ setconfig db.engine=rocks
  #endif

This runs the test once for each test case.

Matching dynamic output
"""""""""""""""""""""""

To filter noisy output that changes on each run (ex. timestamps), use glob
patterns and put a space and ``(glob)`` at the end of the output line::

  $ hg parents -r null --time
  time: real * secs (user * sys *) (glob)

You can match different output based on which features are available. Use
``(feature-name !)`` to mark a line as required if the feature was turned on,
or optional otherwise::

  $ hg debugfsinfo | grep eden
  fstype: eden (eden !)

Use ``(?)`` to mark output as optional unconditionally::

  $ maybe-output-foobar
  foobar (?)


Best practise
~~~~~~~~~~~~~

Silence uninteresting output
""""""""""""""""""""""""""""

Not all output is interesting to the test. For example, when testing
``hg log``, the output of ``hg update`` is not interesting. Use ``-q``
to silence it::

  $ hg update -q commit-x

This makes the test cleaner and easier to codemod ``update`` output.

Similarity, avoid testing revision numbers, or branch names, if they are not
interesting to the test. It will make deprecation of those features easier.

Use drawdag to create commits
"""""""""""""""""""""""""""""

``hg debugdrawdag`` (or ``drawdag`` defined in ``tinit.sh``) can be used to
create commits in a more readable, and efficient way. For example::

  $ echo X > X
  $ hg commit -m X -A X
  $ echo Y > Y
  $ hg commit -m Y -A Y
  $ hg update '.^'
  $ echo Z > Z
  $ hg commit -m Z -A Z

Can be rewritten as::

  $ drawdag <<'EOS'
  > Y Z    # This is a comment.
  > |/     # 'drawdag' defines env-var "$X", "$Y", "$Z" as commit hashes
  > X      # 'hg debugdrawdag' defines tags X, Y, Z instead
  > EOS
  $ hg update $Z

Comments can be used to define relationship between commits, file contents, and
"copy from" source::

  $ drawdag <<'EOS'
  >   D  # amend: C -> D
  >   |  # (Mark commit D as "amended from" commit C)
  >   |
  > C |  # C/src/main.cpp= (deleted)
  > |/   # (Delete the src/main.cpp file in commit C)
  > |
  > B    # B/src/main.cpp=int main()\n{} (renamed from src/main.c)
  > |    # (In commit "B", "src/main.cpp" has content "int main()\n{}",
  > |    #  and is marked as "renamed from" src/main.c.
  > |    #  "(copied from <path>)" can be used too)
  > |
  > A    # A/src/main.c=int main[] = {1,2};
  >      # (In commit "A", "src/main.c" has content "int main[] = {1, 2};")
  > EOS

Avoid depending on context
""""""""""""""""""""""""""

As the test file grows longer, it could become difficult to follow or modify.
It's often caused by commands depending on the context (ex. the current repo
state, or the current directory) and the context is not obvious by just
reading the code. Here are some tips to make tests easier to understand:

- Avoid ``..`` in filesystem paths. Instead of ``cd ../repo1``,
  use ``cd $TESTTMP/repo1``.
- Avoid using a list of ``hg commit``, ``hg update`` to create a repo.
  Use drawdag if possible. If drawdag cannot be used, insert a ``hg log -G``
  command to print the repo content out.


Rust tests
----------

Follow the Rust community standard.

For modules that are likely to be used by other developers, Rustdoc is a good
choice to show examples about how to use a function. Especially when it's not
obvious.

For native Rust code, prefer unit tests inside modules::

  /* module code */

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_feature_x() {
          assert!(...);
      }
  }

Use ``tests/`` for independent integration tests, and ``benches/`` for
benchmarks.


Python tests
------------
``run-tests.py`` supports not only ``.t`` tests, but also standard Python unit
tests in ``.py`` files. See ``test-lock.py`` for an example.

Python functions can have doctests, run by ``run-tests.py test-doctest.py``.
See D8221079 for an example.
