#chg-compatible
#require git no-windows

  $ setconfig workingcopy.ruststatus=False
  $ . $TESTDIR/git.sh
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig diff.git=true ui.allowemptycommit=true

Prepare a git repo:

  $ git init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha

  $ echo 2 > beta
  $ git add beta
  $ git commit -q -mbeta

Init an hg repo using the git changelog backend:

  $ cd $TESTTMP
  $ hg debuginitgit --git-dir gitrepo/.git repo1
  $ cd repo1

  $ hg log -Gr 'all()' -T '{node} {desc}'
  o  3f5848713286c67b8a71a450e98c7fa66787bde2 beta
  │
  o  b6c31add3e60ded7a9c9c803641edffb1dccd251 alpha
  
  $ hg debugchangelog
  The changelog is backed by Rust. More backend information:
  Backend (segmented git):
    Local:
      Segments + IdMap: $TESTTMP/repo1/.hg/store/segments/v1
      Git: $TESTTMP/gitrepo/.git
  Feature Providers:
    Commit Graph Algorithms:
      Segments
    Commit Hash / Rev Lookup:
      IdMap
    Commit Data (user, message):
      Git

Test checkout:

  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo *
  alpha beta
  $ cat beta
  2

Test nullid:

  $ hg log -r null -T '{desc}'

Test non-existed commit hash:

  $ hg log -r deadbeef00000000000000000000000000000000 -T '{desc}'
  abort: unknown revision 'deadbeef00000000000000000000000000000000'!
  [255]

Test diff:

  $ hg log -r tip -p
  commit:      3f5848713286
  bookmark:    master
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:10 2007 +0000
  summary:     beta
  
  diff --git a/beta b/beta
  new file mode 100644
  --- /dev/null
  +++ b/beta
  @@ -0,0 +1,1 @@
  +2
  
Test status:

  $ hg status
  $ echo 3 > alpha
  $ hg status
  M alpha

Test commit:

  $ hg commit -m alpha3 -d '2001-02-03T14:56:01 +0800'
  $ hg log -Gr: -T '{desc}'
  @  alpha3
  │
  o  beta
  │
  o  alpha
  
Test log FILE:

  $ hg log -G -T '{desc}' alpha
  @  alpha3
  ╷
  o  alpha
  
Test log FILE with patches:

  $ hg log -p -G -T '{desc}\n' alpha
  @  alpha3
  ╷  diff --git a/alpha b/alpha
  ╷  --- a/alpha
  ╷  +++ b/alpha
  ╷  @@ -1,1 +1,1 @@
  ╷  -1
  ╷  +3
  ╷
  o  alpha
     diff --git a/alpha b/alpha
     new file mode 100644
     --- /dev/null
     +++ b/alpha
     @@ -0,0 +1,1 @@
     +1
  

Test bookmarks:

  $ hg bookmark -r. foo
  $ hg bookmarks
     foo                       4899b7b71a9c
     master                    3f5848713286

Test changes are readable via git:

  $ export GIT_DIR="$TESTTMP/gitrepo/.git"
  $ git log foo --pretty='format:%s %an %d'
  alpha3 test  (refs/visibleheads/4899b7b71a9c241a7c43171f525cc9d6fcabfd4f, foo)
  beta test  (HEAD -> master)
  alpha test  (no-eol)
  $ git fsck --strict
  $ git show foo
  commit 4899b7b71a9c241a7c43171f525cc9d6fcabfd4f
  Author: test <>
  Date:   Sat Feb 3 14:56:01 2001 +0800
  
      alpha3
  
  diff --git a/alpha b/alpha
  index d00491f..00750ed 100644
  --- a/alpha
  +++ b/alpha
  @@ -1 +1 @@
  -1
  +3

Exercise pathcopies code path:

  $ hg diff -r '.^^' -r .
  diff --git a/alpha b/alpha
  --- a/alpha
  +++ b/alpha
  @@ -1,1 +1,1 @@
  -1
  +3
  diff --git a/beta b/beta
  new file mode 100644
  --- /dev/null
  +++ b/beta
  @@ -0,0 +1,1 @@
  +2

