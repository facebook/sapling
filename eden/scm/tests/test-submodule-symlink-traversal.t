#require git symlink no-eden

Test that submodule checkout audits paths for symlink traversal. If a symlink
exists in the working directory and a submodule shares its name, makedirs()
would follow the symlink without the audit check. This validates Fix 2.

  $ . $TESTDIR/git.sh

Create a payload submodule repo:

  $ git init -q payload
  $ cd payload
  $ echo "malicious" > data
  $ git add data && git commit -q -m "payload"
  $ cd ..

Create a git repo with a submodule named "sub":

  $ git init -q repo
  $ cd repo
  $ echo "readme" > README.md
  $ git add README.md && git commit -q -m "init"

  $ printf "[submodule \"sub\"]\n    path = sub\n    url = $TESTTMP/payload\n" > "$TESTTMP/gitmodules_file"
  $ GITMODULES_HASH=$(git hash-object -w "$TESTTMP/gitmodules_file")
  $ README_HASH=$(git rev-parse HEAD:README.md)
  $ PAYLOAD_COMMIT=$(cd "$TESTTMP/payload" && git rev-parse HEAD)
  $ ROOT_TREE=$(printf "100644 blob ${GITMODULES_HASH}\t.gitmodules\n100644 blob ${README_HASH}\tREADME.md\n160000 commit ${PAYLOAD_COMMIT}\tsub" | git mktree)
  $ SUBMOD_COMMIT=$(echo "Add submodule" | git commit-tree "${ROOT_TREE}" -p HEAD)
  $ git update-ref refs/heads/master "$SUBMOD_COMMIT"
  $ cd ..

Clone the repo but don't update working copy:

  $ hg clone --git "$TESTTMP/repo" workdir --noupdate -q

Plant a symlink "sub" -> ".sl" in the working directory before checkout.
This simulates what happens on a case-insensitive FS when a symlink "Sub"
and submodule "sub" case-fold-collide.

  $ ln -s .sl workdir/sub

Now update to master which has the submodule at path "sub". The path auditor
should detect that "sub" is a symlink and reject the operation.

  $ cd workdir
  $ hg up remote/master
  abort: submodule path 'sub' is a symlink
  [255]

The working directory only has the planted symlink, no .sl/config compromise:

  $ readlink sub
  .sl
