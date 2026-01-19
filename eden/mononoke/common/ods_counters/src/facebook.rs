/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use configerator_structs_rapido_if_clients::RapidoClient;
use configerator_structs_rapido_if_clients::errors::QueryDataFromSourceError;
use configerator_structs_rapido_if_source::RapidoDataQuery;
use configerator_structs_rapido_if_source::RapidoDataResults;
use configerator_structs_rapido_if_srclients::make_Rapido_srclient;
use fbinit::FacebookInit;
use maplit::btreemap;
use thiserror::Error;
use tokio::time::Duration;
use tokio::time::interval;

use crate::CounterManager;

const ODS_STALENESS_THRESHOLD: TimeDelta = TimeDelta::seconds(60);
const ODS_QUERY_INTERVAL: Duration = Duration::from_mins(5);

#[derive(Clone)]
pub struct OdsCounterManager {
    fb: FacebookInit,
    pub counters:
        HashMap<(String, String, Option<String>, Option<String>), (DateTime<Utc>, Option<f64>)>,
}

impl OdsCounterManager {
    pub fn new(fb: FacebookInit) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(OdsCounterManager {
            fb,
            counters: HashMap::new(),
        }))
    }

    fn set_counter(
        &mut self,
        entity: &str,
        key: &str,
        reduce: Option<&str>,
        transform: Option<&str>,
        value: Option<f64>,
    ) {
        let counters = &mut self.counters;

        match value {
            Some(value) => {
                if let Some(counter) = counters.get_mut(&(
                    entity.to_string(),
                    key.to_string(),
                    reduce.map(|s| s.to_string()),
                    transform.map(|s| s.to_string()),
                )) {
                    *counter = (Utc::now(), Some(value));
                }
            }
            None => {
                if let Some(counter) = counters.get_mut(&(
                    entity.to_string(),
                    key.to_string(),
                    reduce.map(|s| s.to_string()),
                    transform.map(|s| s.to_string()),
                )) {
                    let (last_fetched, value) = *counter;
                    if Utc::now().signed_duration_since(last_fetched) > ODS_STALENESS_THRESHOLD {
                        *counter = (Utc::now(), None);
                    } else {
                        *counter = (last_fetched, value);
                    }
                }
            }
        };
    }
}

#[async_trait]
impl CounterManager for OdsCounterManager {
    fn add_counter(
        &mut self,
        entity: String,
        key: String,
        reduce: Option<String>,
        transform: Option<String>,
    ) {
        self.counters.insert(
            (
                entity.clone(),
                key.clone(),
                reduce.clone(),
                transform.clone(),
            ),
            (Utc::now(), None),
        );
    }

    fn get_counter_value(
        &self,
        entity: &str,
        key: &str,
        reduce: Option<&str>,
        transform: Option<&str>,
    ) -> Option<f64> {
        self.counters
            .get(&(
                entity.to_string(),
                key.to_string(),
                reduce.map(|s| s.to_string()),
                transform.map(|s| s.to_string()),
            ))
            .and_then(|(_, value)| *value)
    }
}

async fn fetch_counter(
    fb: FacebookInit,
    entity: &str,
    key: &str,
    reduce: Option<String>,
    transform: Option<String>,
) -> Option<f64> {
    let client = make_Rapido_srclient!(fb).unwrap();
    let query = OdsQuery::new(entity.to_string(), key.to_string());
    let start_time = (Utc::now() - ODS_QUERY_INTERVAL).timestamp();
    let end_time = Utc::now().timestamp();
    let query_detail = query.query_detail(start_time, end_time, transform, reduce);
    OdsQuery::query_latest_value(&client, query_detail)
        .await
        .ok()
}

pub async fn periodic_fetch_counter(
    manager: Arc<RwLock<OdsCounterManager>>,
    interval_duration: Duration,
) {
    let mut interval = interval(interval_duration);

    loop {
        interval.tick().await;

        // Acquire the read guard once to get the keys
        let (fb, keys) = {
            let manager = manager.read().unwrap();
            (
                manager.fb,
                manager.counters.keys().cloned().collect::<Vec<_>>(),
            )
        };

        // Prepare a vector to store the fetched values
        let mut fetched_values = Vec::new();

        // Fetch the counter values asynchronously
        for (entity, key, reduce, transform) in &keys {
            let value = fetch_counter(fb, entity, key, reduce.clone(), transform.clone()).await;
            fetched_values.push((
                entity.clone(),
                key.clone(),
                reduce.clone(),
                transform.clone(),
                value,
            ));
        }

        // Acquire the write guard once to set the counter values
        {
            let mut manager = manager.write().unwrap();
            for (entity, key, reduce, transform, value) in fetched_values {
                manager.set_counter(
                    &entity,
                    &key,
                    reduce.as_deref(),
                    transform.as_deref(),
                    value,
                );
            }
        }
    }
}

