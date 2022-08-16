#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.nativecheckout=true
  $ newserver server

test sparse

  $ newremoterepo myrepo
  $ enable sparse

  $ echo a > show
  $ echo a > show2
  $ echo c > show3
  $ echo x > hide
  $ cat >> .sparse-include <<EOF
  > [include]
  > show
  > .sparse-include
  > EOF
  $ hg add .sparse-include
  $ hg ci -Aqm 'initial'
  $ hg sparse enable .sparse-include
  $ ls
  show
  $ cat >> .sparse-include <<EOF
  > [include]
  > show
  > show2
  > EOF
  $ hg ci -Am 'second'
  $ hg up -q 'desc(initial)'
  $ ls
  show
  $ hg up -q 'desc(second)'
  $ ls
  show
  show2
#if no-windows
  $ cat >> .sparse-include <<EOF
  > [include]
  > show
  > show2
  > show3
  > EOF
  $ hg ci -m "third"
  $ chmod +x show3
  $ hg ci -m "fourth"
  $ hg up -q 'desc(second)'
  $ ls
  show
  show2
  $ hg up -q 'desc(fourth)'
  $ ls
  show
  show2
  show3
#endif

