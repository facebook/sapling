#chg-compatible

test sparse

  $ enable sparse rebase
  $ hg init myrepo
  $ cd myrepo

  $ echo z > file.txt
  $ mkdir exc
  $ mkdir -p inc/exc
  $ echo z > exc/file.txt
  $ echo z > inc/file.txt
  $ echo z > inc/exc/incfile.txt
  $ echo z > inc/exc/excfile.txt
  $ cat > base.sparse <<EOF
  > [include]
  > glob:*.sparse
  > path:file.txt
  > path:inc/
  > 
  > [exclude]
  > path:inc/exc
  > 
  > [metadata]
  > version: 2
  > EOF
  $ cat > main.sparse <<EOF
  > %include base.sparse
  > [include]
  > path:inc/exc/incfile.txt
  > EOF
  $ hg ci -Aqm 'initial'
  $ hg sparse enable main.sparse

# Verify inc/exc/incfile.txt is not included.
  $ ls -R | grep -v :
  base.sparse
  file.txt
  inc
  main.sparse
  
  file.txt


  $ hg debugsparseexplainmatch inc/exc/incfile.txt
  inc/exc/incfile.txt: excluded by rule !inc/exc/** ($TESTTMP/myrepo/.hg/sparse -> main.sparse -> base.sparse)

  $ hg debugsparseexplainmatch -s main.sparse inc/exc/incfile.txt
  inc/exc/incfile.txt: excluded by rule !inc/exc/** (<cli> -> main.sparse -> base.sparse)

# Upgrade main.sparse to v2
  $ cat > main.sparse <<EOF
  > [metadata]
  > version: 2
  > %include base.sparse
  > [include]
  > inc/exc/incfile.txt
  > EOF
  $ hg commit -qm "v2 sparse"

# Verify inc/exc/incfile.txt is now included.
  $ ls -R | grep -v :
  base.sparse
  file.txt
  inc
  main.sparse
  
  exc
  file.txt
  
  incfile.txt

  $ hg debugsparseexplainmatch inc/exc/incfile.txt
  inc/exc/incfile.txt: included by rule inc/exc/incfile.txt/** ($TESTTMP/myrepo/.hg/sparse -> main.sparse)


  $ hg debugsparseprofilev2 main.sparse
  V1 includes 4 files
  V2 includes 5 files
  + inc/exc/incfile.txt

Do not union profiles outside the root .hg/sparse config.
  $ cat > temp.sparse <<EOF
  > [metadata]
  > version: 2
  > 
  > %include main.sparse
  > %include base.sparse
  > EOF
  $ hg commit -Aqm "add temp.sparse"
  $ hg debugsparsematch -s temp.sparse inc/exc/incfile.txt
  considering 1 file(s)
  $ rm temp.sparse

Do union profiles in root .hg/sparse config.
  $ hg sparse enable base.sparse
  $ ls inc/exc/incfile.txt
  inc/exc/incfile.txt
  $ hg sparse disable base.sparse

Test that multiple profiles do not clobber each others includes
# Exclude inc/exc/incfile.txt which main.sparse includes and
# include inc/exc/excfile.txt which main.sparse excludes. Verify they are now
# both present.
  $ cat >> other.sparse <<EOF
  > [include]
  > inc/exc/excfile.txt
  > [exclude]
  > inc/exc/incfile.txt
  > EOF
  $ hg commit -Aqm 'other.sparse'
  $ hg sparse enable other.sparse
  $ find . -type f -not -wholename "**/.hg/**" | sort
  ./base.sparse
  ./file.txt
  ./inc/exc/excfile.txt
  ./inc/exc/incfile.txt
  ./inc/file.txt
  ./main.sparse
  ./other.sparse

# Test explain with multiple matches

  $ newrepo
  $ cat > s1.sparse << 'EOF'
  > !glob:a*.sparse
  > [metadata]
  > version: 2
  > EOF
  $ cat > s2.sparse << 'EOF'
  > glob:a*.sparse
  > [metadata]
  > version: 2
  > EOF
  $ cat > s3.sparse << 'EOF'
  > glob:*b.sparse
  > [metadata]
  > version: 2
  > EOF
  $ cat > s4.sparse << 'EOF'
  > !glob:*b.sparse
  > [metadata]
  > version: 2
  > EOF

  $ hg ci -Am init s1.sparse s2.sparse s3.sparse s4.sparse
  $ hg sparse enable s1.sparse s4.sparse

  $ hg debugsparseexplainmatch ab.sparse
  ab.sparse:
    !a*.sparse/** ($TESTTMP/repo1/.hg/sparse -> s1.sparse)
    !*b.sparse/** ($TESTTMP/repo1/.hg/sparse -> s4.sparse)

  $ hg sparse enable s2.sparse s3.sparse
  $ hg debugsparseexplainmatch ab.sparse
  ab.sparse:
    a*.sparse/** ($TESTTMP/repo1/.hg/sparse -> s2.sparse)
    *b.sparse/** ($TESTTMP/repo1/.hg/sparse -> s3.sparse)
    !a*.sparse/** ($TESTTMP/repo1/.hg/sparse -> s1.sparse) (overridden by rules above)
    !*b.sparse/** ($TESTTMP/repo1/.hg/sparse -> s4.sparse) (overridden by rules above)


  $ newrepo
  $ hg debugsparseexplainmatch something
  abort: --sparse-profile is required
  [255]
