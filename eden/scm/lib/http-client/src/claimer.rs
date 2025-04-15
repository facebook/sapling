/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// A glorified atomic counter to limit the number of in-flight requests.
#[derive(Default, Clone, Debug)]
pub(crate) struct RequestClaimer {
    // Separate escape hatch to disable limiting.
    enable_limiting: bool,
    in_flight_limit: Option<usize>,
    in_flight_count: Arc<AtomicUsize>,
}

impl RequestClaimer {
    pub(crate) fn new(enable_limiting: bool, limit: Option<usize>) -> Self {
        Self {
            enable_limiting,
            in_flight_limit: limit,
            in_flight_count: Default::default(),
        }
    }

    /// Claim up to `want` request spots, returning claims which free the spot on drop.
    /// Can return zero claims if `want` is zero, or if there are no request slots available.
    pub(crate) fn try_claim_requests(&self, want: usize) -> Vec<RequestClaim> {
        let max_requests = if self.enable_limiting {
            self.in_flight_limit
        } else {
            None
        };

        let mut available_requests = 0;
        self.in_flight_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |in_flight| {
                available_requests = match max_requests {
                    None => want,
                    Some(limit) => limit.saturating_sub(in_flight).min(want),
                };
                Some(in_flight + available_requests)
            })
            .unwrap(); // error not possible - callback always returns Some(_)

        (0..available_requests)
            .map(|_| RequestClaim {
                in_flight_count: self.in_flight_count.clone(),
            })
            .collect()
    }

    /// Claim a single request spot. Unlike try_claim_requests, this will wait until a
    /// request spot is available.
    pub(crate) fn claim_request(&self) -> RequestClaim {
        loop {
            match self.try_claim_requests(1).pop() {
                None => std::thread::sleep(Duration::from_millis(1)),
                Some(claim) => return claim,
            }
        }
    }

    pub(crate) fn with_limit(&self, limit: Option<usize>) -> Self {
        Self {
            enable_limiting: self.enable_limiting,
            in_flight_limit: limit,
            in_flight_count: self.in_flight_count.clone(),
        }
    }
}

/// Represents a claim of a single request spot. Releases claim on drop.
#[derive(Default)]
pub(crate) struct RequestClaim {
    in_flight_count: Arc<AtomicUsize>,
}

impl Drop for RequestClaim {
    fn drop(&mut self) {
        self.in_flight_count.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_no_limit_claimer() {
        let claimer = RequestClaimer::new(true, None);

        // works
        claimer.claim_request();

        assert_eq!(claimer.try_claim_requests(10_000).len(), 10_000);
    }

    #[test]
    fn test_limit_claimer() {
        let claimer = RequestClaimer::new(true, Some(5));

        // We ask for 10, but only 5 are available.
        let mut claims = claimer.try_claim_requests(10);
        assert_eq!(claims.len(), 5);

        // Can't get any more
        assert_eq!(claimer.try_claim_requests(10).len(), 0);

        // Free up a slot
        claims.pop().unwrap();

        // Can get one claim.
        let _single_claim = claimer.claim_request();

        // Can't get any more
        assert_eq!(claimer.try_claim_requests(10).len(), 0);

        // Now we only have one active claim.
        drop(claims);

        // Can get all 4.
        assert_eq!(claimer.try_claim_requests(4).len(), 4);

        // Can request 0.
        assert_eq!(claimer.try_claim_requests(0).len(), 0);

        // Still only 4 spots available.
        assert_eq!(claimer.try_claim_requests(5).len(), 4);
    }
}
