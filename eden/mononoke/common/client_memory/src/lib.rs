/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::LazyLock;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use permission_checker::ClientCategory;
use permission_checker::MononokeIdentitySet;
use permission_checker::MononokeIdentitySetExt;

/// When enabled for a client, its [`ClientGroup`] is derived from its
/// identities instead of from substrings of its `main_client_id`. Evaluated
/// with the `main_client_id` as the hash value so a partial rollout classifies
/// each client consistently across requests and hosts.
const GROUPS_FROM_IDENTITIES_KNOB: &str = "scm/mononoke:load_shed_client_groups_from_identities";

/// Coarse client group used to shed load when no single client dominates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum ClientGroup {
    Quicksand = 0,
    Sandcastle = 1,
    DevEnv = 2,
    InteractiveDev = 3,
    Automation = 4,
    FaaS = 5,
    Mast = 6,
    Other = 7,
}

impl ClientGroup {
    pub const COUNT: usize = 8;

    pub const ALL: [ClientGroup; Self::COUNT] = [
        Self::Quicksand,
        Self::Sandcastle,
        Self::DevEnv,
        Self::InteractiveDev,
        Self::Automation,
        Self::FaaS,
        Self::Mast,
        Self::Other,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::Quicksand => "quicksand",
            Self::Sandcastle => "sandcastle",
            Self::DevEnv => "dev_env",
            Self::InteractiveDev => "interactive_dev",
            Self::Automation => "automation",
            Self::FaaS => "faas",
            Self::Mast => "mast",
            Self::Other => "other",
        }
    }

    /// The group a request's memory is attributed to and shed under. Both the
    /// memory tracker and the load shedder must use this so they agree.
    pub fn for_request(identities: Option<&MononokeIdentitySet>, main_id: Option<&str>) -> Self {
        if justknobs::eval(GROUPS_FROM_IDENTITIES_KNOB, main_id, None) {
            identities.map_or(Self::Other, Self::from_identities)
        } else {
            Self::from_main_id(main_id)
        }
    }

    /// Classify from identities using the shared `client_category()` rules.
    /// Both Sandcastle categories are merged because they differ only by the
    /// presence of a Sandcastle alias.
    pub fn from_identities(identities: &MononokeIdentitySet) -> Self {
        let is_quicksand = identities.iter().any(|identity| {
            identity.id_type() == "SERVICE_IDENTITY" && identity.id_data().contains("quicksand")
        });
        if is_quicksand {
            return Self::Quicksand;
        }
        identities.client_category(None).into()
    }

    /// Classify from substrings of `main_client_id`. This misclassifies
    /// clients whose `main_client_id` does not name their type, such as
    /// Sandcastle CI (`ALIAS:<alias>`) and devservers (`MACHINE:<host>`).
    pub fn from_main_id(main_id: Option<&str>) -> Self {
        match main_id {
            Some(id) if id.contains("quicksand") => Self::Quicksand,
            Some(id) if id.contains("ondemand") => Self::DevEnv,
            Some(id) if id.contains("sandcastle") => Self::Sandcastle,
            _ => Self::Other,
        }
    }
}

impl From<ClientCategory> for ClientGroup {
    fn from(category: ClientCategory) -> Self {
        match category {
            ClientCategory::CiSandcastle | ClientCategory::SandcastleAutomation => Self::Sandcastle,
            ClientCategory::DevEnv => Self::DevEnv,
            ClientCategory::InteractiveDev => Self::InteractiveDev,
            ClientCategory::Automation => Self::Automation,
            ClientCategory::FaaS => Self::FaaS,
            ClientCategory::Mast => Self::Mast,
            ClientCategory::HealthCheck | ClientCategory::Unknown => Self::Other,
        }
    }
}

/// Lock-free, process-global registry tracking estimated in-flight memory
/// per [`ClientGroup`].
///
/// Updated by `WeightTracker` during request processing, queried by the
/// rate limiting middleware to decide which group to shed.
pub struct ClientMemoryRegistry {
    groups: [AtomicI64; ClientGroup::COUNT],
}

impl ClientMemoryRegistry {
    pub fn new() -> Self {
        Self {
            groups: std::array::from_fn(|_| AtomicI64::new(0)),
        }
    }

    pub fn add_weight(&self, group: ClientGroup, weight: i64) {
        self.groups[group as usize].fetch_add(weight, Ordering::Relaxed);
    }

    pub fn remove_weight(&self, group: ClientGroup, weight: i64) {
        self.add_weight(group, -weight);
    }

    pub fn group_memory(&self, group: ClientGroup) -> i64 {
        self.groups[group as usize].load(Ordering::Relaxed)
    }

    pub fn total_memory(&self) -> i64 {
        ClientGroup::ALL
            .into_iter()
            .map(|group| self.group_memory(group))
            .sum()
    }

    /// The group using the most estimated memory, or `None` if nothing is
    /// tracked.
    pub fn top_group(&self) -> Option<ClientGroup> {
        ClientGroup::ALL
            .into_iter()
            .map(|group| (group, self.group_memory(group)))
            .max_by_key(|(_, weight)| *weight)
            .filter(|(_, weight)| *weight > 0)
            .map(|(group, _)| group)
    }
}

impl Default for ClientMemoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global singleton registry.
static GLOBAL_REGISTRY: LazyLock<ClientMemoryRegistry> = LazyLock::new(ClientMemoryRegistry::new);

pub fn global_client_memory_registry() -> &'static ClientMemoryRegistry {
    &GLOBAL_REGISTRY
}

