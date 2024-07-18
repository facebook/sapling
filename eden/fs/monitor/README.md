This directory contains a wrapper process that monitors the EdenFS daemon. This
wrapper process serves a few purposes:

# Simplifies management of EdenFS across graceful restarts

This monitoring process provides a single parent process that can be monitored
by systemd and other system management daemons, even across EdenFS graceful
restarts. When a graceful restart is desired this wrapper daemon can spawn the
new EdenFS instance, so that the new EdenFS instance is still part of the
original service process hierarchy.

Note that using a wrapper for this purpose is not strictly required with systemd
(it is possible to inform systemd that the main process ID has changed and it
should monitor a new process moving forward). However, this wrapper provides us
a bit more flexibility and control around the restart mechanism, and also makes
it easier to monitor EdenFS with other service management frameworks on other
platforms.

# Log file management and rotation

This process reads all messages printed by EdenFS to stdout and stderr, and
writes them to a log file, performing log rotation when appropriate.

Implementing log rotation properly is tricky otherwise, as there are many
different sources that can end up writing data to EdenFS's stdout/stderr
descriptors, including separate processes like the privhelper process and
spawned Python subprocesses.

# Intelligent Restarting of EdenFS when it is Idle

This wrapper process supports requests to trigger a restart at some point in the
future when EdenFS appears to be idle.

While graceful restart should minimize user-visible disruption, it can still
introduce a delay for I/O operations while the restart is in progress. Therefore
it is still desirable to try and perform the restart while users are not
actively accessing the file system, if possible.

This functionality is provided by the wrapper primarily because the wrapper
provides a convenient location to centralize this management in case multiple
restart attempts are requested before EdenFS becomes idle.
