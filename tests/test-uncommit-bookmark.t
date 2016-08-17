Test uncommit with merges - set up the config

  $ cat >> $HGRCPATH <<EOF
  > [experimental]
  > evolution=createmarkers
  > [extensions]
  > uncommit = $TESTDIR/../hgext3rd/uncommit.py
  > EOF

Build up a repo

  $ hg init repo
  $ cd repo
  $ hg bookmark foo

Create some history

  $ touch a b
  $ hg add a b
  $ for i in 1 2 3 4 5; do echo $i > a; echo $i > b; hg commit -m "ab $i"; done
  $ ls
  a
  b
  $ hg log -G -T '{rev}:{node} {desc}' --hidden
  @  4:20cb36e71b9db86e13e40aabf3e7acb2f9c0fd71 ab 5
  |
  o  3:182d0df6a3f5a47c25e47fc72869511ca5985d47 ab 4
  |
  o  2:824a0a07ed00f7b8e09fb37e3855ca6c4f908935 ab 3
  |
  o  1:9b7f62cdb1a9367cd958c9971f28f062c95354e6 ab 2
  |
  o  0:eddfce390a2ec769af6240f82381e47d39065489 ab 1
  

Uncommit tip moves bookmark

  $ hg bookmark
   * foo                       4:20cb36e71b9d
  $ hg uncommit
  $ hg status
  M a
  M b
  $ hg bookmark
   * foo                       3:182d0df6a3f5
  $ hg log -G -T '{rev}:{node} {desc}' --hidden
  x  4:20cb36e71b9db86e13e40aabf3e7acb2f9c0fd71 ab 5
  |
  @  3:182d0df6a3f5a47c25e47fc72869511ca5985d47 ab 4
  |
  o  2:824a0a07ed00f7b8e09fb37e3855ca6c4f908935 ab 3
  |
  o  1:9b7f62cdb1a9367cd958c9971f28f062c95354e6 ab 2
  |
  o  0:eddfce390a2ec769af6240f82381e47d39065489 ab 1
  
  $ hg revert --all
  reverting a
  reverting b

Partial uncommit moves bookmark

  $ hg uncommit a
  $ hg status
  M a
  ? a.orig
  ? b.orig
  $ hg bookmark
   * foo                       5:2c0f4ed46f87
  $ hg log -G -T '{rev}:{node} {desc}' --hidden
  @  5:2c0f4ed46f87ef75f357cebde58f12677cb92c07 ab 4
  |
  | x  4:20cb36e71b9db86e13e40aabf3e7acb2f9c0fd71 ab 5
  | |
  | x  3:182d0df6a3f5a47c25e47fc72869511ca5985d47 ab 4
  |/
  o  2:824a0a07ed00f7b8e09fb37e3855ca6c4f908935 ab 3
  |
  o  1:9b7f62cdb1a9367cd958c9971f28f062c95354e6 ab 2
  |
  o  0:eddfce390a2ec769af6240f82381e47d39065489 ab 1
  
  $ hg revert --all
  reverting a

Uncommit in the middle of stack does not move bookmark

  $ hg checkout 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark foo)
  $ hg uncommit
  $ hg status
  M a
  M b
  ? a.orig
  ? b.orig
  $ hg bookmark
     foo                       5:2c0f4ed46f87
  $ hg revert --all
  reverting a
  reverting b

Partial uncommit mid stack does not move bookmark

  $ hg checkout 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg uncommit a
  $ hg status
  M a
  ? a.orig
  ? b.orig
  $ hg bookmark
     foo                       5:2c0f4ed46f87
  $ hg log -G -T '{rev}:{node} {desc}' --hidden
  @  6:be9557d9e693aa9f7942931985b6e1a990211598 ab 3
  |
  | o  5:2c0f4ed46f87ef75f357cebde58f12677cb92c07 ab 4
  | |
  | | x  4:20cb36e71b9db86e13e40aabf3e7acb2f9c0fd71 ab 5
  | | |
  | | x  3:182d0df6a3f5a47c25e47fc72869511ca5985d47 ab 4
  | |/
  | x  2:824a0a07ed00f7b8e09fb37e3855ca6c4f908935 ab 3
  |/
  o  1:9b7f62cdb1a9367cd958c9971f28f062c95354e6 ab 2
  |
  o  0:eddfce390a2ec769af6240f82381e47d39065489 ab 1
  