#[cfg(test)]
mod tests {
    #[cfg(fbcode_build)]
    use std::collections::HashMap;

    #[cfg(fbcode_build)]
    use justknobs::test_helpers::JustKnobsInMemory;
    #[cfg(fbcode_build)]
    use justknobs::test_helpers::KnobVal;
    #[cfg(fbcode_build)]
    use justknobs::test_helpers::with_just_knobs;
    use mononoke_macros::mononoke;
    #[cfg(fbcode_build)]
    use permission_checker::MononokeIdentity;

    use super::*;

    #[cfg(fbcode_build)]
    fn idents(entries: &[(&str, &str)]) -> MononokeIdentitySet {
        entries
            .iter()
            .map(|(id_type, id_data)| MononokeIdentity::from_legacy_type_data(*id_type, *id_data))
            .collect()
    }

    #[mononoke::test]
    fn test_client_memory_registry() {
        let registry = ClientMemoryRegistry::new();
        assert_eq!(registry.total_memory(), 0);
        assert_eq!(registry.top_group(), None);

        registry.add_weight(ClientGroup::Quicksand, 100);
        registry.add_weight(ClientGroup::Sandcastle, 300);
        assert_eq!(registry.group_memory(ClientGroup::Sandcastle), 300);
        assert_eq!(registry.total_memory(), 400);
        assert_eq!(registry.top_group(), Some(ClientGroup::Sandcastle));

        registry.remove_weight(ClientGroup::Sandcastle, 300);
        assert_eq!(registry.total_memory(), 100);
        assert_eq!(registry.top_group(), Some(ClientGroup::Quicksand));

        registry.remove_weight(ClientGroup::Quicksand, 100);
        assert_eq!(
            registry.top_group(),
            None,
            "a group with no memory is never the top group"
        );
    }

    #[mononoke::test]
    fn test_group_from_main_id() {
        assert_eq!(
            ClientGroup::from_main_id(Some("SERVICE_IDENTITY:quicksand_builder")),
            ClientGroup::Quicksand
        );
        assert_eq!(
            ClientGroup::from_main_id(Some("SERVICE_IDENTITY:ondemand_unity")),
            ClientGroup::DevEnv
        );
        assert_eq!(
            ClientGroup::from_main_id(Some("SERVICE_IDENTITY:sandcastle.worker.warmed")),
            ClientGroup::Sandcastle
        );
        assert_eq!(
            ClientGroup::from_main_id(Some("ALIAS:sky-build")),
            ClientGroup::Other,
            "an alias-only Sandcastle main_client_id does not name the client type"
        );
        assert_eq!(ClientGroup::from_main_id(None), ClientGroup::Other);
    }

    #[cfg(fbcode_build)]
    #[mononoke::test]
    fn test_for_request_uses_identities_only_when_enabled() {
        let sandcastle = idents(&[("SERVICE_IDENTITY", "sandcastle.worker.warmed")]);
        assert_eq!(
            ClientGroup::for_request(Some(&sandcastle), Some("ALIAS:sky-build")),
            ClientGroup::Other,
            "the knob defaults to off, keeping the main_client_id classification"
        );
        with_just_knobs(
            JustKnobsInMemory::new(HashMap::from([(
                GROUPS_FROM_IDENTITIES_KNOB.to_string(),
                KnobVal::Bool(true),
            )])),
            || {
                assert_eq!(
                    ClientGroup::for_request(Some(&sandcastle), Some("ALIAS:sky-build")),
                    ClientGroup::Sandcastle
                );
                assert_eq!(
                    ClientGroup::for_request(None, Some("ALIAS:sky-build")),
                    ClientGroup::Other
                );
            },
        );
    }

    #[cfg(fbcode_build)]
    #[mononoke::test]
    fn test_quicksand_group_from_identities() {
        assert_eq!(
            ClientGroup::from_identities(&idents(&[("SERVICE_IDENTITY", "quicksand_builder")])),
            ClientGroup::Quicksand
        );
    }

    #[mononoke::test]
    fn test_group_from_category() {
        assert_eq!(
            ClientGroup::from(ClientCategory::CiSandcastle),
            ClientGroup::Sandcastle
        );
        assert_eq!(
            ClientGroup::from(ClientCategory::SandcastleAutomation),
            ClientGroup::Sandcastle
        );
        assert_eq!(
            ClientGroup::from(ClientCategory::DevEnv),
            ClientGroup::DevEnv
        );
        assert_eq!(
            ClientGroup::from(ClientCategory::HealthCheck),
            ClientGroup::Other
        );
        assert_eq!(
            ClientGroup::from(ClientCategory::Unknown),
            ClientGroup::Other
        );
    }

    #[cfg(fbcode_build)]
    #[mononoke::test]
    fn test_group_from_identities_does_not_depend_on_main_client_id() {
        // Sandcastle CI has an alias-only main_client_id (`ALIAS:<alias>`) and
        // devservers have `MACHINE:<host>`; neither names the client type.
        assert_eq!(
            ClientGroup::from_identities(&idents(&[(
                "SERVICE_IDENTITY",
                "sandcastle.worker.warmed"
            )])),
            ClientGroup::Sandcastle
        );
        assert_eq!(
            ClientGroup::from_identities(&idents(&[
                ("MACHINE", "devbig009.ncg6.facebook.com"),
                ("SERVICE_IDENTITY", "devenv_prereservation"),
            ])),
            ClientGroup::DevEnv
        );
        assert_eq!(
            ClientGroup::from_identities(&idents(&[("USER", "johndoe")])),
            ClientGroup::InteractiveDev
        );
    }
}
