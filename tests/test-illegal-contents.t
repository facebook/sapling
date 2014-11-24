Check for contents we should refuse to export to git repositories (or
at least warn).

Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ hg init hg
  $ cd hg
  $ mkdir -p .git/hooks
  $ cat > .git/hooks/post-update <<EOF
  > #!/bin/sh
  > echo pwned
  > EOF

  $ hg addremove
  adding .git/hooks/post-update
  $ hg ci -m "we should refuse to export this"
  $ hg book master
  $ hg gexport
  abort: Refusing to export likely-dangerous path '.git/hooks/post-update'
  (If you need to continue, read about CVE-2014-9390 and then set '[git] blockdotgit = false' in your hgrc.)
  [255]
  $ cd ..

  $ rm -rf hg
  $ hg init hg
  $ cd hg
  $ mkdir -p nested/.git/hooks/
  $ cat > nested/.git/hooks/post-update <<EOF
  > #!/bin/sh
  > echo pwnd
  > EOF
  $ chmod +x nested/.git/hooks/post-update
  $ hg addremove
  adding nested/.git/hooks/post-update
  $ hg ci -m "also refuse to export this"
  $ hg book master
  $ hg gexport
  abort: Refusing to export likely-dangerous path 'nested/.git/hooks/post-update'
  (If you need to continue, read about CVE-2014-9390 and then set '[git] blockdotgit = false' in your hgrc.)
  [255]
We can override if needed:
  $ hg --config git.blockdotgit=false gexport
  warning: path 'nested/.git/hooks/post-update' contains a potentially dangerous path component.
  It may not be legal to check out in Git.
  It may also be rejected by some git server configurations.
  $ cd ..
  $ git clone hg/.hg/git git
  Cloning into 'git'...
  done.
  error: Invalid path 'nested/.git/hooks/post-update'

Now check something that case-folds to .git, which might let you own
Mac users:

  $ cd ..
  $ rm -rf hg
  $ hg init hg
  $ cd hg
  $ mkdir -p .GIT/hooks/
  $ cat > .GIT/hooks/post-checkout <<EOF
  > #!/bin/sh
  > echo pwnd
  > EOF
  $ chmod +x .GIT/hooks/post-checkout
  $ hg addremove
  adding .GIT/hooks/post-checkout
  $ hg ci -m "also refuse to export this"
  $ hg book master
  $ hg gexport
  $ cd ..

And the NTFS case:
  $ cd ..
  $ rm -rf hg
  $ hg init hg
  $ cd hg
  $ mkdir -p GIT~1/hooks/
  $ cat > GIT~1/hooks/post-checkout <<EOF
  > #!/bin/sh
  > echo pwnd
  > EOF
  $ chmod +x GIT~1/hooks/post-checkout
  $ hg addremove
  adding GIT~1/hooks/post-checkout
  $ hg ci -m "also refuse to export this"
  $ hg book master
  $ hg gexport
  abort: Refusing to export likely-dangerous path 'GIT~1/hooks/post-checkout'
  (If you need to continue, read about CVE-2014-9390 and then set '[git] blockdotgit = false' in your hgrc.)
  [255]
  $ cd ..
