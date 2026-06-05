#chg-compatible
#require no-eden

test sparse

  $ configure modernclient

  $ enable sparse rebase
  $ newclientrepo myrepo

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
  $ sl ci -Aqm 'initial'
  $ sl sparse enable main.sparse

# Verify inc/exc/incfile.txt is not included.
  $ ls -R | grep -v :
  base.sparse
  file.txt
  inc
  main.sparse
  
  file.txt


  $ sl debugsparseexplainmatch inc/exc/incfile.txt
  FALSE by rule !inc/exc/** ($TESTTMP/myrepo/.sl/sparse -> main.sparse -> base.sparse):

  $ sl debugsparseexplainmatch -s main.sparse inc/exc/incfile.txt
  FALSE by rule !inc/exc/** (<cli> -> main.sparse -> base.sparse):

# Upgrade main.sparse to v2
  $ cat > main.sparse <<EOF
  > [metadata]
  > version: 2
  > %include base.sparse
  > [include]
  > inc/exc/incfile.txt
  > EOF
  $ sl commit -qm "v2 sparse"

# Verify inc/exc/incfile.txt is now included.
  $ ls -R | grep -v :
  base.sparse
  file.txt
  inc
  main.sparse
  
  exc
  file.txt
  
  incfile.txt

  $ sl debugsparseexplainmatch inc/exc/incfile.txt
  TRUE by rule inc/exc/incfile.txt/** ($TESTTMP/myrepo/.sl/sparse -> main.sparse):


  $ sl debugsparseprofilev2 main.sparse
  V1 includes 4 files
  V2 includes 5 files
  + inc/exc/incfile.txt

Do not union profiles outside the root .sl/sparse config.
  $ cat > temp.sparse <<EOF
  > [metadata]
  > version: 2
  > 
  > %include main.sparse
  > %include base.sparse
  > EOF
  $ sl commit -Aqm "add temp.sparse"
  $ sl debugsparsematch -s temp.sparse inc/exc/incfile.txt
  considering 1 file(s)
  $ rm temp.sparse

Do union profiles in root .sl/sparse config.
  $ sl sparse enable base.sparse
  $ ls inc/exc/incfile.txt
  inc/exc/incfile.txt
  $ sl sparse disable base.sparse

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
  $ sl commit -Aqm 'other.sparse'
  $ sl sparse enable other.sparse
  $ find . -type f -not -wholename "**/.sl/**" | sort
  ./base.sparse
  ./file.txt
  ./inc/exc/excfile.txt
  ./inc/exc/incfile.txt
  ./inc/file.txt
  ./main.sparse
  ./other.sparse

# Test explain with multiple matches

  $ newclientrepo
  $ cat > s1.sparse << 'EOF'
  > [exclude]
  > glob:a*.sparse
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
  > [exclude]
  > glob:*b.sparse
  > [metadata]
  > version: 2
  > EOF

  $ sl ci -Am init s1.sparse s2.sparse s3.sparse s4.sparse
  $ sl sparse enable s1.sparse s4.sparse

  $ sl debugsparseexplainmatch ab.sparse
  OR(
    FALSE by rule !a*.sparse/** ($TESTTMP/repo1/.sl/sparse -> s1.sparse)
    FALSE by rule !*b.sparse/** ($TESTTMP/repo1/.sl/sparse -> s4.sparse)
  ):

  $ sl sparse enable s2.sparse s3.sparse
  $ sl debugsparseexplainmatch ab.sparse
  OR(
    FALSE by rule !a*.sparse/** ($TESTTMP/repo1/.sl/sparse -> s1.sparse)
    TRUE by rule a*.sparse/** ($TESTTMP/repo1/.sl/sparse -> s2.sparse)
    TRUE by rule *b.sparse/** ($TESTTMP/repo1/.sl/sparse -> s3.sparse)
    FALSE by rule !*b.sparse/** ($TESTTMP/repo1/.sl/sparse -> s4.sparse)
  ):


  $ newclientrepo
  $ sl debugsparseexplainmatch something
  abort: --sparse-profile is required
  [255]
