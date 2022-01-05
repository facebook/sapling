/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use crate::progress::Progress;
use crate::request::Request;
use crate::request::RequestContext;
use crate::request::RequestInfo;
use crate::stats::Stats;

/// Generate a struct for holding event listeners (callbacks).
///
/// For each event `event_name(*args)`, two methods are generated:
/// - `on_event_name(f: impl Fn(*args))`: register a listener (callback).
/// - `trigger_event_name(*args)`: trigger the event, call registered callbacks.
macro_rules! gen_event_listeners {
    (
        #[doc=$struct_doc:literal]
        $struct_name:ident {
            $(
                #[doc=$event_doc:literal]
                $event_name:ident ( $($arg_name:ident : $arg_ty:ty),* ),
            )*
        }
    ) => {
        paste::paste! {
            #[doc=$struct_doc]
            #[derive(Default, Clone)]
            pub struct $struct_name {
                $(
                    #[doc=$event_doc]
                    [< $event_name _listeners >] : Vec::<Arc<dyn Fn( $($arg_ty,)* ) + Send + Sync>>,
                )*
            }

            impl ::std::fmt::Debug for $struct_name {
                fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                    f.write_str(stringify!($struct_name))
                }
            }

            impl $struct_name {
                $(
                    /// Register a callback for the given event type.
                    #[doc=$event_doc]
                    pub fn [< on_ $event_name >](&mut self, f: impl Fn( $($arg_ty,)* ) + Send + Sync + 'static) {
                        self. [< $event_name _listeners >].push(Arc::new(f));
                    }

                    /// Returns true if there are callbacks registered on the event.
                    pub(crate) fn [< should_trigger_ $event_name >](&self) -> bool {
                        !self. [< $event_name _listeners >] .is_empty()
                    }

                    /// Call all registered callbacks for the given event type.
                    pub(crate) fn [< trigger_ $event_name >](&self, $($arg_name: $arg_ty,)*) {
                        for cb in &self. [< $event_name _listeners >] {
                            cb( $($arg_name,)* );
                        }
                    }
                )*
            }
        }
    }
}

gen_event_listeners! {
    /// Events for a `HttpClient`.
    HttpClientEventListeners {
        /// A request is created.
        new_request(req: &mut RequestContext),

        /// A request is completed successfully.
        succeeded_request(req: &RequestContext),

        /// A request is failed.
        failed_request(req: &RequestContext),

        /// One or more requests have completed with statistics.
        stats(stats: &Stats),
    }
}

gen_event_listeners! {
    /// Events for a single Request.
    RequestEventListeners {
        /// Downloaded `n` bytes. Does not include HTTP headers.
        download_bytes(req: &RequestContext, n: usize),

        /// Uploaded `n` bytes.
        upload_bytes(req: &RequestContext, n: usize),

        /// Received Content-Length as `n`.
        content_length(req: &RequestContext, n: usize),

        /// On first byte of network activity.
        first_activity(req: &RequestContext),

        /// On progress update. Note: this is called periodically even if there is no progress.
        progress(req: &RequestContext, progress: Progress),

        /// The request completed successfully.
        success(req: &RequestInfo),

        /// The request completed unsuccessfully.
        failure(req: &RequestInfo),
    }
}

gen_event_listeners! {
    /// Events for request creation (both independent requests and requests via `HttpClient`)
    RequestCreationEventListeners {
        /// A request is created.
        new_request(req: &mut Request),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::SeqCst;
    use std::sync::Arc;

    gen_event_listeners! {
        /// Testing event listeners.
        HelloEventListeners {
            /// A "hello" event.
            hello(msg: &str),
        }
    }

    #[test]
    fn test_hello_event_listener() {
        let mut listeners = HelloEventListeners::default();
        let len = Arc::new(AtomicUsize::new(0));
        listeners.on_hello({
            let len = len.clone();
            move |s| {
                len.fetch_add(s.len(), SeqCst);
            }
        });
        listeners.on_hello({
            let len = len.clone();
            move |s| {
                len.fetch_add(s.len() * 100, SeqCst);
            }
        });
        listeners.trigger_hello("abcde");
        assert_eq!(len.load(SeqCst), 505);
    }
}
