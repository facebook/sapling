#require git no-windows

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true experimental.git-index-fast-path=true

  $ enable rebase

Prepare git repo with a simple rebase scenario

  $ git init -q -b main git-repo
  $ cd git-repo

Create a commit history with two branches:
- main branch: A -> B -> C
- feature branch: A -> D -> E

  $ echo "base" > file.txt
  $ git add file.txt
  $ git commit -q -m "A"

  $ echo "b" >> file.txt
  $ git commit -q -a -m "B"

  $ echo "c" >> file.txt
  $ git commit -q -a -m "C"

  $ git checkout -q HEAD~2
  $ echo "d" >> file2.txt
  $ git add file2.txt
  $ git commit -q -m "D"

  $ echo "e" >> file2.txt
  $ git commit -q -a -m "E"

Show the initial commit graph

  $ sl log -G -T "{desc} {node|short}" -r "all()"
  o  C a7e0c7eef96d
  │
  o  B 5335e60e822c
  │
  │ @  E 4e529e688f00
  │ │
  │ o  D 2bcbe4d8cb3e
  ├─╯
  o  A * (glob)

Test rebase with no conflicts

Rebase feature branch (D and E) onto C

  $ sl rebase -s 'desc(D)' -d 'desc(C)'
  rebasing * "D" (glob)
  rebasing * "E" (glob)

Verify the commit graph after rebase

  $ sl log -G -T "{desc} {node|short}" -r "all()"
  @  E * (glob)
  │
  o  D * (glob)
  │
  o  C * (glob)
  │
  o  B * (glob)
  │
  o  A * (glob)

Verify file contents are correct after rebase

  $ cat file.txt
  base
  b
  c

  $ cat file2.txt
  d
  e

Test rebase with conflicts

Set up a conflict scenario:
- main branch: A -> B2 (modifies file.txt)
- feature branch: A -> B1 (also modifies file.txt)

  $ cd ..
  $ git init -q -b main git-conflict-repo
  $ cd git-conflict-repo

  $ echo "base" > file.txt
  $ git add file.txt
  $ git commit -q -m "A"

Create main branch commit

  $ echo "main change" >> file.txt
  $ git commit -q -a -m "B2"

Go back and create conflicting commit

  $ git checkout -q HEAD~1
  $ echo "feature change" >> file.txt
  $ git commit -q -a -m "B1"

Show the initial commit graph

  $ sl log -G -T "{desc} {node|short}" -r "all()"
  o  B2 0a2106c53a84
  │
  │ @  B1 bf648b54ad47
  ├─╯
  o  A * (glob)

Attempt to rebase B1 onto B2 - this should cause a conflict

  $ sl rebase -s 'desc(B1)' -d 'desc(B2)'
  rebasing * "B1" (glob)
  merging file.txt
  warning: 1 conflicts while merging file.txt! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

Verify we're in a conflicted state

  $ sl status
  M file.txt
  ? file.txt.orig (?)

Check the conflict markers

  $ cat file.txt
  base
  <<<<<<< dest:   0a2106c53a84 - test: B2
  main change
  =======
  feature change
  >>>>>>> source: bf648b54ad47 - test: B1

Resolve the conflict

  $ echo "base" > file.txt
  $ echo "main change" >> file.txt
  $ echo "feature change" >> file.txt

Mark the conflict as resolved

  $ sl resolve -m file.txt
  (no more unresolved files)
  continue: sl rebase --continue

Continue the rebase

  $ sl rebase --continue
  rebasing * "B1" (glob)

Verify the commit graph after rebase with resolved conflict

  $ sl log -G -T "{desc} {node|short}" -r "all()"
  @  B1 * (glob)
  │
  o  B2 * (glob)
  │
  o  A * (glob)

Verify the file contents include both changes after conflict resolution

  $ cat file.txt
  base
  main change
  feature change

Test rebase with submodule changes

Create a submodule repository

  $ cd ..
  $ git init -q -b main submodule-repo
  $ cd submodule-repo
  $ echo "sub content 1" > subfile.txt
  $ git add subfile.txt
  $ git commit -q -m "Sub commit 1"
  $ echo "sub content 2" >> subfile.txt
  $ git commit -q -a -m "Sub commit 2"

Create a parent repo with the submodule

  $ cd ..
  $ git init -q -b main parent-repo
  $ cd parent-repo
  $ git submodule --quiet add -b main ../submodule-repo sub
  $ git commit -qm 'Add submodule'

  $ echo "parent file" > parent.txt
  $ git add parent.txt
  $ git commit -q -m "Parent commit 1"

Create a branch that modifies the submodule

  $ git checkout -q HEAD~1
  $ cd sub
  $ git checkout -q HEAD~1
  $ cd ..
  $ git commit -q -a -m "Change submodule to earlier version"

  $ echo "branch file" > branch.txt
  $ git add branch.txt
  $ git commit -q -m "Branch commit"

Show the initial commit graph

  $ sl log -G -T "{desc} {node|short}" -r "all()"
  o  Parent commit 1 223e47d53b0e
  │
  │ @  Branch commit 0303196d7312
  │ │
  │ o  Change submodule to earlier version 356a3e40d2de
  ├─╯
  o  Add submodule * (glob)

Rebase the branch with submodule changes

FIXME:
  $ sl rebase -s 'desc("Change submodule")' -d 'desc("Parent commit 1")'
  rebasing * "Change submodule to earlier version" (glob)
  rebasing 0303196d7312 "Branch commit"

Verify the commit graph after rebase

  $ sl log -G -T "{desc} {node|short}" -r "all()"
  @  Branch commit bce10c8b66c9
  │
  o  Change submodule to earlier version b5fa591278d7
  │
  o  Parent commit 1 223e47d53b0e
  │
  o  Add submodule df1126da9b2b

Verify the submodule is at the correct version

  $ git ls-tree HEAD sub
  160000 commit 807d30b37126b53327b5b29f6501ffde0b9a1756	sub

  $ sl status
