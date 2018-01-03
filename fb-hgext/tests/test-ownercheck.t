  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > ownercheck=$TESTDIR/../hgext3rd/ownercheck.py
  > EOF

ownercheck does not prevent normal hg operations

  $ hg init repo1

make os.getuid return a different, fake uid

  $ cat >> fakeuid.py << EOF
  > import os
  > _getuid = os.getuid
  > def fakeuid(): return _getuid() + 1
  > os.getuid = fakeuid
  > EOF

ownercheck prevents wrong user from creating new repos

  $ hg --config extensions.fakeuid=fakeuid.py init repo2
  abort: $TESTTMP is owned by *, not you * (glob)
  you are likely doing something wrong.
  (you can skip the check using --config extensions.ownercheck=!)
  [255]

ownercheck prevents wrong user from accessing existing repos

  $ hg --config extensions.fakeuid=fakeuid.py log --repo repo1
  abort: $TESTTMP/repo1 is owned by *, not you * (glob)
  you are likely doing something wrong.
  (you can skip the check using --config extensions.ownercheck=!)
  [255]

