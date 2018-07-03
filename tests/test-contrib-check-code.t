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
  $ "$check_code" ./wrong.py ./correct.py ./quote.py ./classstyle.py
  ./wrong.py:1: gratuitous whitespace in () or [] --> def toto( arg1, arg2):
  ./wrong.py:2: Python keyword is not a function --> del(arg2)
  ./wrong.py:3: gratuitous whitespace in () or [] --> return ( 5+6, 9)
  ./quote.py:5: missing whitespace in expression --> '"""', 42+1, """and
  ./classstyle.py:4: old-style class, use class foo(object) --> class oldstyle_class:
  ./classstyle.py:7: class foo() creates old style object, use class foo(object) --> class empty():
  [1]
  $ cat > python3-compat.py << EOF
  > foo <> bar
  > reduce(lambda a, b: a + b, [1, 2, 3, 4])
  > dict(key=value)
  > EOF
  $ "$check_code" python3-compat.py
  python3-compat.py:1: <> operator is not available in Python 3+, use != --> foo <> bar
  python3-compat.py:2: reduce is not available in Python 3+ --> reduce(lambda a, b: a + b, [1, 2, 3, 4])
  python3-compat.py:3: dict constructor is different in Py2 and 3 and is slower than {} --> dict(key=value)
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
  ./is-op.py:3: object comparison with literal --> y = x is 'foo'
  ./is-op.py:4: object comparison with literal --> y = x is "foo"
  ./is-op.py:5: object comparison with literal --> y = x is 5346
  ./is-op.py:6: object comparison with literal --> y = x is -6
  ./is-op.py:7: object comparison with literal --> y = x is not 'foo'
  ./is-op.py:8: object comparison with literal --> y = x is not "foo"
  ./is-op.py:9: object comparison with literal --> y = x is not 5346
  ./is-op.py:10: object comparison with literal --> y = x is not -6
  [1]

  $ cat > for-nolineno.py <<EOF
  > except:
  > EOF
  $ "$check_code" for-nolineno.py --nolineno
  for-nolineno.py:0: naked except clause --> except:
  [1]

  $ cat > warning.t <<EOF
  >   $ function warnonly {
  >   > }
  >   $ diff -N aaa
  >   $ function onwarn {}
  > EOF
  $ "$check_code" warning.t
  $ "$check_code" --warn warning.t
  warning.t:1: warning: don't use 'function', use old style --> $ function warnonly {
  warning.t:3: warning: don't use 'diff -N' --> $ diff -N aaa
  warning.t:4: warning: don't use 'function', use old style --> $ function onwarn {}
  [1]
  $ cat > error.t <<EOF
  >   $ [ foo == bar ]
  > EOF
  $ "$check_code" error.t
  error.t:1: [ foo == bar ] is a bashism, use [ foo = bar ] instead --> $ [ foo == bar ]
  [1]
  $ rm error.t
  $ cat > raise-format.py <<EOF
  > raise SomeException, message
  > # this next line is okay
  > raise SomeException(arg1, arg2)
  > EOF
  $ "$check_code" not-existing.py raise-format.py
  Skipping*not-existing.py* (glob)
  raise-format.py:1: don't use old-style two-argument raise, use Exception(message) --> raise SomeException, message
  [1]

  $ cat <<EOF > tab.t
  > 	indent
  >   > 	heredoc
  > EOF
  $ "$check_code" tab.t
  tab.t:1: don't use tabs to indent --> indent
  [1]
  $ rm tab.t

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
  rst.py:3: warning: add two newlines after '.. note::' --> .. note::
  rst.py:26: warning: add two newlines after '.. note::' --> .. note::
  [1]

  $ cat > ./map-inside-gettext.py <<EOF
  > print(_("map inside gettext %s" % v))
  > 
  > print(_("concatenating " " by " " space %s" % v))
  > print(_("concatenating " + " by " + " '+' %s" % v))
  > 
  > print(_("mapping operation in different line %s"
  >         % v))
  > 
  > print(_(
  >         "leading spaces inside of '(' %s" % v))
  > EOF
  $ "$check_code" ./map-inside-gettext.py
  ./map-inside-gettext.py:1: don't use % inside _() --> print(_("map inside gettext %s" % v))
  ./map-inside-gettext.py:3: don't use % inside _() --> print(_("concatenating " " by " " space %s" % v))
  ./map-inside-gettext.py:4: don't use % inside _() --> print(_("concatenating " + " by " + " '+' %s" % v))
  ./map-inside-gettext.py:6: don't use % inside _() --> print(_("mapping operation in different line %s"
  ./map-inside-gettext.py:9: don't use % inside _() --> print(_(
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
  mercurial/templates/example.tmpl:2: warning: follow desc keyword with either firstline or websub --> {desc|escape}
  [1]

'string join across lines with no space' detection

  $ cat > stringjoin.py <<EOF
  > foo = (' foo'
  >        'bar foo.'
  >        'bar foo:'
  >        'bar foo@'
  >        'bar foo%'
  >        'bar foo*'
  >        'bar foo+'
  >        'bar foo-'
  >        'bar')
  > EOF

'missing _() in ui message' detection

  $ cat > uigettext.py <<EOF
  > ui.status("% 10s %05d % -3.2f %*s %%"
  >           # this use '\\\\' instead of '\\', because the latter in
  >           # heredoc on shell becomes just '\'
  >           '\\\\ \n \t \0'
  >           """12345
  >           """
  >           '''.:*+-=
  >           ''' "%-6d \n 123456 .:*+-= foobar")
  > EOF

superfluous pass

  $ cat > superfluous_pass.py <<EOF
  > # correct examples
  > if foo:
  >     pass
  > else:
  >     # comment-only line means still need pass
  >     pass
  > def nothing():
  >     pass
  > class empty(object):
  >     pass
  > if whatever:
  >     passvalue(value)
  > # bad examples
  > if foo:
  >     "foo"
  >     pass
  > else: # trailing comment doesn't fool checker
  >     wat()
  >     pass
  > def nothing():
  >     "docstring means no pass"
  >     pass
  > class empty(object):
  >     """multiline
  >     docstring also
  >     means no pass"""
  >     pass
  > EOF

(Checking multiple invalid files at once examines whether caching
translation table for repquote() works as expected or not. All files
should break rules depending on result of repquote(), in this case)

  $ "$check_code" stringjoin.py uigettext.py superfluous_pass.py
  stringjoin.py:1: string join across lines with no space --> foo = (' foo'
  stringjoin.py:2: string join across lines with no space --> 'bar foo.'
  stringjoin.py:3: string join across lines with no space --> 'bar foo:'
  stringjoin.py:4: string join across lines with no space --> 'bar foo@'
  stringjoin.py:5: string join across lines with no space --> 'bar foo%'
  stringjoin.py:6: string join across lines with no space --> 'bar foo*'
  stringjoin.py:7: string join across lines with no space --> 'bar foo+'
  stringjoin.py:8: string join across lines with no space --> 'bar foo-'
  uigettext.py:1: missing _() in ui message (use () to hide false-positives) --> ui.status("% 10s %05d % -3.2f %*s %%"
  superfluous_pass.py:14: omit superfluous pass --> if foo:
  superfluous_pass.py:17: omit superfluous pass --> else: # trailing comment doesn't fool checker
  superfluous_pass.py:20: omit superfluous pass --> def nothing():
  superfluous_pass.py:23: omit superfluous pass --> class empty(object):
  [1]
