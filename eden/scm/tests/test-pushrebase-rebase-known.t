#chg-compatible
#debugruntest-compatible

Pushrebase still needs filepeer.

  $ setconfig experimental.allowfilepeer=True

  $ configure modern
  $ enable pushrebase

Prepare repos.

The client repo1 has commit C known to the server.

  $ newserver server
  $ clone server repo1
  $ cd repo1
  $ drawdag << 'EOS'
  > B C
  >  \|
  >   A
  > EOS
  $ hg push -r $B --to b1 --create -q
  $ hg push -r $B --to b2 --create -q
  $ hg push -r $C --to c1 --create -q
  $ hg pull -B b1 -B b2 -B c1 -q

  $ hg log -Gr: -T '{desc} {remotenames} {phase}'
  o  C remote/c1 public
  │
  │ o  B remote/b1 remote/b2 public
  ├─╯
  o  A  public
  
Trying to push C (public) to b1 (B) is a no-op:

  $ hg push -r $C --to b1
  pushing rev dc0947a82db8 to destination ssh://user@dummy/server bookmark b1
  searching for changes
  no changes found
  updating bookmark b1

The client repo2 has draft C known to the server.

  $ clone server repo2
  $ cd repo2
  $ drawdag << 'EOS'
  >  C
  >  |
  > desc(A)
  > EOS

  $ hg pull -B b2 -q
  $ hg log -Gr: -T '{desc} {remotenames} {phase}'
  @  C  draft
  │
  │ o  B remote/b2 public
  ├─╯
  o  A  public
  
Rebase C (draft) to b2 (B) when the server already knows C:

  $ hg push -r $C --to b2
  pushing rev dc0947a82db8 to destination ssh://user@dummy/server bookmark b2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark b2
  remote: pushing 1 changeset:
  remote:     dc0947a82db8  C
  remote: 2 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -Gr ::b2 -T '{desc}\n'
  @  C
  │
  o  B
  │
  o  A
  
