#chg-compatible
#debugruntest-compatible

  $ enable commitextras
  $ enable globalrevs

  $ newrepo

Test with a valid svnrev

  $ echo data > file
  $ hg add file
  $ hg commit -m "foo" --extra "convert_revision=svn:1234/foo/trunk/bar@4567"
  $ hg log -T '{globalrev}\n' -r .
  4567

Test with an invalid svnrev

  $ echo moredata > file
  $ hg commit -m "foo" --extra "convert_revision=111111"
  $ hg log -T '{globalrev}\n' -r .
  
