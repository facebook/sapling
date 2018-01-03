  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fbhistedit=$TESTDIR/../hgext3rd/fbhistedit.py
  > histedit=
  > rebase=
  > [alias]
  > tglog = log -G --template "{rev}:{node}:{phase} '{desc}'\n"
  > EOF

  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg commit -m "base"
  $ for x in a b c ; do
  >   echo $x > $x
  >   hg add $x
  >   hg commit -m $x
  > done
  $ hg up -q 0
  $ for x in d e f ; do
  >   echo $x > $x
  >   hg add $x
  >   hg commit -m $x
  > done
  created new head
  $ hg tglog
  @  6:1eb7eda15cd7b2222738a7c9b47d1f51349b2bdb:draft 'f'
  |
  o  5:581a2eefdc84e2ec03c03b152f4982eefd77d7d8:draft 'e'
  |
  o  4:331acda6ee0072ace4b46c46bf80bb585d55d799:draft 'd'
  |
  | o  3:f9d2e574dc5853aac398917ed798d8640e8203af:draft 'c'
  | |
  | o  2:c87fe1ae405ff0a6dcc1ce27064cb9d303a05734:draft 'b'
  | |
  | o  1:c604726e05fb3a349978173b3ab4a3ee6e43cd6c:draft 'a'
  |/
  o  0:d20a80d4def38df63a4b330b7fb688f3d4cae1e3:draft 'base'
  
Use histedit to graft an extra commit into current history

  $ hg up -q 3
  $ hg histedit 1 --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft 581a2eefdc84
  > pick f9d2e574dc58
  > EOF
  [1]

  $ hg tglog
  @  7:fc9a25c1b8afb917f8e3dacad873f0d0bea14a96:draft 'c'
  |
  o  6:efc3ff9af0d1c5d50876bd81f6c4782dccc1c5b2:draft 'e'
  |
  | o  5:1eb7eda15cd7b2222738a7c9b47d1f51349b2bdb:draft 'f'
  | |
  | o  4:581a2eefdc84e2ec03c03b152f4982eefd77d7d8:draft 'e'
  | |
  | o  3:331acda6ee0072ace4b46c46bf80bb585d55d799:draft 'd'
  | |
  o |  2:c87fe1ae405ff0a6dcc1ce27064cb9d303a05734:draft 'b'
  | |
  o |  1:c604726e05fb3a349978173b3ab4a3ee6e43cd6c:draft 'a'
  |/
  o  0:d20a80d4def38df63a4b330b7fb688f3d4cae1e3:draft 'base'
  
Try to use histedit to graft a non-existent commit

  $ hg histedit 1 --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft abcdefabcdef
  > pick fc3ff9af0d1c
  > pick fc9a25c1b8af
  > EOF
  hg: parse error: unknown changeset abcdefabcdef listed

Try to use histedit to graft a commit from the set of commits being edited

  $ hg histedit 1 --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft fc9a25c1b8af
  > pick fc3ff9af0d1c
  > pick fc9a25c1b8af
  > EOF
  hg: parse error: graft "fc9a25c1b8af" changeset was an edited list candidate
  (graft must only use unlisted changesets)

