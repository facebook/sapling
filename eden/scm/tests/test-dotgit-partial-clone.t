#require git no-eden no-windows

  $ . $TESTDIR/git.sh
  $ git init -q -b main server-repo
  $ cd server-repo
  $ git config uploadpack.allowFilter true
  $ echo first > a
  $ git add a
  $ git commit -q -m first
  $ echo second > a
  $ git commit -qam second
  $ cd
  $ git clone -q --filter=blob:none --no-checkout file://$TESTTMP/server-repo client-repo
  $ cd client-repo
  $ git rev-list --objects --all --missing=print | grep '^[?]' | wc -l
  2

Keep the old extension if its remote has no promisor setting, even if another remote does.

  $ git config extensions.partialclone origin
  $ git config --unset remote.origin.promisor
  $ git config remote.other.promisor true
  $ sl log -r . -T '{desc}\n' > missing-promisor-error 2>&1
  [255]
  $ grep 'unsupported extension name extensions.partialclone' missing-promisor-error
  *unsupported extension name extensions.partialclone* (glob)
  $ git config --get extensions.partialclone
  origin
  $ git config --unset remote.other.promisor

The corresponding promisor setting only needs to exist, regardless of its value.

  $ git config remote.origin.promisor false
  $ sl log -r . -T '{desc}\n'
  second
  $ git config --get extensions.partialclone
  [1]
  $ git config --get remote.origin.promisor
  false

Valueless promisor settings count as present too.

  $ git config --unset remote.origin.promisor
  $ cat >> .git/config << 'EOF'
  > [remote "origin"]
  > promisor
  > EOF
  $ git config extensions.partialclone origin
  $ sl log -r . -T '{desc}\n'
  second
  $ git config --get extensions.partialclone
  [1]
  $ git config --bool --get remote.origin.promisor
  true
  $ git config remote.origin.promisor true

git-repo adds the obsolete extension even though Git already configured the promisor remote.

  $ git config extensions.partialclone origin
  $ sl cat -r . a
  second
  $ git config --get extensions.partialclone
  [1]
  $ git config --get remote.origin.promisor
  true
  $ git config --get remote.origin.partialclonefilter
  blob:none
  $ git rev-list --objects --all --missing=print | grep '^[?]' | wc -l
  1

A later repo sync can restore the extension. Remove it again and fetch another missing blob.

  $ git config extensions.partialclone origin
  $ sl cat -r 'desc(first)' a
  first
  $ git config --get extensions.partialclone
  [1]
  $ git rev-list --objects --all --missing=print | grep '^[?]'
  [1]

A new Git commit and a restored extension must not prevent DAG synchronization.

  $ git reset --hard -q HEAD
  $ echo third > a
  $ git commit -qam third
  $ git config extensions.partialclone origin
  $ sl log -r 'ancestors(.)' -T '{desc}\n'
  first
  second
  third
  $ git config --get extensions.partialclone
  [1]

Other unsupported extensions still fail without being removed.

  $ git config extensions.unsupported true
  $ sl log -r . -T '{desc}\n' > unsupported-error 2>&1
  [255]
  $ grep 'unsupported extension name extensions.unsupported' unsupported-error
  *unsupported extension name extensions.unsupported* (glob)
  $ git -C .. config --file client-repo/.git/config --get extensions.unsupported
  true
  $ git -C .. config --file client-repo/.git/config --unset extensions.unsupported

A failed Git config command leaves the old extension in place.

  $ git config extensions.partialclone origin
  $ sl --config ui.git=$TESTTMP/missing-git log -r . -T '{desc}\n' > unset-error 2>&1
  [255]
  $ git config --get extensions.partialclone
  origin
