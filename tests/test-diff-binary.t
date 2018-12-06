  $ newrepo
  $ setconfig diff.git=1 diff.nobinary=1

  >>> open('a.bin', 'wb').write(b'\0\1')
  $ hg commit -m A -A a.bin

  >>> open('a.bin', 'wb').write(b'\0\2')

  $ hg diff
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed

  $ HGPLAIN=1 hg diff
  diff --git a/a.bin b/a.bin
  index bdc955b7b2e610ad5a72302b139a2e6cb325519a..8835708590a9afa236e1bbad18df9d23de82ccd3
  GIT binary patch
  literal 2
  Jc${Nk0ssI600RI3
  

  $ HGPLAIN=1 HGPLAINEXCEPT=diffopts hg diff
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed
