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
  (mercurial.mpatch.)?mpatchError: patch cannot be decoded (re)
