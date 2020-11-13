#chg-compatible
#testcases treematcher non-treematcher

test sparse

  $ enable sparse
  $ hg init myrepo
  $ cd myrepo
#if treematcher
  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > treematcher = False
  > EOF
#endif

  $ echo a > show
  $ echo a > show2
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
