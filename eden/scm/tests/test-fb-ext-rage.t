#debugruntest-compatible
#chg-compatible

  $ enable blackbox rage smartlog sparse share

  $ hg init repo
  $ cd repo
#if osx
  $ echo "[rage]" >> .hg/hgrc
  $ echo "rpmbin = /""bin/rpm" >> .hg/hgrc
#endif
  $ hg rage --preview > out.txt
  $ cat out.txt | grep -o '^hg blackbox'
  hg blackbox
  $ cat out.txt | grep -o '^hg cloud status'
  hg cloud status
  $ cat out.txt | grep -o '^hg sparse:'
  hg sparse:
  $ rm out.txt

Test with shared repo
  $ cd ..
  $ hg share repo repo2
  updating working directory
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Create fake infinitepush backup state to be collected by rage

  $ mkdir repo/.hg/infinitepushbackups
  $ echo '{ "fakestate": "something" }' > repo/.hg/infinitepushbackups/infinitepushbackupstate_somestate
  $ cd repo2
  $ hg rage --preview | grep [f]akestate
      "fakestate": "something"

  $ cd ..

Create fake commit cloud  state to be collected by rage

  $ echo '{ "commit_cloud_workspace": "something" }' > repo/.hg/store/commitcloudstate.someamazingworkspace.json
  $ cd repo2
  $ hg rage --preview | grep [c]ommit_cloud_workspace
      "commit_cloud_workspace": "something"
