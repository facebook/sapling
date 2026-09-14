#require no-eden

  $ enable shelve

Inject a working-copy access failure only while revert is running. No protected
filesystem paths or live repositories are involved.

  $ cat > $TESTTMP/deniedrevert.py <<'EOF'
  > from sapling import cmdutil, context, error, extensions
  > active = False
  > def revert(orig, *args, **kwargs):
  >     global active
  >     if args[0].configbool("test-revert", "fail"):
  >         raise error.Abort("synthetic revert failure")
  >     active = True
  >     try:
  >         return orig(*args, **kwargs)
  >     finally:
  >         active = False
  > def contains(orig, ctx, path):
  >     if active and path == ctx.repo().ui.config("test-revert", "denied"):
  >         raise error.Abort("synthetic revert denial for %s" % path)
  >     return orig(ctx, path)
  > def extsetup(ui):
  >     extensions.wrapfunction(cmdutil, "revert", revert)
  >     extensions.wrapfunction(context.workingctx, "__contains__", contains)
  > EOF
  $ setconfig extensions.deniedrevert=$TESTTMP/deniedrevert.py

The shelf changes only owned. Its new parent changes an unrelated file.

  $ newclientrepo
  $ drawdag <<'EOS'
  > REMOTE_UNRELATED  # BASE_OWNED/owned = base\n
  > |                 # REMOTE_UNRELATED/unrelated = remote\n
  > BASE_OWNED
  > EOS
  $ sl goto -q $BASE_OWNED
  $ echo edited > owned
  $ sl status
  M owned
  $ sl shelve -q --name feedback
  $ sl goto -q $REMOTE_UNRELATED
  $ setconfig test-revert.denied=unrelated

Unrelated parent files must not prevent restoring the shelf.

  $ sl unshelve -q --keep --name feedback
  $ cat owned
  edited
  $ sl status
  M owned
  $ test -f .sl/shelved/feedback.patch

A failure inside revert must remain visible.

  $ newclientrepo
  $ drawdag <<'EOS'
  > BASE_DENIED  # BASE_DENIED/owned = base\n
  > EOS
  $ sl goto -q $BASE_DENIED
  $ echo edited > owned
  $ sl status
  M owned
  $ sl shelve -q --name feedback
  $ setconfig test-revert.fail=true

  $ sl unshelve -q --keep --name feedback
  abort: synthetic revert failure
  [255]
  $ cat owned
  base
  $ sl status
  $ test -f .sl/shelved/feedback.patch
