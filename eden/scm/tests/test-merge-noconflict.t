#require no-eden

  $ eagerepo
  $ enable amend rebase
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ newclientrepo

A conflict-free merge of two commits touching different regions of a file is
committed directly and checked out
  $ drawdag <<'EOS'
  > b c   # a/f = 1\n2\n3\n4\n5\n
  > |/    # b/f = one\n2\n3\n4\n5\n
  > a     # b/x = x\n
  >       # c/f = 1\n2\n3\n4\n five\n
  >       # c/y = y\n
  > EOS
  $ sl goto -q $b
  $ setconfig experimental.noconflict-merge=false
  $ sl merge --noconflict $c
  abort: conflict-free merges are disabled by experimental.noconflict-merge
  (set experimental.noconflict-merge=true to enable them)
  [255]
  $ sl log -r . -T '{desc}\n'
  b
  $ setconfig experimental.noconflict-merge=true
  $ sl merge --noconflict $c
  created conflict-free merge * (glob)
  $ cat f
  one
  2
  3
  4
   five
  $ sl status
  $ sl log -r . -T '{desc}\n{p1node|short} {p2node|short}\n{extras % "{key}={value}\n"}'
  Merge c
  * * (glob)
  branch=default
  noconflict_merge=1
  $ sl log -r '.^' -T '{desc}\n'
  b

A merge that would conflict stops at the first affected path without touching
the working copy
  $ drawdag <<'EOS'
  > d e   # d/g = d\n
  > |/    # e/g = e\n
  > a     # d/other = d\n
  >       # e/other = e\n
  > EOS
  $ sl goto -q $d
  $ sl merge --noconflict $e
  abort: merge of * stopped at conflicts in: (glob)
   g
  (run 'sl merge' without --noconflict to resolve them by hand)
  [255]
  $ sl status
  $ sl log -r . -T '{desc} {p2node|short}\n'
  d 000000000000
  $ cat g
  d

Both commits must be draft
  $ drawdag <<'EOS'
  > p     # p/p = p\n
  > |
  > a
  > EOS
  $ sl debugmakepublic $p
  $ sl goto -q $b
  $ sl merge --noconflict $p
  abort: cannot create a conflict-free merge with public commit * (glob)
  (both parents must be draft; rebase onto the public commit instead)
  [255]
  $ sl goto -q $p
  $ sl merge --noconflict $b
  abort: cannot create a conflict-free merge with public commit * (glob)
  (both parents must be draft; rebase onto the public commit instead)
  [255]

The message can be given, and a dirty working copy is refused
  $ sl goto -q $b
  $ echo dirty > x
  $ sl merge --noconflict -m "b and c" $c
  abort: uncommitted changes
  [255]
  $ sl revert -q x
  $ sl merge -q --noconflict -m "b and c" $c
  $ sl log -r . -T '{desc}\n'
  b and c

Changing the content of a conflict-free merge is refused; the message can be
edited
  $ echo edited >> y
  $ sl amend
  abort: cannot amend conflict-free merge *: it must stay the automatic merge of its parents (glob)
  (commit the change on top of it instead)
  [255]
  $ sl revert -q y
  $ rm y.orig
  $ sl amend -m "b and c, renamed"
  $ sl metaedit -m "b and c, renamed twice"
  $ sl log -r . -T '{desc}\n'
  b and c, renamed twice
  $ sl log -r . -T '{extras % "{key}={value}\n"}' | grep noconflict
  noconflict_merge=1
  $ echo d > d
  $ sl commit -Aqm d
  $ sl fold --from .^ -m folded
  abort: cannot fold conflict-free merge * (glob)
  (it must stay the automatic merge of its parents)
  [255]
  $ sl goto -q '.^'
  $ sl uncommit
  abort: cannot uncommit merge changeset
  [255]

It cannot be pushed
  $ sl push -r . --to scratch/test --create
  abort: cannot push conflict-free merge:
    * b and c, renamed twice (glob)
  (such a merge only records that its parents can be merged automatically; push or land the parents and descendants instead)
  [255]

A plain merge is not marked
  $ sl goto -q $b
  $ sl merge -q $c
  $ sl commit -m "plain merge"
  $ sl log -r . -T '{extras % "{key}={value}\n"}'
  branch=default

Conflict-free merges are allowed even where merges are otherwise disabled
  $ setconfig ui.allowmerge=false
  $ sl goto -q $b
  $ sl merge -q $c
  abort: merging is not supported for this repository
  (use rebase, or 'sl merge --noconflict' for a conflict-free merge)
  [255]
  $ sl merge -q --noconflict $c
  $ sl log -r . -T '{extras % "{key}={value}\n"}' | grep noconflict
  noconflict_merge=1

A conflict-free merge can be the parent of another one
  $ drawdag <<'EOS'
  > h     # h/h = h\n
  > |
  > a
  > EOS
  $ sl merge -q --noconflict -m chain $h
  $ sl log -r . -T '{desc}\n{extras % "{key}={value}\n"}'
  chain
  branch=default
  noconflict_merge=1
  $ sl log -r '.^' -T '{desc}\n'
  Merge c
  $ sl log -r '.^2' -T '{desc}\n'
  h

--parent names both parents in order; the working copy is not involved
  $ drawdag <<'EOS'
  > v w   # v/v = v\n
  > |/    # w/w = w\n
  > a
  > EOS
  $ echo dirty > dirty
  $ sl merge --noconflict -m vw --parent $w --parent $v
  created conflict-free merge * (glob)
  $ sl status
  ? dirty
  $ sl log -r . -T '{desc}\n'
  chain
  $ sl log -r "children($v) & children($w)" -T '{desc}\n{extras % "{key}={value}\n"}'
  vw
  branch=default
  noconflict_merge=1
  $ sl log -r "p1(children($v) & children($w))" -T '{desc}\n'
  w
  $ sl log -r "p2(children($v) & children($w))" -T '{desc}\n'
  v
  $ rm dirty
  $ sl merge --noconflict --parent $v
  abort: --parent must be given exactly twice
  [255]
  $ sl merge --noconflict --parent $v --parent $w $b
  abort: --parent cannot be combined with REV or --rev
  [255]
  $ sl merge --parent $v --parent $w
  abort: --parent requires --noconflict
  [255]
  $ sl merge --noconflict --parent $v --parent $v
  abort: cannot create a conflict-free merge of * with itself (glob)
  [255]
  $ sl merge -m plain $c
  abort: --message requires --noconflict
  [255]
  $ sl merge --noconflict --parent $v --parent $p
  abort: cannot create a conflict-free merge with public commit * (glob)
  (both parents must be draft; rebase onto the public commit instead)
  [255]

Adding or removing files with amend -A cannot change a conflict-free merge
  $ newclientrepo
  $ drawdag <<'EOS'
  > b c
  > |/
  > a
  > EOS
  $ sl goto -q $b
  $ sl merge -q --noconflict $c
  $ echo extra > extra
  $ sl amend -q -A -m changed
  abort: cannot amend conflict-free merge *: it must stay the automatic merge of its parents (glob)
  (commit the change on top of it instead)
  [255]
  $ sl cat -r . extra
  extra: no such file in rev * (glob)
  [1]
  $ cat extra
  extra
  $ sl revert -q --all
  $ rm extra
  $ rm b
  $ sl amend -q -A -m changed
  abort: cannot amend conflict-free merge *: it must stay the automatic merge of its parents (glob)
  (commit the change on top of it instead)
  [255]
  $ sl cat -r . b
  b (no-eol)
