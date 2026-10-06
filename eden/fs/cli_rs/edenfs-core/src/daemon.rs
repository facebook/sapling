/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A narrow, transport-agnostic view of the EdenFS daemon.

use std::future::Future;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use edenfs_utils::bytes_from_path;
use fbthrift_socket::SocketTransport;
use thiserror::Error;
use thrift_types::edenfs::MountId;
use thrift_types::edenfs::MountInfo;
use thrift_types::edenfs::UnmountArgument;
use thrift_types::edenfs_clients::EdenService;
use thrift_types::edenfs_clients::errors::UnmountV2Error;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::ReadBuf;
use tokio::sync::oneshot;
use tokio_uds_compat::UnixStream;
use tracing::error;
use tracing::warn;

const DEFAULT_CONNECTION_TIMEOUT: Duration = Duration::from_mins(2);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_mins(5);

#[derive(Debug, Error)]
pub enum DaemonError {
    /// The daemon predates the requested thrift method
    /// (`ApplicationExceptionErrorCode::UnknownMethod`).
    #[error("EdenFS daemon does not implement {method}")]
    UnknownMethod { method: &'static str },

    #[error("failed to connect to the EdenFS daemon: {0:#}")]
    Connect(#[source] anyhow::Error),

    #[error("EdenFS {method} request failed: {source:#}")]
    Request {
        method: &'static str,
        #[source]
        source: anyhow::Error,
    },

    #[error("EdenFS {method} request timed out after {} seconds", timeout.as_secs_f64())]
    Timeout {
        method: &'static str,
        timeout: Duration,
    },
}

pub type DaemonResult<T> = std::result::Result<T, DaemonError>;

fn request_error(method: &'static str, error: impl Into<anyhow::Error>) -> DaemonError {
    DaemonError::Request {
        method,
        source: error.into(),
    }
}

fn encode_path(method: &'static str, path: &Path) -> DaemonResult<Vec<u8>> {
    bytes_from_path(path.to_path_buf()).map_err(|error| request_error(method, error))
}

async fn with_request_timeout<T>(
    method: &'static str,
    request: impl Future<Output = T>,
) -> DaemonResult<T> {
    tokio::time::timeout(DEFAULT_REQUEST_TIMEOUT, request)
        .await
        .map_err(|_| DaemonError::Timeout {
            method,
            timeout: DEFAULT_REQUEST_TIMEOUT,
        })
}

/// The thrift calls the daemon-independent EdenFS CLI workflows make,
/// decoupled from any particular transport.
///
/// This intentionally covers only what the workflows in this crate need
/// (checkout removal and redirection management today). As more `edenfsctl`
/// workflows are inlined into other binaries, grow this trait method by
/// method; there are only two production implementations (the
/// thriftclient-backed one in `edenfs-client` and [`SocketDaemon`]).
#[async_trait]
pub trait EdenFsDaemon: Send + Sync {
    /// Thrift `listMounts`. `conn_timeout` bounds establishing the
    /// connection; callers that tolerate a stopped daemon pass a short
    /// timeout and ignore the error.
    async fn list_mounts(&self, conn_timeout: Option<Duration>) -> DaemonResult<Vec<MountInfo>>;

    /// Thrift `unmountV2`.
    async fn unmount_v2(&self, mount_point: &Path, use_force: bool) -> DaemonResult<()>;

    /// Legacy thrift `unmount`, for daemons that predate `unmountV2`.
    async fn unmount_legacy(&self, mount_point: &Path) -> DaemonResult<()>;

    /// Thrift `addBindMount` (Linux bind-mount redirections).
    #[cfg(target_os = "linux")]
    async fn add_bind_mount(
        &self,
        mount_point: &Path,
        repo_path: &Path,
        target_path: &Path,
    ) -> DaemonResult<()>;

    /// Thrift `removeBindMount` (Linux bind-mount redirections).
    #[cfg(target_os = "linux")]
    async fn remove_bind_mount(&self, mount_point: &Path, repo_path: &Path) -> DaemonResult<()>;

    /// `unmountV2`, falling back to the legacy `unmount` endpoint when the
    /// daemon predates it. The fallback decision lives here so every
    /// implementation shares it.
    async fn unmount_with_fallback(&self, mount_point: &Path, use_force: bool) -> DaemonResult<()> {
        match self.unmount_v2(mount_point, use_force).await {
            Err(error @ DaemonError::UnknownMethod { .. }) => {
                warn!(%error, "falling back to legacy unmount for {}", mount_point.display());
                self.unmount_legacy(mount_point).await
            }
            result => result,
        }
    }
}

#[derive(Debug, Error)]
#[error("EdenFS request cancelled")]
struct RequestCancelled;

// SocketTransport detaches its worker. Wake both I/O directions when the owning
// request ends so the worker releases its socket even if the peer never replies.
struct RequestSocket {
    stream: UnixStream,
    cancellation: oneshot::Receiver<()>,
}

impl RequestSocket {
    fn poll_cancelled(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        if Pin::new(&mut self.cancellation).poll(cx).is_ready() {
            Err(io::Error::other(RequestCancelled))
        } else {
            Ok(())
        }
    }
}

impl AsyncRead for RequestSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.poll_cancelled(cx)?;
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for RequestSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.poll_cancelled(cx)?;
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_cancelled(cx)?;
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_cancelled(cx)?;
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

/// An [`EdenFsDaemon`] speaking thrift over a unix-domain socket with the
/// pure-Rust fbthrift transport: no fbinit, no C++ thrift stack. Connects
/// per call, matching how Sapling's own EdenFS client uses this transport.
pub struct SocketDaemon {
    socket: PathBuf,
}

impl SocketDaemon {
    /// `socket` is the daemon socket (`$config_dir/socket`), NOT the
    /// per-mount `.eden/socket` symlink, so unmounted checkouts can still be
    /// operated on.
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    async fn connect(
        &self,
        conn_timeout: Option<Duration>,
    ) -> DaemonResult<(Arc<dyn EdenService + Send + Sync>, oneshot::Sender<()>)> {
        let timeout = conn_timeout.unwrap_or(DEFAULT_CONNECTION_TIMEOUT);
        let connect = UnixStream::connect(&self.socket);
        let stream = tokio::time::timeout(timeout, connect)
            .await
            .map_err(|_| {
                DaemonError::Connect(anyhow!(
                    "timed out connecting to {} after {} seconds",
                    self.socket.display(),
                    timeout.as_secs_f64()
                ))
            })?
            .map_err(|error| {
                DaemonError::Connect(
                    anyhow::Error::new(error)
                        .context(format!("failed to connect to {}", self.socket.display())),
                )
            })?;
        let (cancellation, cancelled) = oneshot::channel();
        let transport = SocketTransport::new_with_error_handler(
            RequestSocket {
                stream,
                cancellation: cancelled,
            },
            |error| {
                if !error.chain().any(|cause| {
                    cause
                        .downcast_ref::<io::Error>()
                        .and_then(io::Error::get_ref)
                        .is_some_and(|error| error.is::<RequestCancelled>())
                }) {
                    error!(target: "transport_errors", thrift_transport_error=?error);
                }
            },
        );
        Ok((
            <dyn EdenService>::new(BinaryProtocol, transport),
            cancellation,
        ))
    }
}

#[async_trait]
impl EdenFsDaemon for SocketDaemon {
    async fn list_mounts(&self, conn_timeout: Option<Duration>) -> DaemonResult<Vec<MountInfo>> {
        let (client, _cancellation) = self.connect(conn_timeout).await?;
        with_request_timeout("listMounts", client.listMounts())
            .await?
            .map_err(|error| request_error("listMounts", error))
    }

    async fn unmount_v2(&self, mount_point: &Path, use_force: bool) -> DaemonResult<()> {
        let unmount_argument = UnmountArgument {
            mountId: MountId {
                mountPoint: encode_path("unmountV2", mount_point)?,
                ..Default::default()
            },
            useForce: use_force,
            ..Default::default()
        };
        let (client, _cancellation) = self.connect(None).await?;
        match with_request_timeout("unmountV2", client.unmountV2(&unmount_argument)).await? {
            Ok(()) => Ok(()),
            Err(UnmountV2Error::ApplicationException(ref e))
                if e.type_ == ApplicationExceptionErrorCode::UnknownMethod =>
            {
                Err(DaemonError::UnknownMethod {
                    method: "unmountV2",
                })
            }
            Err(error) => Err(request_error("unmountV2", error)),
        }
    }

    async fn unmount_legacy(&self, mount_point: &Path) -> DaemonResult<()> {
        let encoded_path = encode_path("unmount", mount_point)?;
        let (client, _cancellation) = self.connect(None).await?;
        with_request_timeout("unmount", client.unmount(&encoded_path))
            .await?
            .map_err(|error| request_error("unmount", error))
    }

    #[cfg(target_os = "linux")]
    async fn add_bind_mount(
        &self,
        mount_point: &Path,
        repo_path: &Path,
        target_path: &Path,
    ) -> DaemonResult<()> {
        let mount_point = encode_path("addBindMount", mount_point)?;
        let repo_path = encode_path("addBindMount", repo_path)?;
        let target_path = encode_path("addBindMount", target_path)?;
        let (client, _cancellation) = self.connect(None).await?;
        with_request_timeout(
            "addBindMount",
            client.addBindMount(&mount_point, &repo_path, &target_path),
        )
        .await?
        .map_err(|error| request_error("addBindMount", error))
    }

    #[cfg(target_os = "linux")]
    async fn remove_bind_mount(&self, mount_point: &Path, repo_path: &Path) -> DaemonResult<()> {
        let mount_point = encode_path("removeBindMount", mount_point)?;
        let repo_path = encode_path("removeBindMount", repo_path)?;
        let (client, _cancellation) = self.connect(None).await?;
        with_request_timeout(
            "removeBindMount",
            client.removeBindMount(&mount_point, &repo_path),
        )
        .await?
        .map_err(|error| request_error("removeBindMount", error))
    }
}

/// An [`EdenFsDaemon`] that always fails to connect, for tests and
/// config-only flows that must behave exactly as if the daemon is not
/// running.
pub struct DisconnectedDaemon;

impl DisconnectedDaemon {
    fn error(&self) -> DaemonError {
        DaemonError::Connect(anyhow!("EdenFS daemon connections are disabled"))
    }
}

#[async_trait]
impl EdenFsDaemon for DisconnectedDaemon {
    async fn list_mounts(&self, _conn_timeout: Option<Duration>) -> DaemonResult<Vec<MountInfo>> {
        Err(self.error())
    }

    async fn unmount_v2(&self, _mount_point: &Path, _use_force: bool) -> DaemonResult<()> {
        Err(self.error())
    }

    async fn unmount_legacy(&self, _mount_point: &Path) -> DaemonResult<()> {
        Err(self.error())
    }

    #[cfg(target_os = "linux")]
    async fn add_bind_mount(
        &self,
        _mount_point: &Path,
        _repo_path: &Path,
        _target_path: &Path,
    ) -> DaemonResult<()> {
        Err(self.error())
    }

    #[cfg(target_os = "linux")]
    async fn remove_bind_mount(&self, _mount_point: &Path, _repo_path: &Path) -> DaemonResult<()> {
        Err(self.error())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::*;

    enum UnmountV2Behavior {
        Ok,
        UnknownMethod,
        RequestError,
    }

    struct FakeDaemon {
        unmount_v2_behavior: UnmountV2Behavior,
        unmount_v2_calls: AtomicUsize,
        unmount_legacy_calls: AtomicUsize,
    }

    impl FakeDaemon {
        fn new(unmount_v2_behavior: UnmountV2Behavior) -> Self {
            Self {
                unmount_v2_behavior,
                unmount_v2_calls: AtomicUsize::new(0),
                unmount_legacy_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl EdenFsDaemon for FakeDaemon {
        async fn list_mounts(
            &self,
            _conn_timeout: Option<Duration>,
        ) -> DaemonResult<Vec<MountInfo>> {
            Ok(Vec::new())
        }

        async fn unmount_v2(&self, _mount_point: &Path, _use_force: bool) -> DaemonResult<()> {
            self.unmount_v2_calls.fetch_add(1, Ordering::SeqCst);
            match self.unmount_v2_behavior {
                UnmountV2Behavior::Ok => Ok(()),
                UnmountV2Behavior::UnknownMethod => Err(DaemonError::UnknownMethod {
                    method: "unmountV2",
                }),
                UnmountV2Behavior::RequestError => Err(request_error(
                    "unmountV2",
                    anyhow!("injected request error"),
                )),
            }
        }

        async fn unmount_legacy(&self, _mount_point: &Path) -> DaemonResult<()> {
            self.unmount_legacy_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        #[cfg(target_os = "linux")]
        async fn add_bind_mount(
            &self,
            _mount_point: &Path,
            _repo_path: &Path,
            _target_path: &Path,
        ) -> DaemonResult<()> {
            Ok(())
        }

        #[cfg(target_os = "linux")]
        async fn remove_bind_mount(
            &self,
            _mount_point: &Path,
            _repo_path: &Path,
        ) -> DaemonResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn unmount_succeeds_without_fallback() {
        let daemon = FakeDaemon::new(UnmountV2Behavior::Ok);

        daemon
            .unmount_with_fallback(Path::new("/checkout"), true)
            .await
            .expect("unmountV2 success should not need the fallback");

        assert_eq!(daemon.unmount_v2_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            daemon.unmount_legacy_calls.load(Ordering::SeqCst),
            0,
            "legacy unmount must not run when unmountV2 succeeds"
        );
    }

    #[tokio::test]
    async fn unknown_method_falls_back_to_legacy_unmount() {
        let daemon = FakeDaemon::new(UnmountV2Behavior::UnknownMethod);

        daemon
            .unmount_with_fallback(Path::new("/checkout"), true)
            .await
            .expect("fallback to the legacy endpoint should succeed");

        assert_eq!(daemon.unmount_v2_calls.load(Ordering::SeqCst), 1);
        assert_eq!(daemon.unmount_legacy_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn request_error_does_not_fall_back() {
        let daemon = FakeDaemon::new(UnmountV2Behavior::RequestError);

        let error = daemon
            .unmount_with_fallback(Path::new("/checkout"), true)
            .await
            .expect_err("request errors other than UnknownMethod must propagate");

        assert!(matches!(error, DaemonError::Request { .. }));
        assert_eq!(
            daemon.unmount_legacy_calls.load(Ordering::SeqCst),
            0,
            "legacy unmount must not run for non-UnknownMethod errors"
        );
    }
}
