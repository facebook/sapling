/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use observability_config::types::{
    ScubaObservabilityConfig, ScubaVerbosityLevel as CfgrScubaVerbosityLevel,
};

use scuba::ScubaValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScubaVerbosityLevel {
    Normal,
    Verbose,
}

#[derive(Clone)]
pub struct ScubaLoggingDecisionFields<'a> {
    pub maybe_session_id: Option<&'a ScubaValue>,
    pub maybe_unix_username: Option<&'a ScubaValue>,
    pub maybe_source_hostname: Option<&'a ScubaValue>,
}

fn cfgr_level_to_scuba_verbosity_level(level: &CfgrScubaVerbosityLevel) -> ScubaVerbosityLevel {
    match *level {
        CfgrScubaVerbosityLevel::Normal => ScubaVerbosityLevel::Normal,
        CfgrScubaVerbosityLevel::Verbose => ScubaVerbosityLevel::Verbose,
        // Let's default to normal verbosity if unknown value is parsed
        // (this can only happen if the interface was changed in configerator,
        // not synced here, and the newly added value was used)
        _ => ScubaVerbosityLevel::Normal,
    }
}

fn is_verbose(maybe_value: Option<&ScubaValue>, verbose_values: &[String]) -> bool {
    // Note: assymptotically speaking having `verbose_values` to be a `HashSet` would
    //       provide better big-O complexity, but I expect the lengths of these lists
    //       to be so small, that O(n) search would provide sufficient perf.
    if let Some(ScubaValue::Normal(this_value)) = maybe_value {
        verbose_values
            .iter()
            .any(|verbose_value| verbose_value == this_value)
    } else {
        false
    }
}

pub fn should_log_scuba_sample(
    sample_verbosity_level: ScubaVerbosityLevel,
    scuba_observability_config: &ScubaObservabilityConfig,
    logging_decision_fields: ScubaLoggingDecisionFields,
) -> bool {
    let current_system_verbosity_level =
        cfgr_level_to_scuba_verbosity_level(&scuba_observability_config.level);

    if sample_verbosity_level <= current_system_verbosity_level {
        // This sample should be logged regardless of its fields
        return true;
    }

    // Check if any of the `logging_decision_fields` convince
    // us to log this sample
    is_verbose(
        logging_decision_fields.maybe_session_id,
        &scuba_observability_config.verbose_sessions,
    ) || is_verbose(
        logging_decision_fields.maybe_unix_username,
        &scuba_observability_config.verbose_unixnames,
    ) || is_verbose(
        logging_decision_fields.maybe_source_hostname,
        &scuba_observability_config.verbose_source_hostnames,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_always_log_normal_samples() {
        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: None,
            maybe_source_hostname: None,
        };

        let observability_config = ScubaObservabilityConfig {
            level: CfgrScubaVerbosityLevel::Normal,
            verbose_sessions: Vec::new(),
            verbose_unixnames: Vec::new(),
            verbose_source_hostnames: Vec::new(),
        };

        assert!(should_log_scuba_sample(
            ScubaVerbosityLevel::Normal,
            &observability_config,
            fields.clone()
        ));

        let observability_config = ScubaObservabilityConfig {
            level: CfgrScubaVerbosityLevel::Verbose,
            verbose_sessions: Vec::new(),
            verbose_unixnames: Vec::new(),
            verbose_source_hostnames: Vec::new(),
        };

        assert!(should_log_scuba_sample(
            ScubaVerbosityLevel::Normal,
            &observability_config,
            fields,
        ));
    }

    #[test]
    fn test_log_any_sample_with_verbose_config() {
        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: None,
            maybe_source_hostname: None,
        };

        let observability_config = ScubaObservabilityConfig {
            level: CfgrScubaVerbosityLevel::Verbose,
            verbose_sessions: Vec::new(),
            verbose_unixnames: Vec::new(),
            verbose_source_hostnames: Vec::new(),
        };

        assert!(should_log_scuba_sample(
            ScubaVerbosityLevel::Normal,
            &observability_config,
            fields.clone()
        ));

        assert!(should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields.clone()
        ));
    }

    #[test]
    fn test_session_filtering() {
        let observability_config = ScubaObservabilityConfig {
            level: CfgrScubaVerbosityLevel::Normal,
            verbose_sessions: vec!["verbose_session".to_string()],
            verbose_unixnames: Vec::new(),
            verbose_source_hostnames: Vec::new(),
        };

        let verbose_session = ScubaValue::Normal("verbose_session".to_string());
        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: Some(&verbose_session),
            maybe_unix_username: None,
            maybe_source_hostname: None,
        };

        assert!(should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));

        let normal_session = ScubaValue::Normal("normal_session".to_string());
        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: Some(&normal_session),
            maybe_unix_username: None,
            maybe_source_hostname: None,
        };

        assert!(!should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));

        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: None,
            maybe_source_hostname: None,
        };

        assert!(!should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));
    }

    #[test]
    fn test_unixname_filtering() {
        let observability_config = ScubaObservabilityConfig {
            level: CfgrScubaVerbosityLevel::Normal,
            verbose_sessions: Vec::new(),
            verbose_unixnames: vec!["verbose_user".to_string()],
            verbose_source_hostnames: Vec::new(),
        };

        let verbose_user = ScubaValue::Normal("verbose_user".to_string());
        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: Some(&verbose_user),
            maybe_source_hostname: None,
        };

        assert!(should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));

        let normal_user = ScubaValue::Normal("normal_user".to_string());
        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: Some(&normal_user),
            maybe_source_hostname: None,
        };

        assert!(!should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));

        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: None,
            maybe_source_hostname: None,
        };

        assert!(!should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));
    }

    #[test]
    fn test_hostname_filtering() {
        let observability_config = ScubaObservabilityConfig {
            level: CfgrScubaVerbosityLevel::Normal,
            verbose_sessions: Vec::new(),
            verbose_unixnames: Vec::new(),
            verbose_source_hostnames: vec!["verbose_host.com".to_string()],
        };

        let verbose_host = ScubaValue::Normal("verbose_host.com".to_string());
        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: None,
            maybe_source_hostname: Some(&verbose_host),
        };

        assert!(should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));

        let normal_host = ScubaValue::Normal("normal_host.com".to_string());
        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: None,
            maybe_source_hostname: Some(&normal_host),
        };

        assert!(!should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));

        let fields = ScubaLoggingDecisionFields {
            maybe_session_id: None,
            maybe_unix_username: None,
            maybe_source_hostname: None,
        };

        assert!(!should_log_scuba_sample(
            ScubaVerbosityLevel::Verbose,
            &observability_config,
            fields
        ));
    }
}
