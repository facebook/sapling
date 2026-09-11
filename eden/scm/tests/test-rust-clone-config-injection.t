#require no-eden no-windows

  $ eagerepo
  $ setconfig clone.use-rust=True
  $ setconfig commands.force-rust=clone
  $ setconfig remotefilelog.reponame=test-repo

Rust clone must serialize persisted config values without allowing injected sections.

  $ newrepo server
  $ drawdag << 'EOS'
  > A # bookmark master = A
  > EOS

  $ cd "$TESTTMP"
  $ cat > clone_config_payload.py << EOF
  > from pathlib import Path
  > Path("$TESTTMP/clone-config-rce-proof").write_text("executed\\n")
  > EOF
  $ PAYLOAD="$(base64 -w0 "$TESTTMP/clone_config_payload.py")"
  $ MALICIOUS_BOOKMARK="$(printf 'master\n[extensions]\nzz = python-base64:%s' "$PAYLOAD")"
  $ sl clone -Uq test:server client-config --config "remotenames.selectivepulldefault=$MALICIOUS_BOOKMARK"
  $ test ! -e "$TESTTMP/clone-config-rce-proof"
  $ sl -R client-config config extensions.zz
  [1]
