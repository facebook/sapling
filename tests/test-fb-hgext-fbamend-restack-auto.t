  $ . helpers-usechg.sh
  $ enable fbamend inhibit rebase
  $ setconfig experimental.evolution.allowdivergence=True
  $ setconfig experimental.evolution="createmarkers, allowunstable"
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg ci -m "add $1"
  > }
  $ showgraph() {
  >   hg log --graph -T "{rev} {desc|firstline}" | sed \$d
  > }

Test auto-restack heuristics - no changes to manifest and clean working directory
  $ newrepo
  $ hg debugdrawdag<<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg amend -m 'Unchanged manifest for B'
  rebasing 2:26805aba1e60 "C" (C)
  hint[amend-autorebase]: descendants have been auto-rebased because no merge conflict could have happened - use --no-rebase or set commands.amend.autorebase=False to disable auto rebase
  hint[hint-ack]: use 'hg hint --ack amend-autorebase' to silence these hints
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [426bad] A
  $ hg amend -m 'Unchanged manifest for A'
  rebasing 3:5357953e3ea3 "Unchanged manifest for B"
  rebasing 4:b635bd2cf20b "C"
  hint[amend-autorebase]: descendants have been auto-rebased because no merge conflict could have happened - use --no-rebase or set commands.amend.autorebase=False to disable auto rebase
  hint[hint-ack]: use 'hg hint --ack amend-autorebase' to silence these hints

Test commands.amend.autorebase=False flag - no changes to manifest and clean working directory
  $ newrepo
  $ hg debugdrawdag<<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg amend --config commands.amend.autorebase=False -m 'Unchanged manifest for B'
  hint[amend-restack]: descendants of 112478962961 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [426bad] A
  $ hg amend --config commands.amend.autorebase=False -m 'Unchanged manifest for A'
  hint[amend-restack]: descendants of 426bada5c675 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

Test auto-restack heuristics - manifest changes
  $ newrepo
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

Test auto-restack heuristics - no committed changes to manifest but dirty working directory
  $ newrepo
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
  hint[amend-restack]: descendants of 112478962961 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

Test auto-restack heuristics - no changes to manifest but no children
  $ newrepo
  $ hg debugdrawdag<<'EOS'
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg amend -m 'Unchanged manifest for B'
