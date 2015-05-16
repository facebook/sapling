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
  > if True:
  >     x = all()
  >     y = format(x)
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
  > 
  > # yield inside a try/finally block is not allowed in Python 2.4
  >     try:
  >         pass
  >         yield 1
  >     finally:
  >         pass
  >     try:
  >         yield
  >         pass
  >     finally:
  >         pass
  > 
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
  ./non-py24.py:3:
   >     x = all()
   all/format not available in Python 2.4
  ./non-py24.py:4:
   >     y = format(x)
   all/format not available in Python 2.4
  ./non-py24.py:24:
   >     try:
   no yield inside try/finally in Python 2.4
  ./non-py24.py:29:
   >     try:
   no yield inside try/finally in Python 2.4
  ./classstyle.py:4:
   > class oldstyle_class:
   old-style class, use class foo(object)
  ./classstyle.py:7:
   > class empty():
   class foo() creates old style object, use class foo(object)
  [1]
  $ cat > python3-compat.py << EOF
  > foo <> bar
  > reduce(lambda a, b: a + b, [1, 2, 3, 4])
  > dict(key=value)
  > EOF
  $ "$check_code" python3-compat.py
  python3-compat.py:1:
   > foo <> bar
   <> operator is not available in Python 3+, use !=
  python3-compat.py:2:
   > reduce(lambda a, b: a + b, [1, 2, 3, 4])
   reduce is not available in Python 3+
  python3-compat.py:3:
   > dict(key=value)
   dict() is different in Py2 and 3 and is slower than {}
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

  $ cat > for-nolineno.py <<EOF
  > except:
  > EOF
  $ "$check_code" for-nolineno.py --nolineno
  for-nolineno.py:0:
   > except:
   naked except clause
  [1]

  $ cat > warning.t <<EOF
  >   $ function warnonly {
  >   > }
  >   $ diff -N aaa
  >   $ function onwarn {}
  > EOF
  $ "$check_code" warning.t
  $ "$check_code" --warn warning.t
  warning.t:1:
   >   $ function warnonly {
   warning: don't use 'function', use old style
  warning.t:3:
   >   $ diff -N aaa
   warning: don't use 'diff -N'
  warning.t:4:
   >   $ function onwarn {}
   warning: don't use 'function', use old style
  [1]
  $ cat > raise-format.py <<EOF
  > raise SomeException, message
  > # this next line is okay
  > raise SomeException(arg1, arg2)
  > EOF
  $ "$check_code" not-existing.py raise-format.py
  Skipping*not-existing.py* (glob)
  raise-format.py:1:
   > raise SomeException, message
   don't use old-style two-argument raise, use Exception(message)
  [1]

  $ cat > rst.py <<EOF
  > """problematic rst text
  > 
  > .. note::
  >     wrong
  > """
  > 
  > '''
  > 
  > .. note::
  > 
  >     valid
  > 
  > new text
  > 
  >     .. note::
  > 
  >         also valid
  > '''
  > 
  > """mixed
  > 
  > .. note::
  > 
  >   good
  > 
  >     .. note::
  >         plus bad
  > """
  > EOF
  $ $check_code -w rst.py
  rst.py:3:
   > .. note::
   warning: add two newlines after '.. note::'
  rst.py:26:
   >     .. note::
   warning: add two newlines after '.. note::'
  [1]

  $ cat > ./map-inside-gettext.py <<EOF
  > print _("map inside gettext %s" % v)
  > 
  > print _("concatenating " " by " " space %s" % v)
  > print _("concatenating " + " by " + " '+' %s" % v)
  > 
  > print _("mapping operation in different line %s"
  >         % v)
  > 
  > print _(
  >         "leading spaces inside of '(' %s" % v)
  > EOF
  $ "$check_code" ./map-inside-gettext.py
  ./map-inside-gettext.py:1:
   > print _("map inside gettext %s" % v)
   don't use % inside _()
  ./map-inside-gettext.py:3:
   > print _("concatenating " " by " " space %s" % v)
   don't use % inside _()
  ./map-inside-gettext.py:4:
   > print _("concatenating " + " by " + " '+' %s" % v)
   don't use % inside _()
  ./map-inside-gettext.py:6:
   > print _("mapping operation in different line %s"
   don't use % inside _()
  ./map-inside-gettext.py:9:
   > print _(
   don't use % inside _()
  [1]

web templates

  $ mkdir -p mercurial/templates
  $ cat > mercurial/templates/example.tmpl <<EOF
  > {desc}
  > {desc|escape}
  > {desc|firstline}
  > {desc|websub}
  > EOF

  $ "$check_code" --warnings mercurial/templates/example.tmpl
  mercurial/templates/example.tmpl:2:
   > {desc|escape}
   warning: follow desc keyword with either firstline or websub
  [1]