/// ODS error type
#[derive(Error, Debug)]
pub enum OdsError {
    /// Err for regular ods query, retry suggested
    #[error("Error caught when querying ods: {0}")]
    OdsQueryErr(#[from] QueryDataFromSourceError),

    /// Permanent err, retry not suggested
    #[error("Permanent error caught when interacting with ods: {0}")]
    PermanentErr(String),
}

/// Utilities for reading from ODS
///
/// # Example
/// let client = make_Rapido_srclient!(fb)?;
/// let ek_pair = OdsQuery::new("entity".to_string(), "key".to_string());
/// let query1 = ek_pair.query_detail(123456, 123457, None, None);
/// let res1 = OdsQuery::query(&client, query1).await?;
/// Struct for init ODS query
pub struct OdsQuery {
    entity: String,
    key: String,
}

impl OdsQuery {
    /// Constructor
    pub fn new(entity: String, key: String) -> Self {
        OdsQuery { entity, key }
    }

    /// Create a RapidoDataQuery with query details
    pub fn query_detail(
        &self,
        start_time: i64,
        end_time: i64,
        transforms_str: Option<String>,
        reduce_str: Option<String>,
    ) -> RapidoDataQuery {
        let _ = end_time;
        let mut time_series_description = btreemap! {
            "entity".to_string() => self.entity.to_string(),
            "keys".to_string() => self.key.to_string(),
        };

        if let Some(transforms_str) = transforms_str {
            time_series_description.insert("transforms".to_string(), transforms_str.to_string());
        }

        if let Some(reduce_str) = reduce_str {
            time_series_description.insert("reduce".to_string(), reduce_str.to_string());
        }

        RapidoDataQuery {
            timeSeriesDescription: time_series_description,
            source: "ods".to_string(),
            startTime: start_time,
            endTime: end_time,
            ..Default::default()
        }
    }

    /// Regular ODS query
    pub async fn query(
        client: &RapidoClient,
        query_detail: RapidoDataQuery,
    ) -> Result<RapidoDataResults, OdsError> {
        match client.queryDataFromSource(&query_detail).await {
            Ok(results) => Ok(results),
            Err(e) => match e {
                QueryDataFromSourceError::rue(err) => {
                    Err(OdsError::PermanentErr(err.message.to_string()))
                }
                _ => Err(OdsError::OdsQueryErr(e)),
            },
        }
    }

    /// Query and return the latest value
    pub async fn query_latest_value(
        client: &RapidoClient,
        query_detail: RapidoDataQuery,
    ) -> Result<f64, OdsError> {
        let query_result = Self::query(client, query_detail).await?;
        let latest_value = Self::parse_query_result(&query_result)?;
        Ok(latest_value)
    }

