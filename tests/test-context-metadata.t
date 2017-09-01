Tests about metadataonlyctx

  $ hg init
  $ echo A > A
  $ hg commit -A A -m 'Add A'
  $ echo B > B
  $ hg commit -A B -m 'Add B'
  $ hg rm A
  $ echo C > C
  $ echo B2 > B
  $ hg add C -q
  $ hg commit -m 'Remove A'

  $ cat > metaedit.py <<EOF
  > from __future__ import absolute_import
  > from mercurial import context, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('metaedit')
  > def metaedit(ui, repo, arg):
  >     # Modify commit message to "FOO"
  >     with repo.wlock(), repo.lock(), repo.transaction('metaedit'):
  >         old = repo['.']
  >         kwargs = dict(s.split('=', 1) for s in arg.split(';'))
  >         if 'parents' in kwargs:
  >             kwargs['parents'] = kwargs['parents'].split(',')
  >         new = context.metadataonlyctx(repo, old, **kwargs)
  >         new.commit()
  > EOF
  $ hg --config extensions.metaedit=$TESTTMP/metaedit.py metaedit 'text=Changed'
  $ hg log -r tip
  changeset:   3:ad83e9e00ec9
  tag:         tip
  parent:      1:3afb7afe6632
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Changed
  
  $ hg --config extensions.metaedit=$TESTTMP/metaedit.py metaedit 'parents=0' 2>&1 | egrep '^RuntimeError'
  RuntimeError: can't reuse the manifest: its p1 doesn't match the new ctx p1

  $ hg --config extensions.metaedit=$TESTTMP/metaedit.py metaedit 'user=foo <foo@example.com>'
  $ hg log -r tip
  changeset:   4:1f86eaeca92b
  tag:         tip
  parent:      1:3afb7afe6632
  user:        foo <foo@example.com>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Remove A
  
