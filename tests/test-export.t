  $ hg init repo
  $ cd repo
  $ touch foo
  $ hg add foo
  $ for i in 0 1 2 3 4 5 6 7 8 9 10 11; do
  >    echo "foo-$i" >> foo
  >    hg ci -m "foo-$i"
  > done

  $ for out in "%nof%N" "%%%H" "%b-%R" "%h" "%r"; do
  >    echo
  >    echo "# foo-$out.patch"
  >    hg export -v -o "foo-$out.patch" 2:tip
  > done
  
  # foo-%nof%N.patch
  exporting patches:
  foo-01of10.patch
  foo-02of10.patch
  foo-03of10.patch
  foo-04of10.patch
  foo-05of10.patch
  foo-06of10.patch
  foo-07of10.patch
  foo-08of10.patch
  foo-09of10.patch
  foo-10of10.patch
  
  # foo-%%%H.patch
  exporting patches:
  foo-%617188a1c80f869a7b66c85134da88a6fb145f67.patch
  foo-%dd41a5ff707a5225204105611ba49cc5c229d55f.patch
  foo-%f95a5410f8664b6e1490a4af654e4b7d41a7b321.patch
  foo-%4346bcfde53b4d9042489078bcfa9c3e28201db2.patch
  foo-%afda8c3a009cc99449a05ad8aa4655648c4ecd34.patch
  foo-%35284ce2b6b99c9d2ac66268fe99e68e1974e1aa.patch
  foo-%9688c41894e6931305fa7165a37f6568050b4e9b.patch
  foo-%747d3c68f8ec44bb35816bfcd59aeb50b9654c2f.patch
  foo-%5f17a83f5fbd9414006a5e563eab4c8a00729efd.patch
  foo-%f3acbafac161ec68f1598af38f794f28847ca5d3.patch
  
  # foo-%b-%R.patch
  exporting patches:
  foo-repo-2.patch
  foo-repo-3.patch
  foo-repo-4.patch
  foo-repo-5.patch
  foo-repo-6.patch
  foo-repo-7.patch
  foo-repo-8.patch
  foo-repo-9.patch
  foo-repo-10.patch
  foo-repo-11.patch
  
  # foo-%h.patch
  exporting patches:
  foo-617188a1c80f.patch
  foo-dd41a5ff707a.patch
  foo-f95a5410f866.patch
  foo-4346bcfde53b.patch
  foo-afda8c3a009c.patch
  foo-35284ce2b6b9.patch
  foo-9688c41894e6.patch
  foo-747d3c68f8ec.patch
  foo-5f17a83f5fbd.patch
  foo-f3acbafac161.patch
  
  # foo-%r.patch
  exporting patches:
  foo-02.patch
  foo-03.patch
  foo-04.patch
  foo-05.patch
  foo-06.patch
  foo-07.patch
  foo-08.patch
  foo-09.patch
  foo-10.patch
  foo-11.patch

Exporting 4 changesets to a file:

  $ hg export -o export_internal 1 2 3 4
  $ grep HG export_internal | wc -l
  \s*4 (re)

Exporting 4 changesets to a file:

  $ hg export 1 2 3 4 | grep HG | wc -l
  \s*4 (re)

Exporting revision -2 to a file:

  $ hg export -- -2
  # HG changeset patch
  # User test
  # Date 0 0
  # Node ID 5f17a83f5fbd9414006a5e563eab4c8a00729efd
  # Parent  747d3c68f8ec44bb35816bfcd59aeb50b9654c2f
  foo-10
  
  diff -r 747d3c68f8ec -r 5f17a83f5fbd foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -8,3 +8,4 @@
   foo-7
   foo-8
   foo-9
  +foo-10