Prepare a new git "client" repo:

  $ unset GIT_DIR
  $ git init -q --bare $TESTTMP/gitrepo2
  $ cd "$TESTTMP/gitrepo2"
  $ git remote add origin "$TESTTMP/gitrepo/.git"
  $ hg debuginitgit --git-dir="$TESTTMP/gitrepo2" "$TESTTMP/repo2"
  $ cd "$TESTTMP/repo2"

Test pull:

  $ hg paths -a origin "file://$TESTTMP/gitrepo/.git"

- tree prefetch config is ignored

  $ setconfig treemanifest.pullprefetchrevs=tip

- pull with -B
  $ hg pull origin -B foo
  pulling from file:/*/$TESTTMP/gitrepo/.git (glob)
  From file:/*/$TESTTMP/gitrepo/ (glob)
   * [new ref]         4899b7b71a9c241a7c43171f525cc9d6fcabfd4f -> origin/foo
  $ hg log -r origin/foo -T '{desc}\n'
  alpha3

- pull with -B and --update
  $ hg pull -q origin -B master --update
  $ hg log -r . -T '{remotenames}\n'
  origin/master

  $ hg pull -q origin -B foo --update
  $ hg log -r . -T '{remotenames}\n'
  origin/foo

- pull with -B and --update with wrong tweakdefaults dynamicconfig configuration
  $ cat > "$TESTTMP/buggy.rc" << EOF
  > [tweakdefaults]
  > defaultdest=nonexisted
  > EOF
  $ HG_TEST_DYNAMICCONFIG="$TESTTMP/buggy.rc" hg pull origin -B master --update --config extensions.tweakdefaults=
  pulling from file:/*/$TESTTMP/gitrepo/.git (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

- pull without arguments
  $ hg paths -a default "file://$TESTTMP/gitrepo/.git"
  $ hg pull
  pulling from file:/*/$TESTTMP/gitrepo/.git (glob)

