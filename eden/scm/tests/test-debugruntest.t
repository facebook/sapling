#chg-compatible
This test is broken while conch_parser is broken
#require false


Classic .t test:

  $ cat > test-sh.t << 'EOF'
  > Check shell output:
  >   $ echo 1
  >   1
  > Check Python output:
  >   >>> 1+2
  >   3
  > No PATH access:
  >   $ bash -c ''
  >   sh: command not found: bash
  >   [127]
  >   >>> try:
  >   ...     __import__('subprocess').call(['sh', '-c', "echo abcdef"])
  >   ... except FileNotFoundError:
  >   ...     print('not found as expected')
  >   not found as expected
  > EOF

Vanilla Python test:

  $ cat > test-py-vanilla.t << 'EOF'
  > a = 1
  > b = 2
  > assert a != b
  > EOF

Python / .t hybrid:

  $ cat > test-py-hybrid.t << 'EOF'
  > for i in range(3):
  >     setenv("A", str(i))
  >     setenv("B", str(i))
  >     $ [ $A -eq $B ] && echo same
  >     same
  >     $ echo $A
  >     [012] (re)
  > EOF

Diff output:

  $ cat > test-fail-sh.t << 'EOF'
  >   $ seq 3
  >   0
  >   1
  >   2
  > 
  >   >>> 1+2
  >   5
  > EOF

Skip:

  $ cat > test-skip.t << 'EOF'
  > #require false
  > EOF

Exception:

  $ cat > test-py-exc.t << 'EOF'
  > raise ValueError('this test is broken')
  > EOF

Test output:

  $ hg debugruntest test-sh.t
  # Ran 1 tests, 0 skipped, 0 failed.

  $ hg debugruntest -v test-sh.t
  Passed 1 test:
    test-sh.t
  
  # Ran 1 tests, 0 skipped, 0 failed.

  $ hg debugruntest -j1 test-*.t test-foo.t test-bar.t
  test-fail-sh.t ---------------------------------------------------------------
     1 $ seq 3
      -0
       1
       2
      +3
  
     6 >>> 1+2
      -5
      +3
  
  test-py-exc.t ----------------------------------------------------------------
  Traceback (most recent call last):
    File * (glob)
      raise ValueError('this test is broken')
  ValueError: this test is broken
  
  -----------------------------------------------------------------------------
  Skipped 1 test (missing feature: false):
    test-skip.t
  
  Failed 2 tests (not found):
    test-bar.t
    test-foo.t
  
  Failed 1 test (output mismatch):
    test-fail-sh.t
  
  Failed 1 test (this test is broken):
    test-py-exc.t
  
  # Ran 8 tests, 1 skipped, 4 failed.
  [1]

Autofix:

  $ hg debugruntest --fix test-fail-sh.t
  Failed 1 test (output mismatch):
    test-fail-sh.t
  
  Fixed 1 test:
    test-fail-sh.t
  
  # Ran 1 tests, 0 skipped, 1 failed.
  [1]

  $ hg debugruntest test-fail-sh.t
  # Ran 1 tests, 0 skipped, 0 failed.


#if no-bash
Doctest:

  $ cat >> testmodule.py << 'EOF'
  > """
  > A module for doctest testing
  >   >>> 1+1
  >   3
  > """
  > def plus(a, b):
  >     r"""a+b
  >        >>> plus(10, 20)
  >        31
  >        32
  >        >>> plus('a', 'b')
  >        >>> plus('a', 3)
  >     """
  >     return a + b
  > EOF

  $ ls testmodule.py
  testmodule.py

  >>> import testmodule

  $ hg debugruntest doctest:testmodule
  doctest:testmodule -----------------------------------------------------------
     3 >>> 1+1
      -3
      +2
  
     8 >>> plus(10, 20)
      -31
      -32
      +30
  
    11 >>> plus('a', 'b')
      +'ab'
  
    12 >>> plus('a', 3)
      +Traceback (most recent call last):
      +  ...
      +TypeError: can only concatenate str (not "int") to str
  
  -----------------------------------------------------------------------------
  Failed 1 test (output mismatch):
    doctest:testmodule
  
  # Ran 1 tests, 0 skipped, 1 failed.
  [1]

Doctest can be auto fixed too:

  $ hg debugruntest -q --fix doctest:testmodule
  [1]
  $ cat testmodule.py
  """
  A module for doctest testing
    >>> 1+1
    2
  """
  def plus(a, b):
      r"""a+b
         >>> plus(10, 20)
         30
         >>> plus('a', 'b')
         'ab'
         >>> plus('a', 3)
         Traceback (most recent call last):
           ...
         TypeError: can only concatenate str (not "int") to str
      """
      return a + b

Reload the cached module:

    from importlib import reload
    import testmodule
    reload(testmodule)

The doctest passes with the autofix changes:

  $ hg debugruntest doctest:testmodule
  # Ran 1 tests, 0 skipped, 0 failed.

#endif
