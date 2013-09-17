This test shows how dulwich fails to convert a commit accepted by hg.

In the real world case, it was a hand edit by the user to change the
timezone field in an export. However, if it is good enough for hg, we
have to make it good enough for git.

Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ hg init hgrepo
  $ cd hgrepo
  $ touch beta
  $ hg add beta
  $ fn_hg_commit -m "test commit"
  $ cat >patch2 <<EOF
  > # HG changeset patch
  > # User J. User <juser@example.com>
  > # Date 1337962044 25201
  > # Node ID 1111111111111111111111111111111111111111
  > # Parent  0000000000000000000000000000000000000000
  > Patch with sub-minute time zone
  >
  > diff --git a/alpha b/alpha
  > new file mode 100644
  > --- /dev/null
  > +++ b/alpha
  > @@ -0,0 +1,1 @@
  > +alpha
  > EOF
  $ hg import patch2
  applying patch2
  $ hg gexport
