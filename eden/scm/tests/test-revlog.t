#chg-compatible

  $ hg init empty-repo
  $ cd empty-repo

Flags on revlog version 0 are rejected

  >>> with open('.hg/store/00changelog.i', 'wb') as fh:
  ...     fh.write('\x00\x01\x00\x00')

  $ hg log
  abort: repo is corrupted: 00changelog.i
  [255]

Unknown flags on revlog version 1 are rejected

  >>> with open('.hg/store/00changelog.i', 'wb') as fh:
  ...     fh.write('\x00\x04\x00\x01')

  $ hg log
  abort: unknown flags (0x04) in version 1 revlog 00changelog.i!
  [255]

Unknown version is rejected

  >>> with open('.hg/store/00changelog.i', 'wb') as fh:
  ...     fh.write('\x00\x00\x00\x02')

  $ hg log
  abort: unknown version (2) in revlog 00changelog.i!
  [255]

  $ cd ..

Test for CVE-2016-3630

  $ hg init

  >>> open("a.i", "w").write(
  ... """eJxjYGZgZIAAYQYGxhgom+k/FMx8YKx9ZUaKSOyqo4cnuKb8mbqHV5cBCVTMWb1Cwqkhe4Gsg9AD
  ... Joa3dYtcYYYBAQ8Qr4OqZAYRICPTSr5WKd/42rV36d+8/VmrNpv7NP1jQAXrQE4BqQUARngwVA=="""
  ... .decode("base64").decode("zlib"))

  $ hg debugindex a.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      19     -1       2 99e0332bd498 000000000000 000000000000
       1        19      12      0       3 6674f57a23d8 99e0332bd498 000000000000
  $ hg debugdata a.i 1 2>&1 | egrep 'Error:.*decoded'
  (mercurial\.\w+\.mpatch\.)?mpatchError: patch cannot be decoded (re)