- infinitepush compatibility
  $ hg pull --config extensions.infinitepush=
  pulling from file:/*/$TESTTMP/gitrepo/.git (glob)

Test error message display:

  $ mkdir $TESTTMP/errortest
  $ cd $TESTTMP/errortest
  $ hg clone --git "$TESTTMP/nonexisted"
  abort: git command failed with exit code 128
    git * (glob)
      fatal: '$TESTTMP/nonexisted' does not appear to be a git repository
      fatal: Could not read from remote repository.
  
      Please make sure you have the correct access rights
      and the repository exists.
  [255]

Test clone with flags (--noupdate, --updaterev):

  $ mkdir $TESTTMP/clonetest
  $ cd $TESTTMP/clonetest

  $ hg clone -q --noupdate --git "$TESTTMP/gitrepo"
  $ cd gitrepo
  $ hg log -r . -T '{node|short}\n'
  000000000000
  $ hg bookmarks --remote
     remote/master             3f5848713286
  $ cd ..

  $ hg clone --git "$TESTTMP/gitrepo" cloned1 --config remotenames.selectivepulldefault=foo,master
  From $TESTTMP/gitrepo
   * [new ref]         4899b7b71a9c241a7c43171f525cc9d6fcabfd4f -> remote/foo
   * [new ref]         3f5848713286c67b8a71a450e98c7fa66787bde2 -> remote/master
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd cloned1 log -r . -T '{node|short} {remotenames} {desc}\n'
  4899b7b71a9c remote/foo alpha3
  $ cd ..

  $ hg clone --updaterev remote/foo --git "$TESTTMP/gitrepo" cloned2 --config remotenames.selectivepulldefault=foo
  From $TESTTMP/gitrepo
   * [new ref]         4899b7b71a9c241a7c43171f525cc9d6fcabfd4f -> remote/foo
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd cloned2 log -r . -T '{node|short} {remotenames} {desc}\n'
  4899b7b71a9c remote/foo alpha3
  $ cd ..

Test clone using scp-like path:

#if execbit no-windows
  $ cat > "$TESTTMP/ssh.sh" << 'EOF'
  > #!/bin/sh
  > # $GIT_SSH host "git-upload-pack '/path/to/repo'"
  > ssh() {
  >   local cmd=$1
  >   local path=$(eval echo $2)  # unquote '/path' to /path
  >   $cmd "$TESTTMP$path"
  > }
  > ssh $2
  > EOF
  $ chmod +x "$TESTTMP/ssh.sh"
  $ GIT_SSH="$TESTTMP/ssh.sh" hg clone localhost:gitrepo2 cloned-gitrepo2
  $ hg status --cwd cloned-gitrepo2
  $ hg paths --cwd cloned-gitrepo2
  default = ssh://localhost/gitrepo2
#endif

Test clone when destination is a file:

  $ touch "$TESTTMP/already_exists"
  $ hg clone -q --git "$TESTTMP/gitrepo" "$TESTTMP/already_exists"
  abort: destination '$TESTTMP/already_exists' already exists
  [255]

Test clone when folder is not empty:

  $ mkdir "$TESTTMP/not_quite_empty"
  $ touch "$TESTTMP/not_quite_empty/some_file"
  $ hg clone -q --git "$TESTTMP/gitrepo" "$TESTTMP/not_quite_empty"
  abort: destination '$TESTTMP/not_quite_empty' is not empty
  [255]

Test clone into the current folder, if empty:

  $ mkdir "$TESTTMP/empty_folder"
  $ cd "$TESTTMP/empty_folder"
  $ hg clone -q --git "$TESTTMP/gitrepo" .
  $ ls -A1 | sort -d
  alpha
  beta
  .hg

Make sure we clean up if repo init fails:

  $ FAILPOINTS=repo-init=panic hg clone -q --git "$TESTTMP/gitrepo" "$TESTTMP/init_fails" &> /dev/null
  [1]
  $ test -d "$TESTTMP/init_fails"
  [1]

Test push:

  $ cd "$TESTTMP/clonetest/cloned1"
  $ echo 3 > beta
  $ hg commit -m 'beta.change'

- --to without -r
  $ hg push -q --to book_change_beta

- --to with -r
  $ hg push -r '.^' --to parent_change_beta
  To $TESTTMP/gitrepo
   * [new branch]      4899b7b71a9c241a7c43171f525cc9d6fcabfd4f -> parent_change_beta

  $ hg log -r '.^+.' -T '{desc} {remotenames}\n'
  alpha3 remote/foo remote/parent_change_beta
  beta.change remote/book_change_beta

- delete bookmark
  $ hg push --delete book_change_beta
  To $TESTTMP/gitrepo
   - [deleted]         book_change_beta

  $ hg log -r '.^+.' -T '{desc} {remotenames}\n'
  alpha3 remote/foo remote/parent_change_beta
  beta.change 

- infinitepush compatibility
  $ hg push -q -r '.^' --to push_with_infinitepush --config extensions.infinitepush=

- push with --force

  $ cd "$TESTTMP"
  $ git init -qb main --bare "pushforce.git"
  $ hg clone --git "$TESTTMP/pushforce.git"
  $ cd pushforce
  $ git --git-dir=.hg/store/git config advice.pushUpdateRejected false

  $ drawdag << 'EOS'
  > B C
  > |/
  > A
  > EOS

  $ hg push -qr $B --to foo
  $ hg push -qr $C --to foo
  To $TESTTMP/pushforce.git
   ! [rejected]        5d38a953d58b0c80a4416ba62e62d3f2985a3726 -> foo (non-fast-forward)
  error: failed to push some refs to '$TESTTMP/pushforce.git'
  [1]
  $ hg push -qr $C --to foo --force

- push without --to

  $ cd "$TESTTMP"
  $ git init -qb main --bare "pushto.git"
  $ hg clone --git "$TESTTMP/pushto.git"
  $ cd pushto

  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ hg push -qr $A --to stable
  $ hg push -qr $B --to main
  $ hg up -q $B
  $ hg commit -m C

 (pick "main" automatically)
  $ hg push
  To $TESTTMP/pushto.git
     0de3093..a9d5bd6  a9d5bd6ac8bcf89de9cd99fd215cca243e8aeed9 -> main
  $ hg push -q --to stable

 (cannot pick with multiple candidates)
  $ hg commit -m D
  $ hg push
  abort: use '--to' to specify destination bookmark
  [255]

"files" metadata:

  $ hg log -r $A+$B -T '{files}\n'
  A
  B

Submodule does not cause a crash:

  $ cd
  $ git init -q submod
  $ cd submod

  $ git submodule --quiet add ../gitrepo b
  $ echo 1 > a
  $ echo 2 > c
  $ git add a c
  $ git commit --quiet -m s

- checkout silently ignores the submodule

  $ cd
  $ setconfig git.submodules=false
  $ hg clone --git "$TESTTMP/submod" cloned-submod
  From $TESTTMP/submod
   * [new ref]         a4c97159e197fb3aaab3f24fc3b39d7942b311ff -> remote/master
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd cloned-submod
  $ echo *
  a c

- changing the tree does not lose submodule

  $ touch d
  $ hg commit -m d -A d
  $ hg book changed
  $ git --git-dir=.hg/store/git cat-file -p changed:
  100644 blob 703feeadc77c10eeec4dfe76ae58506b6a77ab11	.gitmodules
  100644 blob d00491fd7e5bb6fa28c517a0bb32b8b506539d4d	a
  160000 commit 3f5848713286c67b8a71a450e98c7fa66787bde2	b
  100644 blob 0cfbf08886fca9a91cb753ec8734c84fcbe52c9f	c
  100644 blob e69de29bb2d1d6434b8b29ae775ad8c2e48c5391	d

Tags are ignored during clone and pull:

  $ cd
  $ git init -b main -q gittag
  $ cd gittag
  $ echo 1 > a
  $ git add a
  $ git commit -q -m a
  $ git tag v1

  $ cd
  $ hg clone -q git+file://$TESTTMP/gittag cloned-gittag
  $ cd cloned-gittag
  $ hg pull -q
  $ hg bookmarks
  no bookmarks set
  $ hg bookmarks --remote
     remote/main               379d702a285c
  $ git --git-dir=.hg/store/git for-each-ref
  379d702a285c1e34e6365cc347249ec73bcd6b40 commit	refs/remotes/remote/main

Cloud sync does not crash:

  $ enable commitcloud infinitepush
  $ hg cloud sync
  abort: commitcloud: workspace error: undefined workspace
  (your repo is not connected to any workspace)
  (use 'hg cloud join --help' for more details)
  [255]

Init with --git:

  $ cd
  $ hg init --git init-git
  $ cd init-git
  $ [ -d $TESTTMP/init-git/.hg/store/git ]
  $ hg log
  $ hg status

Rebase merging conflicts

  $ cd
  $ hg init --git rebase
  $ cd rebase
  $ enable rebase
  $ drawdag << 'EOS'
  > B C  # A/f=1\n2\n3\n
  > |/   # B/f=1\n1.5\n2\n3\n
  > A    # C/f=1\n2\n2.5\n3\n
  > EOS
  $ hg rebase -r $B -d $C
  rebasing e03992db70e4 "B"
  merging f

Rebasing an add conflict
  $ cd
  $ hg init --git rebase-add
  $ cd rebase-add
  $ enable rebase
  $ touch base
  $ hg commit -Aqm base
  $ echo 2 > file1
  $ hg commit -Aqm file1.a
  $ hg book dest
  $ hg up .^
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark dest)
  $ echo 1 > file1
  $ hg commit -Aqm file1.b
  $ hg rebase -d dest
  rebasing ce2d49965394 "file1.b"
  merging file1
  warning: 1 conflicts while merging file1! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Test amend:

  $ cd
  $ hg init --git amend
  $ cd amend
  $ enable amend
  $ echo 1 > file
  $ hg commit -Aqm 'one'
  $ echo 2 > file
  $ hg commit -Aqm 'base'
  $ echo 3 > file
# This used to trigger a bug.
  $ HGEDITOR=cat hg commit --amend --config committemplate.changeset='{diff()}'
  diff --git a/file b/file
  --- a/file
  +++ b/file
  @@ -1,1 +1,1 @@
  -1
  +3

Init with --git works without a reponame:

  $ cd
  $ grep -v reponame $HGRCPATH > $TESTTMP/hgrc-no-reponame
  $ HGRCPATH=$TESTTMP/hgrc-no-reponame hg init --git init-git-no-reponame
