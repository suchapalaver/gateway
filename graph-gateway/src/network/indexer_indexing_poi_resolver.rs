//! A resolver for the Proof of Indexing (POI) of indexers.
//!
//! The resolver fetches the public POIs of indexers based on the given POIs metadata. It caches the
//! results of these requests to avoid making the same request multiple times.
//!
//! The cache has a TTL of 20 minutes. Entries are considered expired after this time causing the
//! resolver to fetch the public POIs of the indexer again.

use std::{collections::HashMap, time::Duration};

use alloy_primitives::BlockNumber;
use gateway_common::ttl_hash_map::TtlHashMap;
use thegraph_core::types::{DeploymentId, ProofOfIndexing};
use url::Url;

use crate::indexers;

/// The default TTL for cache entries is 20 minutes. Entries are considered expired after this time.
pub const DEFAULT_CACHE_TLL: Duration = Duration::from_secs(20 * 60); // 20 minutes

/// The timeout for the indexer indexings' POI resolution.
pub const DEFAULT_INDEXER_INDEXING_POIS_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(5);

/// The number of Public POIs to query in a single request.
const POIS_QUERY_BATCH_SIZE: usize = 10;

/// Error that can occur during POI resolution.
#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    /// Resolution timed out.
    #[error("timeout")]
    Timeout,
}

/// A resolver for the Proof of Indexing (POI) of indexers.
pub struct PoiResolver {
    client: reqwest::Client,
    cache: TtlHashMap<Url, HashMap<(DeploymentId, BlockNumber), ProofOfIndexing>>,
    timeout: Duration,
}

impl PoiResolver {
    /// Create a new [`PoiResolver`] with the given client.
    ///
    /// The client is used to make requests to indexers. The resolver caches the results of these
    /// requests to avoid making the same request multiple times.
    ///
    /// By default, the cache has a TTL of 20 minutes, [`DEFAULT_CACHE_TLL`]. Entries are considered
    /// expired after this time causing the resolver to make a new requests to the indexer.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            cache: TtlHashMap::with_ttl(DEFAULT_CACHE_TLL),
            timeout: DEFAULT_INDEXER_INDEXING_POIS_RESOLUTION_TIMEOUT,
        }
    }

    /// Create a new [`PoiResolver`] with the given client and timeout.
    pub fn with_timeout(client: reqwest::Client, timeout: Duration) -> Self {
        Self {
            client,
            cache: TtlHashMap::with_ttl(DEFAULT_CACHE_TLL),
            timeout,
        }
    }

    /// Fetch the public POIs of the indexer based on the given POIs metadata.
    async fn fetch_indexer_public_pois(
        &self,
        indexer_status_url: Url,
        pois: &[(DeploymentId, BlockNumber)],
    ) -> Result<HashMap<(DeploymentId, BlockNumber), ProofOfIndexing>, ResolutionError> {
        // TODO: Handle the different errors once the indexers client module reports them
        tokio::time::timeout(
            self.timeout,
            indexers::public_poi::merge_queries(
                self.client.clone(),
                indexer_status_url,
                pois,
                POIS_QUERY_BATCH_SIZE,
            ),
        )
        .await
        .map_err(|_| ResolutionError::Timeout)
    }

    /// Resolve the public POIs of the indexer based on the given POIs metadata.
    ///
    /// If the public POIs of the indexer are already in the cache, the resolver returns them.
    pub async fn resolve(
        &mut self,
        url: &Url,
        pois: &[(DeploymentId, BlockNumber)],
    ) -> Result<HashMap<(DeploymentId, BlockNumber), ProofOfIndexing>, ResolutionError> {
        let indexer_status_url = indexers::status_url(url);

        // Check if the indexer public POIs are already in the cache
        match self.cache.get(&indexer_status_url) {
            Some(pois) => Ok(pois.clone()),
            None => {
                // Fetch the public POIs of the indexer
                let pois = self
                    .fetch_indexer_public_pois(indexer_status_url.clone(), pois)
                    .await?;

                // Insert the public POIs into the cache
                self.cache.insert(indexer_status_url, pois.clone());

                Ok(pois)
            }
        }
    }
}
