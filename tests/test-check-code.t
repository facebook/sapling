  $ cat > correct.py <<EOF
  > def toto(arg1, arg2):
  >     del arg2
  >     return (5 + 6, 9)
  > EOF
  $ cat > wrong.py <<EOF
  > def toto( arg1, arg2):
  >     del(arg2)
  >     return ( 5+6, 9)
  > EOF
  $ cat > quote.py <<EOF
  > # let's use quote in comments
  > (''' ( 4x5 )
  > but """\\''' and finally''',
  > """let's fool checkpatch""", '1+2',
  > '"""', 42+1, """and
  > ( 4-1 ) """, "( 1+1 )\" and ")
  > a, '\\\\\\\\', "\\\\\\" x-2", "c-1"
  > EOF
  $ cat > non-py24.py <<EOF
  > # Using builtins that does not exist in Python 2.4
  > if any():
  >     x = all()
  >     y = format(x)
  > 
  > # Do not complain about our own definition
  > def any(x):
  >     pass
  > 
  > # try/except/finally block does not exist in Python 2.4
  >     try:
  >         pass
  >     except StandardError, inst:
  >         pass
  >     finally:
  >         pass
  > 
  > # nested try/finally+try/except is allowed
  >     try:
  >         try:
  >             pass
  >         except StandardError, inst:
  >             pass
  >     finally:
  >         pass
  > EOF
  $ cat > classstyle.py <<EOF
  > class newstyle_class(object):
  >     pass
  > 
  > class oldstyle_class:
  >     pass
  > 
  > class empty():
  >     pass
  > 
  > no_class = 1:
  >     pass
  > EOF
  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ "$check_code" ./wrong.py ./correct.py ./quote.py ./non-py24.py ./classstyle.py
  ./wrong.py:1:
   > def toto( arg1, arg2):
   gratuitous whitespace in () or []
  ./wrong.py:2:
   >     del(arg2)
   Python keyword is not a function
  ./wrong.py:3:
   >     return ( 5+6, 9)
   gratuitous whitespace in () or []
   missing whitespace in expression
  ./quote.py:5:
   > '"""', 42+1, """and
   missing whitespace in expression
  ./non-py24.py:2:
   > if any():
   any/all/format not available in Python 2.4
  ./non-py24.py:3:
   >     x = all()
   any/all/format not available in Python 2.4
  ./non-py24.py:4:
   >     y = format(x)
   any/all/format not available in Python 2.4
  ./non-py24.py:11:
   >     try:
   no try/except/finally in Py2.4
  ./classstyle.py:4:
   > class oldstyle_class:
   old-style class, use class foo(object)
  ./classstyle.py:7:
   > class empty():
   class foo() not available in Python 2.4, use class foo(object)
  [1]

  $ cat > is-op.py <<EOF
  > # is-operator comparing number or string literal
  > x = None
  > y = x is 'foo'
  > y = x is "foo"
  > y = x is 5346
  > y = x is -6
  > y = x is not 'foo'
  > y = x is not "foo"
  > y = x is not 5346
  > y = x is not -6
  > EOF

  $ "$check_code" ./is-op.py
  ./is-op.py:3:
   > y = x is 'foo'
   object comparison with literal
  ./is-op.py:4:
   > y = x is "foo"
   object comparison with literal
  ./is-op.py:5:
   > y = x is 5346
   object comparison with literal
  ./is-op.py:6:
   > y = x is -6
   object comparison with literal
  ./is-op.py:7:
   > y = x is not 'foo'
   object comparison with literal
  ./is-op.py:8:
   > y = x is not "foo"
   object comparison with literal
  ./is-op.py:9:
   > y = x is not 5346
   object comparison with literal
  ./is-op.py:10:
   > y = x is not -6
   object comparison with literal
  [1]

