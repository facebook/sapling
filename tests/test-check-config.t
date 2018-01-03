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
  > # Inconsistent values for defaults.
  > foo = ui.configint('ui', 'intdefault', default=1)
  > foo = ui.configint('ui', 'intdefault', default=42)
  > # Can suppress inconsistent value error
  > foo = ui.configint('ui', 'intdefault2', default=1)
  > # inconsistent config: ui.intdefault2
  > foo = ui.configint('ui', 'intdefault2', default=42)
  > EOF

  $ cat > files << EOF
  > mercurial/help/config.txt
  > $TESTTMP/testfile.py
  > EOF

  $ cd "$TESTDIR"/..

  $ $PYTHON contrib/check-config.py < $TESTTMP/files
  foo = ui.configint('ui', 'intdefault', default=42)
  conflict on ui.intdefault: ('int', '42') != ('int', '1')
  at $TESTTMP/testfile.py:12:
  undocumented: ui.doesnotexist (str)
  undocumented: ui.intdefault (int) [42]
  undocumented: ui.intdefault2 (int) [42]
  undocumented: ui.missingbool1 (bool) [True]
  undocumented: ui.missingbool2 (bool)
  undocumented: ui.missingint (int)

New errors are not allowed. Warnings are strongly discouraged.

  $ testrepohg files "set:(**.py or **.txt) - tests/**" | sed 's|\\|/|g' |
  >   $PYTHON contrib/check-config.py
  undocumented: remotenames.alias.default (bool)
  undocumented: remotenames.allownonfastforward (bool)
  undocumented: remotenames.calculatedistance (bool)
  undocumented: remotenames.disallowedbookmarks (list)
  undocumented: remotenames.disallowedhint (str)
  undocumented: remotenames.disallowedto (str)
  undocumented: remotenames.fastheaddiscovery (bool)
  undocumented: remotenames.forcecompat (bool)
  undocumented: remotenames.forceto (bool)
  undocumented: remotenames.hoist (str)
  undocumented: remotenames.pushanonheads (bool)
  undocumented: remotenames.pushrev (str)
  undocumented: remotenames.resolvenodes (bool)
  undocumented: remotenames.selectivepull (bool)
  undocumented: remotenames.selectivepulldefault (list)
  undocumented: remotenames.suppressbranches (bool)
  undocumented: remotenames.syncbookmarks (bool)
  undocumented: remotenames.tracking (bool)
  undocumented: remotenames.transitionmessage (str)
  undocumented: remotenames.upstream (list)
