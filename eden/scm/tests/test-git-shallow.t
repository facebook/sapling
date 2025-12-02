#require git no-windows

Test client git repo with lazy blob/tree objects.
"shallow" in terms of a sapling / remotefilelog terminology.

  $ . $TESTDIR/git.sh
  $ git -c init.defaultBranch=main init -q server-repo
  $ cd server-repo
  $ git config uploadpack.allowFilter true
  $ SL_IDENTITY=sl drawdag << 'EOS'
  > B  # bookmark main = B
  > |  # B/dir/1=2\n
  > A  # A/dir/1=1\n
  > EOS

Blobs are lazy, trees are not.
(suboptimal: there are duplicated fetches)

  $ cd
  $ sl clone -q --config git.shallow=1 --git file://$TESTTMP/server-repo client-repo1
  $ cd client-repo1
  $ LOG=gitstore::fetch=trace sl prev -q
  TRACE gitstore::fetch::detail: fetch object hex="d00491fd7e5bb6fa28c517a0bb32b8b506539d4d"
  DEBUG gitstore::fetch: fetch objects count=1
  TRACE gitstore::fetch::detail: fetch object hex="d00491fd7e5bb6fa28c517a0bb32b8b506539d4d"
  DEBUG gitstore::fetch: fetch objects count=1
  [47f14a] A

Both blobs and trees are lazy:

  $ cd
  $ sl clone -q --config git.shallow=1 --config git.filter=tree:0 --git file://$TESTTMP/server-repo client-repo2
  $ cd client-repo2
  $ LOG=gitstore::fetch=trace sl prev -q
  TRACE gitstore::fetch::detail: fetch object hex="b4600ac31e67dcf7d490a149b0e27981a2ee7088"
  DEBUG gitstore::fetch: fetch objects count=1
  TRACE gitstore::fetch::detail: fetch object hex="d00491fd7e5bb6fa28c517a0bb32b8b506539d4d"
  DEBUG gitstore::fetch: fetch objects count=1
  TRACE gitstore::fetch::detail: fetch object hex="d00491fd7e5bb6fa28c517a0bb32b8b506539d4d"
  DEBUG gitstore::fetch: fetch objects count=1
  [47f14a] A

(note the trees are fetched in addition to the blobs)
  $ sl log -r . -T '{manifest}\n'
  b4600ac31e67dcf7d490a149b0e27981a2ee7088
