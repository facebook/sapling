
  $ "$TESTDIR/hghave" svn svn-bindings || exit 80

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert =
  > graphlog =
  > EOF

  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/tags.svndump"

Convert
  $ hg convert --datesort svn-repo A-hg
  initializing destination A-hg repository
  scanning source...
  sorting...
  converting...
  5 init projA
  4 adda
  3 changea
  2 changea2
  1 changea3
  0 changea
  updating tags

  $ cd A-hg
  $ hg glog --template '{rev} {desc|firstline} tags: {tags}\n'
  o  6 update tags tags: tip
  |
  o  5 changea tags: trunk.goodtag
  |
  o  4 changea3 tags:
  |
  o  3 changea2 tags: trunk.v1
  |
  o  2 changea tags:
  |
  o  1 adda tags:
  |
  o  0 init projA tags:
  

  $ hg tags -q
  tip
  trunk.goodtag
  trunk.v1

  $ cd ..

Convert without tags

  $ hg convert --datesort --config convert.svn.tags= svn-repo A-notags-hg
  initializing destination A-notags-hg repository
  scanning source...
  sorting...
  converting...
  5 init projA
  4 adda
  3 changea
  2 changea2
  1 changea3
  0 changea

  $ hg -R A-notags-hg tags -q
  tip

