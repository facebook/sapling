#require chg

init repo

  $ chg init foo
  $ cd foo

ill-formed config

  $ chg status
  $ echo '=brokenconfig' >> $HGRCPATH
  $ chg status
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

  $ A=1 chg printa
  P1
  $ A=2 chg printa
  P2
