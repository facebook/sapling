/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::error;

py_exception!(error, CertificateError);
py_exception!(error, CommitLookupError, exc::KeyError);
py_exception!(error, FetchError, exc::KeyError);
py_exception!(error, HttpError);
py_exception!(error, IndexedLogError);
py_exception!(error, LockContendedError);
py_exception!(error, MetaLogError);
py_exception!(error, NeedSlowPathError);
py_exception!(error, NonUTF8Path);
py_exception!(error, RustError);
py_exception!(error, RevisionstoreError);
py_exception!(error, TlsError);

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "error"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add(py, "CertificateError", py.get_type::<CertificateError>())?;
    m.add(py, "CommitLookupError", py.get_type::<CommitLookupError>())?;
    m.add(py, "FetchError", py.get_type::<FetchError>())?;
    m.add(py, "HttpError", py.get_type::<HttpError>())?;
    m.add(py, "IndexedLogError", py.get_type::<IndexedLogError>())?;
    m.add(
        py,
        "LockContendedError",
        py.get_type::<LockContendedError>(),
    )?;
    m.add(py, "MetaLogError", py.get_type::<MetaLogError>())?;
    m.add(py, "NeedSlowPathError", py.get_type::<NeedSlowPathError>())?;
    m.add(py, "RustError", py.get_type::<RustError>())?;
    m.add(
        py,
        "RevisionstoreError",
        py.get_type::<RevisionstoreError>(),
    )?;
    m.add(py, "NonUTF8Path", py.get_type::<NonUTF8Path>())?;
    m.add(py, "TlsError", py.get_type::<TlsError>())?;

    register_error_handlers();

    Ok(m)
}

