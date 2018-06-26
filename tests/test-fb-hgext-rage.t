  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > blackbox=
  > rage=
  > smartlog=
  > sparse=
  > EOF

  $ hg init repo
  $ cd repo
#if osx
  $ echo "[rage]" >> .hg/hgrc
  $ echo "rpmbin = /""bin/rpm" >> .hg/hgrc
#endif
  $ hg rage --preview | grep -o 'hg blackbox'
  hg blackbox

Test with shared repo
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > share=
  > EOF
  $ cd ..
  $ hg share repo repo2
  updating working directory
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Create fake infinitepush backup state to be collected by rage

  $ echo '{ "fakestate": "something" }' > repo/.hg/infinitepushbackupstate
  $ cd repo2
  $ hg rage --preview | grep fakestate
      "fakestate": "something"

  $ cd ..

Create fake commit cloud  state to be collected by rage

  $ echo '{ "commit_cloud_workspace": "something" }' > repo/.hg/store/commitcloudstate.someamazingworkspace.json
  $ cd repo2
  $ hg rage --preview | grep commit_cloud_workspace
      "commit_cloud_workspace": "something"