    /// Parse ODS query result
    pub fn parse_query_result(result: &RapidoDataResults) -> Result<f64, OdsError> {
        // Check that there is only one time series
        if result.timeSeries.len() != 1 {
            return Err(OdsError::PermanentErr(
                "Expected exactly one time series in ODS query result".to_string(),
            ));
        }

        // Find the latest value by iterating through the time series
        let latest = result
            .timeSeries
            .iter()
            .flat_map(|time_series| &time_series.timeValues)
            .max_by_key(|&(timestamp, _)| timestamp);

        if let Some((_, value)) = latest {
            Ok(*value)
        } else {
            Err(OdsError::PermanentErr(
                "No values found in ODS query result".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod test {

    use std::sync::Arc;

    use configerator_structs_rapido_if_clients::Rapido;
    use configerator_structs_rapido_if_source::RapidoTimeSeries;
    use mononoke_macros::mononoke;

    use super::*;

    pub fn mock_rapido_client() -> RapidoClient {
        let mock_rapido_client =
            Arc::new(configerator_structs_rapido_if_mocks::new::<dyn Rapido>());

        // Form data results
        let mut rapido_time_series_1 = RapidoTimeSeries {
            timeSeriesDescription: btreemap! {
                "entity".to_string() => "time_series_description1".to_string()
            },
            ..RapidoTimeSeries::default()
        };

        rapido_time_series_1.timeValues.insert(123456, 0.5);
        rapido_time_series_1.timeValues.insert(123457, 1.5);

        let data_results1 = RapidoDataResults {
            timeSeries: vec![rapido_time_series_1],
            ..RapidoDataResults::default()
        };
        let data_results2 = data_results1.clone();

        mock_rapido_client
            .queryDataFromSource
            .mock_result(move |_query| Ok(data_results2.clone()));
        mock_rapido_client
    }

    pub fn mock_rapido_client_failure() -> RapidoClient {
        let mock_rapido_client =
            Arc::new(configerator_structs_rapido_if_mocks::new::<dyn Rapido>());
        mock_rapido_client
            .queryDataFromSource
            .mock_result(|_query| {
                Err(QueryDataFromSourceError::ThriftError(anyhow::Error::msg(
                    "An error occurred".to_string(),
                )))
            });
        mock_rapido_client
    }

    #[tokio::test]
    async fn test_query() {
        let mock_rapido_client = mock_rapido_client();
        let ods_query = OdsQuery::new("entity".to_string(), "key".to_string());
        let query_detail = ods_query.query_detail(123456, 123457, None, None);
        let ods_query_result = OdsQuery::query(&mock_rapido_client, query_detail).await;
        assert_eq!(
            ods_query_result.unwrap().timeSeries[0].timeValues,
            btreemap! {123456 => 0.5, 123457 => 1.5}
        );
    }

    #[tokio::test]
    async fn test_query_failure() {
        let mock_rapido_client = mock_rapido_client_failure();
        let ods_query = OdsQuery::new("entity".to_string(), "key".to_string());
        let query_detail = ods_query.query_detail(123456, 123457, None, None);
        let ods_query_result = OdsQuery::query(&mock_rapido_client, query_detail).await;
        assert!(ods_query_result.is_err());
    }

    #[tokio::test]
    async fn test_query_latest_value() {
        let mock_rapido_client = mock_rapido_client();
        let ods_query = OdsQuery::new("entity".to_string(), "key".to_string());
        let query_detail = ods_query.query_detail(123456, 123457, None, None);
        let latest_value = OdsQuery::query_latest_value(&mock_rapido_client, query_detail)
            .await
            .unwrap();
        assert_eq!(latest_value, 1.5);
    }

    #[mononoke::fbinit_test]
    async fn test_ods_counter_manager(fb: FacebookInit) {
        let manager = Arc::new(RwLock::new(OdsCounterManager {
            fb,
            counters: HashMap::new(),
        }));

        // Add a new counter
        manager
            .write()
            .unwrap()
            .add_counter("entity".to_string(), "key".to_string(), None, None);

        // Check the counter value
        let value = manager
            .read()
            .unwrap()
            .get_counter_value("entity", "key", None, None);

        assert_eq!(value, None);

        // Give it a value
        {
            manager.write().unwrap().counters.insert(
                ("entity".to_string(), "key".to_string(), None, None),
                (Utc::now(), Some(5.0)),
            );
        }

        // Check the value of the new counter in counters
        let timestamp = {
            let manager_lock = manager.read().unwrap();
            let values = manager_lock
                .counters
                .get(&("entity".to_string(), "key".to_string(), None, None))
                .clone();
            let (timestamp, value) = values.unwrap();
            assert!(timestamp.timestamp() > 0);
            assert_eq!(*value, Some(5.0));
            timestamp.clone()
        };

        let clone = manager.clone();

        mononoke::spawn_task(periodic_fetch_counter(clone, Duration::from_secs(1)));

        // Wait for the counter to be fetched
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Check that the timestamp has not been updated, since we haven't been able to fetch a new value
        {
            let manager_lock = manager.read().unwrap();
            let values = manager_lock
                .counters
                .get(&("entity".to_string(), "key".to_string(), None, None))
                .clone();

            let (second_timestamp, value) = values.unwrap();
            assert_eq!(second_timestamp.timestamp(), timestamp.timestamp());
            assert_eq!(*value, Some(5.0));
        }
    }
}
