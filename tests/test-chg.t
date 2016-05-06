init repo

  $ hg init foo
  $ cd foo

ill-formed config

  $ hg status
  $ echo '=brokenconfig' >> $HGRCPATH
  $ hg status
  hg: parse error at * (glob)
  [255]

alias having an environment variable and set to use pager

  $ rm $HGRCPATH
  $ cat >> $HGRCPATH <<'EOF'
  > [ui]
  > formatted = yes
  > [extensions]
  > pager =
  > [pager]
  > pager = sed -e 's/^/P/'
  > attend = printa
  > [alias]
  > printa = log -T "$A\n" -r 0
  > EOF

  $ A=1 hg printa
  P1
  $ A=2 hg printa
  P2
