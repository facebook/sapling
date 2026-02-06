  $ setconfig drawdag.defaultfiles=false
  $ setconfig diff.git=true

  $ newserver server
  $ drawdag <<EOS
  > C  # C/foo = content\n
  > |  # C/top = top\n
  > |
  > B  # B/foo = changed\n
  > |  # B/middle = middle\n
  > |
  > A  # A/foo = content\n
  >    # A/base = base\n
  > EOS

  $ newclientrepo client server

Don't fetch content unnecessarily.
FIXME: "foo" is not actually modified.
  $ SL_LOG=file_fetches=trace hg st -q --rev $A --rev $C
  M foo
  A middle
  A top

Make sure we do prefetch file content for diff operation:
FIXME: "header" is seriallly fetched
  $ SL_LOG=file_fetches=trace hg diff -q --rev $A --rev $C
  TRACE file_fetches: attrs=["header"] keys=["middle"]
  TRACE file_fetches: attrs=["header"] keys=["top"]
  TRACE file_fetches: attrs=["content", "header"] keys=["foo", "foo", "middle", "top"]
  TRACE file_fetches: attrs=["content", "header"] keys=["foo"]
  TRACE file_fetches: attrs=["content", "header"] keys=["foo"]
  TRACE file_fetches: attrs=["content", "header"] keys=["middle"]
  diff --git a/middle b/middle
  new file mode 100644
  --- /dev/null
  +++ b/middle
  @@ -0,0 +1,1 @@
  +middle
  TRACE file_fetches: attrs=["content", "header"] keys=["top"]
  diff --git a/top b/top
  new file mode 100644
  --- /dev/null
  +++ b/top
  @@ -0,0 +1,1 @@
  +top
