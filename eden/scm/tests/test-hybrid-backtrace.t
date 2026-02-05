#require no-eden

Test `bindings.backtrace.backtrace()` includes both resolved Python and Rust frames on supported platforms.

Can only test if supported:

    import bindings
    SUPPORTED_INFO = bindings.backtrace.SUPPORTED_INFO
    if not SUPPORTED_INFO.os_arch or not SUPPORTED_INFO.c_evalframe:
        $ exit 80

Test getting backtrace:

  $ cat > print_backtrace.py << 'EOF'
  > import bindings, re
  > def my_unique_function_name_for_test():
  >     for frame in bindings.backtrace.backtrace():
  >         # remove Rust function hashes
  >         # e.g. commands[ec2db546b32b7e26]::run::run_command (-C symbol-mangling-version=v0)
  >         frame = re.sub(r'\[[0-9a-f]+\]', '', frame) # remove Rust function hashes (e.g.
  >         # e.g. commands::run::dispatch_command::hac40196f059e3564 (-C symbol-mangling-version=legacy)
  >         frame = re.sub(r'::h[0-9a-f]{16}$', '', frame)
  >         print(frame)
  > my_unique_function_name_for_test()
  > 'EOF'

The backtrace should include the `my_unique_function_name_for_test` Python function and the `run_command` Rust function:

  $ sl debugshell print_backtrace.py
  ...
  my_unique_function_name_for_test at print_backtrace.py:3
  ...
  commands::run::run_command
  ...
