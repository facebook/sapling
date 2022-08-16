#chg-compatible
#debugruntest-compatible

  $ newrepo
  $ setconfig diff.git=1 diff.hashbinary=1

  >>> _ = open('a.bin', 'wb').write(b'\0\1')
  $ hg commit -m A -A a.bin

  >>> _ = open('a.bin', 'wb').write(b'\0\2')

  $ hg diff
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed to 9ac521e32f8e19473bc914e1af8ae423a6d8c122

  $ HGPLAIN=1 hg diff
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed to 9ac521e32f8e19473bc914e1af8ae423a6d8c122

  $ HGPLAIN=1 HGPLAINEXCEPT=diffopts hg diff
  diff --git a/a.bin b/a.bin
  Binary file a.bin has changed to 9ac521e32f8e19473bc914e1af8ae423a6d8c122

  $ hg rm a.bin -f

  $ hg diff
  diff --git a/a.bin b/a.bin
  deleted file mode 100644
  Binary file a.bin has changed

  $ HGPLAIN=1 hg diff
  diff --git a/a.bin b/a.bin
  deleted file mode 100644
  Binary file a.bin has changed

  $ HGPLAIN=1 HGPLAINEXCEPT=diffopts hg diff
  diff --git a/a.bin b/a.bin
  deleted file mode 100644
  Binary file a.bin has changed