fn register_error_handlers() {
    fn specific_error_handler(py: Python, mut e: &error::Error) -> Option<PyErr> {
        // We care about concrete errors, so peel away anyhow contextual layers.
        while let Some(inner) = e.downcast_ref::<error::Error>() {
            e = inner;
        }

        // Extract inner io::Error out.
        // Why does Python need the low-level IOError? It doesn't have to.
        // Consider:
        // - Only expose high-level errors to Python with just enough
        //   information that Python can consume. Python no longer handles
        //   IOError directly.
        // - Gain more explicit control about error types exposed to
        //   Python. This means dropping anyhow.
        if let Some(e) = e.downcast_ref::<std::io::Error>() {
            return Some(cpython_ext::error::translate_io_error(py, e));
        }

        if let Some(revlogindex::Error::Corruption(e)) = e.downcast_ref::<revlogindex::Error>() {
            if let revlogindex::errors::CorruptionError::Io(e) = e.as_ref() {
                return Some(cpython_ext::error::translate_io_error(py, e));
            }
        }

        let mut dag_error = None;
        if let Some(e) = e.downcast_ref::<dag::Error>() {
            dag_error = Some(e);
        } else if let Some(hgcommits::Error::Dag(e)) = e.downcast_ref::<hgcommits::Error>() {
            dag_error = Some(e);
        }

        if let Some(e) = dag_error {
            match e {
                dag::Error::Backend(ref backend_error) => match backend_error.as_ref() {
                    dag::errors::BackendError::Io(e) => {
                        return Some(cpython_ext::error::translate_io_error(py, &e));
                    }
                    dag::errors::BackendError::Other(e) => return specific_error_handler(py, e),
                    _ => {}
                },
                dag::Error::VertexNotFound(_) | dag::Error::IdNotFound(_) => {
                    return Some(PyErr::new::<CommitLookupError, _>(
                        py,
                        cpython_ext::Str::from(e.to_string()),
                    ));
                }
                dag::Error::NeedSlowPath(_) => {
                    return Some(PyErr::new::<NeedSlowPathError, _>(
                        py,
                        cpython_ext::Str::from(e.to_string()),
                    ));
                }
                _ => {}
            }
        }

        if let Some(e) = e.downcast_ref::<repolock::LockError>() {
            match e {
                repolock::LockError::Contended(repolock::LockContendedError {
                    contents, ..
                }) => {
                    return Some(PyErr::new::<LockContendedError, _>(
                        py,
                        cpython_ext::Str::from(contents.clone()),
                    ));
                }
                repolock::LockError::Io(e) => {
                    return Some(cpython_ext::error::translate_io_error(py, &e.to_io_err()));
                }
                _ => {}
            };
        }

        if e.is::<indexedlog::Error>() {
            Some(PyErr::new::<IndexedLogError, _>(
                py,
                cpython_ext::Str::from(format!("{:?}", e)),
            ))
        } else if e.is::<metalog::Error>() {
            Some(PyErr::new::<MetaLogError, _>(
                py,
                cpython_ext::Str::from(format!("{:?}", e)),
            ))
        } else if e.is::<revisionstore::Error>() {
            Some(PyErr::new::<RevisionstoreError, _>(
                py,
                cpython_ext::Str::from(format!("{:?}", e)),
            ))
        } else if matches!(
            e.downcast_ref::<dag::Error>(),
            Some(dag::Error::NeedSlowPath(_))
        ) {
            Some(PyErr::new::<NeedSlowPathError, _>(
                py,
                cpython_ext::Str::from(e.to_string()),
            ))
        } else if matches!(
            e.downcast_ref::<dag::Error>(),
            Some(dag::Error::VertexNotFound(_)) | Some(dag::Error::IdNotFound(_))
        ) {
            Some(PyErr::new::<CommitLookupError, _>(
                py,
                cpython_ext::Str::from(e.to_string()),
            ))
        } else if e.is::<cpython_ext::Error>() {
            Some(PyErr::new::<NonUTF8Path, _>(
                py,
                cpython_ext::Str::from(format!("{:?}", e)),
            ))
        } else if let Some(e) = e.downcast_ref::<edenapi::EdenApiError>() {
            match e {
                edenapi::EdenApiError::Http(http_client::HttpClientError::Tls(
                    http_client::TlsError { source: e, .. },
                )) => Some(PyErr::new::<TlsError, _>(
                    py,
                    cpython_ext::Str::from(e.to_string()),
                )),
                _ => Some(PyErr::new::<HttpError, _>(
                    py,
                    cpython_ext::Str::from(e.to_string()),
                )),
            }
        } else if e.is::<auth::MissingCerts>() {
            Some(PyErr::new::<CertificateError, _>(
                py,
                cpython_ext::Str::from(format!("{}", e)),
            ))
        } else if e.is::<auth::X509Error>() {
            Some(PyErr::new::<CertificateError, _>(
                py,
                cpython_ext::Str::from(format!("{}", e)),
            ))
        } else if let Some(e) = e.downcast_ref::<revisionstore::scmstore::KeyFetchError>() {
            use revisionstore::scmstore::KeyFetchError::*;
            if let Other(ref e) = e {
                specific_error_handler(py, e)
            } else {
                Some(PyErr::new::<FetchError, _>(
                    py,
                    cpython_ext::Str::from(format!("{}", e)),
                ))
            }
        } else if let Some(e) = e.downcast_ref::<types::errors::NetworkError>() {
            // If we don't handle inner error specifically, default to
            // HttpError which will trigger the network doctor.
            specific_error_handler(py, &e.0).or_else(|| {
                Some(PyErr::new::<HttpError, _>(
                    py,
                    cpython_ext::Str::from(e.0.to_string()),
                ))
            })
        } else if let Some(e) = e.downcast_ref::<cpython_ext::PyErr>() {
            Some(e.clone(py).into())
        } else {
            None
        }
    }

    fn fallback_error_handler(py: Python, e: &error::Error) -> Option<PyErr> {
        Some(PyErr::new::<RustError, _>(py, format!("{:?}", e)))
    }

    error::register("010-specific", specific_error_handler);
    error::register("999-fallback", fallback_error_handler);
}
