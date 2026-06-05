#require no-eden no-windows

  $ setconfig clone.use-rust=true
  >>> import os, subprocess, time
  >>> env = os.environ.copy()
  >>> env["LOG"] = "cmdclone=trace,atexit=debug,warn"
  >>> env["FAILPOINTS"] = "run::clone=sleep(5000)"
  >>> hg = env["HGEXECUTABLEPATH"]
  >>> with open("output", "wb") as output:
  ...     proc = subprocess.Popen([hg, "clone", "-Uq", "test:e1", "failure"], stdout=output, stderr=output, env=env)
  ...     deadline = time.time() + 10
  ...     while time.time() < deadline:
  ...         output.flush()
  ...         with open("output", "rb") as f:
  ...             if b"performing rust clone" in f.read():
  ...                 break
  ...         time.sleep(0.1)
  ...     else:
  ...         proc.kill()
  ...         raise RuntimeError("timed out waiting for clone to start")
  ...     proc.terminate()
  ...     _ = proc.wait()
  >>> if os.path.exists("failure"):
  ...     print(open("output", errors="replace").read(), end="")
  ...     raise RuntimeError("clone directory was not cleaned up")
