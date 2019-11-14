  $ enable obsstore

File node could be reused during commit --amend

  $ newrepo
  $ echo 1 > a
  $ echo 2 > b
  $ hg commit -m 12 -A a b
  $ echo 3 >> a
  $ hg commit -m 3

  $ hg commit --debug --amend -m 'without content change'
  amending changeset 0bd823dca296
  copying changeset 0bd823dca296 to dd3d87f356df
  committing files:
  a
  reusing a filelog node (exact match)
  committing manifest
  committing changelog
  committed changeset 2:92bc7a9d76f010337ece134e095054c094d44760

#if execbit

File node is reused for mode-only change

  $ chmod +x b
  $ hg ci --debug --amend -m 'without content change'
  amending changeset 92bc7a9d76f0
  committing files:
  a
  reusing a filelog node (exact match)
  b
  committing manifest
  committing changelog
  committed changeset 3:ba954a28eb454eb63e7348349f8e87e7b1be3601
#endif
