  $ . helpers-usechg.sh
  $ enable amend rebase
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.singletransaction=True
  $ setconfig experimental.evolution.allowdivergence=True
  $ setconfig experimental.evolution="createmarkers, allowunstable"
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg ci -m "add $1"
  > }

Test invalid value for amend.autorestack
  $ newrepo
  $ setconfig amend.autorestack=test
  $ hg debugdrawdag<<'EOS'
  > C              # C/file = 1\n2\n3\n4\n
  > |              # B/file = 1\n2\n
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg amend -m "new message"
  invalid amend.autorestack config of "test"; falling back to only-trivial
  rebasing ca039b450ae0 "C" (C)
  hint[amend-autorebase]: descendants have been auto-rebased because no merge conflict could have happened - use --no-rebase or set commands.amend.autorebase=False to disable auto rebase
  hint[hint-ack]: use 'hg hint --ack amend-autorebase' to silence these hints

If they disabled amend.autorestack, disable the new behavior (for now, during rollout)
  $ newrepo
  $ setconfig commands.amend.autorebase=False
  $ setconfig amend.autorestack=always
  $ hg debugdrawdag<<'EOS'
  > C              # C/file = 1\n2\n3\n4\n
  > |              # B/file = 1\n2\n
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg amend -m "new message"
  hint[amend-restack]: descendants of fe14e2b67b65 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

amend.autorestack=only-trivial, and simple changes (expect restack)
  $ newrepo
  $ setconfig amend.autorestack=only-trivial
  $ hg debugdrawdag<<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg amend -m 'Unchanged manifest for B'
  rebasing 26805aba1e60 "C" (C)
  hint[amend-autorebase]: descendants have been auto-rebased because no merge conflict could have happened - use --no-rebase or set commands.amend.autorebase=False to disable auto rebase
  hint[hint-ack]: use 'hg hint --ack amend-autorebase' to silence these hints
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [426bad] A
  $ hg amend -m 'Unchanged manifest for A'
  rebasing 5357953e3ea3 "Unchanged manifest for B"
  rebasing b635bd2cf20b "C"
  hint[amend-autorebase]: descendants have been auto-rebased because no merge conflict could have happened - use --no-rebase or set commands.amend.autorebase=False to disable auto rebase
  hint[hint-ack]: use 'hg hint --ack amend-autorebase' to silence these hints

amend.autorestack=never
  $ newrepo
  $ setconfig amend.autorestack=never
  $ hg debugdrawdag<<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg amend -m 'Unchanged manifest for B'
  hint[amend-restack]: descendants of 112478962961 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [426bad] A
  $ hg amend -m 'Unchanged manifest for A'
  hint[amend-restack]: descendants of 426bada5c675 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

amend.autorestack=only-trivial, and manifest changes (expect no restack)
  $ newrepo
  $ setconfig amend.autorestack=only-trivial
  $ hg debugdrawdag<<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ echo 'new b' > B
  $ hg amend -m 'Change manifest for B'
  hint[amend-restack]: descendants of 112478962961 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

amend.autorestack=only-trivial, and dirty working copy (expect no restack)
  $ newrepo
  $ setconfig amend.autorestack=only-trivial
  $ hg debugdrawdag<<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ echo 'new b' > B
  $ hg amend a -m 'Unchanged manifest, but dirty workdir'
  a: $ENOENT$ (?)
  hint[amend-restack]: descendants of 112478962961 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

amend.autorestack=only-trivial, and no manifest changes, but no children (expect no restack)
  $ newrepo
  $ setconfig amend.autorestack=only-trivial
  $ hg debugdrawdag<<'EOS'
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg amend -m 'Unchanged manifest for B'

amend.autorestack=no-conflict, and mergeable changes (expect restack)
  $ newrepo
  $ setconfig amend.autorestack=no-conflict
  $ setconfig amend.autorestackmsg="custom autorestack message"
  $ hg debugdrawdag<<'EOS'
  > C              # C/file = 1\n2\n3\n4\n
  > |              # B/file = 1\n2\n
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ $TESTDIR/seq.py 0 2 > file
  $ hg amend
  custom autorestack message
  rebasing ca039b450ae0 "C" (C)
  merging file
  $ showgraph
  o  4 7ed7d67ad7bf C
  |
  @  3 767372f778c5 B
  |
  | x  2 ca039b450ae0 C
  | |
  | x  1 fe14e2b67b65 B
  |/
  o  0 426bada5c675 A
  $ cat file
  0
  1
  2

