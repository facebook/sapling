
  $ cat > adddrop.py <<EOF
  > from mercurial import cmdutil
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('debugadddrop',
  >   [('', 'drop', False, 'drop file from dirstate', 'FILE'),
  >    ('', 'normal-lookup', False, 'add file to dirstate', 'FILE')],
  >     'hg debugadddrop')
  > def debugadddrop(ui, repo, *pats, **opts):
  >   '''Add or drop unnamed arguments to or from the dirstate'''
  >   drop = opts.get('drop')
  >   nl = opts.get('normal_lookup')
  >   if nl and drop:
  >       raise error.Abort('drop and normal-lookup are mutually exclusive')
  >   wlock = repo.wlock()
  >   try:
  >     for file in pats:
  >       if opts.get('normal_lookup'):
  >         repo.dirstate.normallookup(file)
  >       else:
  >         repo.dirstate.drop(file)
  > 
  >     repo.dirstate.write(repo.currenttransaction())
  >   finally:
  >     wlock.release()
  > EOF

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "debugadddrop=`pwd`/adddrop.py" >> $HGRCPATH

basic test for hg debugrebuildstate

  $ hg init repo
  $ cd repo

  $ touch foo bar
  $ hg ci -Am 'add foo bar'
  adding bar
  adding foo

  $ touch baz
  $ hg add baz
  $ hg rm bar

  $ hg debugrebuildstate

state dump after

  $ hg debugstate --nodates | sort
  n 644         -1 set                 bar
  n 644         -1 set                 foo

  $ hg debugadddrop --normal-lookup file1 file2
  $ hg debugadddrop --drop bar
  $ hg debugadddrop --drop
  $ hg debugstate --nodates
  n   0         -1 unset               file1
  n   0         -1 unset               file2
  n 644         -1 set                 foo
  $ hg debugrebuildstate

status

  $ hg st -A
  ! bar
  ? baz
  C foo

Test debugdirstate --minimal where a file is not in parent manifest
but in the dirstate
  $ touch foo bar qux
  $ hg add qux
  $ hg remove bar
  $ hg status -A
  A qux
  R bar
  ? baz
  C foo
  $ hg debugadddrop --normal-lookup baz
  $ hg debugdirstate --nodates
  r   0          0 * bar (glob)
  n   0         -1 * baz (glob)
  n 644          0 * foo (glob)
  a   0         -1 * qux (glob)
  $ hg debugrebuilddirstate --minimal
  $ hg debugdirstate --nodates
  r   0          0 * bar (glob)
  n 644          0 * foo (glob)
  a   0         -1 * qux (glob)
  $ hg status -A
  A qux
  R bar
  ? baz
  C foo

Test debugdirstate --minimal where file is in the parent manifest but not the
dirstate
  $ hg manifest
  bar
  foo
  $ hg status -A
  A qux
  R bar
  ? baz
  C foo
  $ hg debugdirstate --nodates
  r   0          0 * bar (glob)
  n 644          0 * foo (glob)
  a   0         -1 * qux (glob)
  $ hg debugadddrop --drop foo
  $ hg debugdirstate --nodates
  r   0          0 * bar (glob)
  a   0         -1 * qux (glob)
  $ hg debugrebuilddirstate --minimal
  $ hg debugdirstate --nodates
  r   0          0 * bar (glob)
  n 644         -1 * foo (glob)
  a   0         -1 * qux (glob)
  $ hg status -A
  A qux
  R bar
  ? baz
  C foo

