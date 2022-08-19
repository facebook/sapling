#chg-compatible
#debugruntest-compatible

test sparse

  $ enable sparse rebase
  $ hg init repo
  $ cd repo
  $ mkdir subdir

  $ cat > foo.sparse <<EOF
  > [include]
  > *
  > EOF
  $ cat > subdir/bar.sparse <<EOF
  > [include]
  > *
  > EOF
  $ hg ci -Aqm 'initial'

Sanity check
  $ hg sparse enable foo.sparse
  $ hg sparse
  %include foo.sparse
  [include]
  
  [exclude]
  
  



  $ hg sparse disable foo.sparse

Relative works from root.
  $ hg sparse enable ./foo.sparse
  $ hg sparse
  %include foo.sparse
  [include]
  
  [exclude]
  
  



  $ hg sparse disable foo.sparse

  $ cd subdir

Canonical path works from subdir.
  $ hg sparse enable foo.sparse
  $ hg sparse
  %include foo.sparse
  [include]
  
  [exclude]
  
  



  $ hg sparse disable foo.sparse

Relative path also works
  $ hg sparse enable ../foo.sparse
  $ hg sparse
  %include foo.sparse
  [include]
  
  [exclude]
  
  



  $ hg sparse disable foo.sparse

  $ hg sparse enable bar.sparse
  $ hg sparse
  %include subdir/bar.sparse
  [include]
  
  [exclude]
  
  



  $ hg sparse disable bar.sparse
