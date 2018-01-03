Other tests make sure that gverify passes. This makes sure that gverify detects
inconsistencies. Since hg-git is ostensibly correct, we artificially create
inconsistencies by placing different Mercurial and Git repos in the right spots.

  $ . "$TESTDIR/testutil"
  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo normalf > normalf
  $ echo missingf > missingf
  $ echo differentf > differentf
(executable in git, non-executable in hg)
  $ echo exef > exef
  $ chmod +x exef
(symlink in hg, regular file in git)
equivalent to 'echo -n foo > linkf', but that doesn't work on OS X
  $ printf foo > linkf
  $ git add normalf missingf differentf exef linkf
  $ fn_git_commit -m 'add files'
  $ cd ..

  $ hg init hgrepo
  $ cd hgrepo
  $ echo normalf > normalf
  $ echo differentf2 > differentf
  $ echo unexpectedf > unexpectedf
  $ echo exef > exef
  $ ln -s foo linkf
  $ hg add normalf differentf unexpectedf exef linkf
  $ fn_hg_commit -m 'add files'
  $ git clone --mirror ../gitrepo .hg/git
  Cloning into bare repository '.hg/git'...
  done.
  $ echo "$(cd ../gitrepo && git rev-parse HEAD) $(hg log -r . --template '{node}')" >> .hg/git-mapfile
  $ hg gverify
  verifying rev 3f1601c3cf54 against git commit 039c1cd9fdda382c9d1e8ec85de6b5b59518ca80
  difference in: differentf
  file has different flags: exef (hg '', git 'x')
  file has different flags: linkf (hg 'l', git '')
  file found in git but not hg: missingf
  file found in hg but not git: unexpectedf
  [1]

  $ echo newf > newf
  $ hg add newf
  $ fn_hg_commit -m 'new hg commit'
  $ hg gverify
  abort: no git commit found for rev 4e582b4eb862
  (if this is an octopus merge, verify against the last rev)
  [255]

invalid git SHA
  $ echo "ffffffffffffffffffffffffffffffffffffffff $(hg log -r . --template '{node}')" >> .hg/git-mapfile
  $ hg gverify
  abort: git equivalent ffffffffffffffffffffffffffffffffffffffff for rev 4e582b4eb862 not found!
  [255]

git SHA is not a commit
  $ echo new2 >> newf
  $ fn_hg_commit -m 'new hg commit 2'
this gets the tree pointed to by the commit at HEAD
  $ echo "$(cd ../gitrepo && git show --format=%T HEAD | head -n 1) $(hg log -r . --template '{node}')" >> .hg/git-mapfile
  $ hg gverify
  abort: git equivalent f477b00e4a9907617f346a529cc0fe9ba5d6f6d3 for rev 5c2eb98af3e2 is not a commit!
  [255]
