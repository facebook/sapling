#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"

Sanity check check-config.py

  $ cat > testfile.py << EOF
  > # Good
  > foo = ui.config('ui', 'username')
  > # Missing
  > foo = ui.config('ui', 'doesnotexist')
  > # Missing different type
  > foo = ui.configint('ui', 'missingint')
  > # Missing with default value
  > foo = ui.configbool('ui', 'missingbool1', default=True)
  > foo = ui.configbool('ui', 'missingbool2', False)
  > EOF

  $ cat > files << EOF
  > mercurial/help/config.txt
  > $TESTTMP/testfile.py
  > EOF

  $ cd "$TESTDIR"/..

  $ $PYTHON contrib/check-config.py < $TESTTMP/files
  undocumented: ui.doesnotexist (str)
  undocumented: ui.missingbool1 (bool) [True]
  undocumented: ui.missingbool2 (bool)
  undocumented: ui.missingint (int)

New errors are not allowed. Warnings are strongly discouraged.

  $ hg files "set:(**.py or **.txt) - tests/**" | sed 's|\\|/|g' |
  >   $PYTHON contrib/check-config.py
              limit = ui.configwith(fraction, 'profiling', 'showmin', 0.05)
  
  conflict on profiling.showmin: ('with', '0.05') != ('with', '0.005')
