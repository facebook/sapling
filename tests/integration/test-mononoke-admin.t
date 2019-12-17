  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

  $ mononoke_admin bookmarks log master_bookmark 2>&1 | grep master_bookmark
  (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

  $ mononoke_admin bookmarks set another_bookmark 26805aba1e600a82e93661149f2313866a221a7b 2>/dev/null

  $ mononoke_admin bookmarks list --kind publishing 2> /dev/null | sort
  another_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b
  master_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b

  $ mononoke_admin bookmarks delete master_bookmark 2> /dev/null

  $ mononoke_admin bookmarks list --kind publishing 2> /dev/null
  another_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b