amend.autorestack=no-conflict, and mergeable changes, but dirty WC (expect no restack)
  $ newrepo
  $ setconfig amend.autorestack=no-conflict
  $ hg debugdrawdag<<'EOS'
  > C              # C/file = 1\n2\n3\n4\n
  > |              # B/file = 1\n2\n
  > B              # A/other = i don't matter
  > |
  > A
  > EOS
  $ hg update B -q
  $ echo "new content" > other
  $ $TESTDIR/seq.py 0 2 > file
  $ cat <<EOS | hg amend -i --config ui.interactive=1
  > y
  > y
  > n
  > EOS
  diff --git a/file b/file
  1 hunks, 1 lines changed
  examine changes to 'file'? [Ynesfdaq?] y
  
  @@ -1,2 +1,3 @@
  +0
   1
   2
  record change 1/2 to 'file'? [Ynesfdaq?] y
  
  diff --git a/other b/other
  1 hunks, 1 lines changed
  examine changes to 'other'? [Ynesfdaq?] n
  
  not restacking because working copy is dirty
  hint[amend-restack]: descendants of bf943f2ff2de are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints




amend.autorestack=no-conflict, and conflicting changes (expect cancelled restack)
  $ newrepo
  $ setconfig amend.autorestack=no-conflict
  $ hg debugdrawdag<<'EOS'
  > D
  > |
  > C              # D/file = 1\n2\n3\n4\n
  > |              # B/file = 1\n2\n
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ echo 'unmergeable!' > file
  $ hg amend
  restacking children automatically (unless they conflict)
  rebasing b6c0d35dc9e9 "C" (C)
  rebasing 02cc3cc1d010 "D" (D)
  merging file
  transaction abort!
  rollback completed
  restacking would create conflicts (hit merge conflicts in file), so you must run it manually
  (run `hg restack` manually to restack this commit's children)
  $ showgraph
  @  4 3000de962fa1 B
  |
  | o  3 02cc3cc1d010 D
  | |
  | o  2 b6c0d35dc9e9 C
  | |
  | x  1 fe14e2b67b65 B
  |/
  o  0 426bada5c675 A
  $ cat file
  unmergeable!

amend.autorestack=always, and conflicting changes (expect restack)
  $ newrepo
  $ setconfig amend.autorestack=always
  $ hg debugdrawdag<<'EOS'
  > D
  > |
  > C              # D/file = 1\n2\n3\n4\n
  > |              # B/file = 1\n2\n
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ echo 'unmergeable!' > file
  $ hg amend
  rebasing b6c0d35dc9e9 "C" (C)
  rebasing 02cc3cc1d010 "D" (D)
  merging file
  hit merge conflicts (in file); switching to on-disk merge
  rebasing 02cc3cc1d010 "D" (D)
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ cat file
  unmergeable!
  $ showgraph
  @  4 3000de962fa1 B
  |
  | o  3 02cc3cc1d010 D
  | |
  | o  2 b6c0d35dc9e9 C
  | |
  | x  1 fe14e2b67b65 B
  |/
  o  0 426bada5c675 A

Test rebasing children with obsolete children themselves needing a restack.
  $ newrepo
  $ setconfig amend.autorestack=no-conflict
  $ hg debugdrawdag<<'EOS'
  > D
  > |
  > C C2  # amend: C -> C2
  > |/
  > B
  > |
  > A     # <-- then amend this
  > |
  > Z
  > EOS
  $ hg update A -q
  $ echo "new value" > A
  $ hg amend
  restacking children automatically (unless they conflict)
  rebasing 917a077edb8d "B" (B)
  rebasing ff9eba5e2480 "C2" (C2)
  rebasing 01f26f1a10b2 "D" (D)
  $ showgraph
  o  9 0a75af8fc6e3 D
  |
  o  8 84f362759e03 C2
  |
  o  7 23018262b14e B
  |
  @  6 21006be03678 A
  |
  | x  5 01f26f1a10b2 D
  | |
  | | x  4 ff9eba5e2480 C2
  | | |
  | x |  3 b45c90359798 C
  | |/
  | x  2 917a077edb8d B
  | |
  | x  1 ac2f7407182b A
  |/
  o  0 48b9aae0607f Z

Test not rebasing unrelated changes. When rebasing X, only X:: are expected to be rebased.
Rebasing commits outside X:: can be surprising and more easily cause conflicts.

  $ newrepo
  $ setconfig amend.autorestack=no-conflict
  $ hg debugdrawdag<<'EOS'
  > D  C
  > |  |
  > B2 B1  # amend: B1 -> B2
  > | /    # then, amend B2.
  > |/     # expect: D gets rebased, while C is kept unchanged.
  > A
  > |
  > Z
  > EOS
  $ hg up -q B2
  $ hg amend -m B3
  restacking children automatically (unless they conflict)
  rebasing afb1812f5f28 "D" (D)
  $ showgraph
  o  7 c9dfcf01df0b D
  |
  @  6 1c28c4186c15 B3
  |
  | x  5 afb1812f5f28 D
  | |
  | | o  4 dbe6ebcaec86 C
  | | |
  | x |  3 1b9d31199d34 B2
  |/ /
  | x  2 588f87b965af B1
  |/
  o  1 ac2f7407182b A
  |
  o  0 48b9aae0607f Z

