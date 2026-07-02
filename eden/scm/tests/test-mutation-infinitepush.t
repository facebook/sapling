#require bash no-eden

  $ enable amend rebase histedit fbhistedit
  $ setconfig experimental.evolution=obsolete
  $ setconfig visibility.enabled=true
  $ setconfig mutation.record=true mutation.enabled=true

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ cp $HGRCPATH $TESTTMP/defaulthgrc
  $ setupcommon
  $ newserver repo
  $ setupserver
  $ cd ..
  $ sl clone ssh://user@dummy/repo client -q
  $ cd client
  $ mkcommit initialcommit
  $ sl push -q -r . --create --to foo
  $ mkcommit scratchcommit

Make a scratch branch with an initial commit.
  $ sl push -q -r . --to scratch/mybranch --create

Amend the commit a couple of times and push to the scratch branch again
  $ sl amend -m "scratchcommit (amended 1)"
  $ sl amend -m "scratchcommit (amended 2)"
  $ sl push -q -r . --to scratch/mybranch --non-forward-move

Clone the repo again, and pull the scratch branch.
  $ cd ..
  $ sl clone ssh://user@dummy/repo client2 -q
  $ cd client2
  $ sl pull -q -B scratch/mybranch

Amend the commit a couple of times again.
  $ sl up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl amend -m "scratchcommit (amended 3)"
  $ sl amend -m "scratchcommit (amended 4)"
  $ sl push -q -r . --to scratch/mybranch --non-forward-move

Pull the branch back into the original repo.
  $ cd ..
  $ cd client
  $ sl pull -B scratch/mybranch
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ sl up -q tip

We have the predecessor chain that links all versions of the commits.
This works even though we are missing the third amended version.
  $ sl log -r 'predecessors(.)' -T '{node|short} {desc}\n' --hidden
  20759b6926ce scratchcommit
  ef7d26c88be0 scratchcommit (amended 1)
  598fd30ad501 scratchcommit (amended 2)
  f1f3b31bcda8 scratchcommit (amended 4)

Something more complicated involving splits and folds.
  $ drawdag --print <<EOS
  >     E      # split: A -> C,D
  >     |      # rebase: B -> E
  >  B  D F    # fold: D, E -> F
  >  |  |/
  >  A  C
  >  |  |
  >  f1f3b
  > EOS
  f9407b1692b9 A
  80b4b0467fc6 B
  91713f37cee7 C
  113fbb421191 D
  798e89a318d8 E
  9d3e6062ef0c F
  f1f3b31bcda8 f1f3b
  $ A=f9407b1692b9
  $ C=91713f37cee7
  $ sl up -q $F

Push commit A to a scratch branch, simulating a pushbackup.
  $ sl push --to scratch/$A -r $A --create --hidden
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 2 commits:
  remote:     f1f3b31bcda8  scratchcommit (amended 4)
  remote:     f9407b1692b9  A

Push the current commit to the scratch branch.
  $ sl push --to scratch/mybranch -r .
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 3 commits:
  remote:     f1f3b31bcda8  scratchcommit (amended 4)
  remote:     91713f37cee7  C
  remote:     9d3e6062ef0c  F

Pull the scratch branch and commit A into the repo.
  $ cd ..
  $ cd client2
  $ sl pull -q -B scratch/mybranch
  $ sl pull -q -r $A
  $ sl up -q $F

The predecessor information successfully reaches from F to A
  $ sl log -r "predecessors(.)" -T "{node|short} {desc}\n" --hidden
  9d3e6062ef0c F
  f9407b1692b9 A

The successor information succesfully reaches from A to C and F (it was split)
  $ sl log -r "successors($A)" -T "{node|short} {desc}\n" --hidden
  91713f37cee7 C
  9d3e6062ef0c F
  f9407b1692b9 A

Clone the repo again, and pull an earlier commit.  This will cause the server to rebundle.
  $ cd ..
  $ sl clone ssh://user@dummy/repo client3 -q
  $ cd client3
  $ sl pull -q -r $C

Check the history of the commits has been included.
  $ sl debugmutation -r f1f3b
   *  f1f3b31bcda86dbc8fe6a31ba7c6893bee792127 amend by test at 1970-01-01T00:00:00 from:
      01c5cd3313b899dca3e059b77aa454e0e2b5df7b amend by test at 1970-01-01T00:00:00 from:
      598fd30ad50172d0389be262a242092f221bd196 amend by test at 1970-01-01T00:00:00 from:
      ef7d26c88be0bf3c8d40a3569fd9c018f32a19ab amend by test at 1970-01-01T00:00:00 from:
      20759b6926ce827d5a8c73eb1fa9726d6f7defb2
  

Pulling in an earlier predecessor makes the predecessor show up.
  $ sl pull -q -r $A
  $ sl log -r "predecessors($C)" -T '{node} {desc}\n' --hidden
  91713f37cee74b0145e53c32cf3fef354944ce4d C
  f9407b1692b919f7a3e45186464e2256e67c1be5 A
  $ sl log -r "successors($A)" -T '{node} {desc}\n' --hidden
  91713f37cee74b0145e53c32cf3fef354944ce4d C
  f9407b1692b919f7a3e45186464e2256e67c1be5 A
