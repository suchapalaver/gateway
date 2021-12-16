use crate::{
    indexer_selection::{IndexerError, IndexerQuery},
    prelude::*,
};
use async_trait::async_trait;
use hex;
use reqwest;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait IndexerInterface {
    async fn query_indexer(&self, query: &IndexerQuery) -> Result<IndexerResponse, IndexerError>;
}

#[derive(Debug)]
pub struct IndexerResponse {
    pub status: u16,
    pub payload: String,
    pub attestation: Option<Attestation>,
}

#[derive(Debug, Deserialize)]
pub struct IndexerResponsePayload {
    #[serde(rename(deserialize = "graphQLResponse"))]
    pub graphql_response: String,
    pub attestation: Option<Attestation>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Attestation {
    #[serde(rename(deserialize = "requestCID"))]
    pub request_cid: Bytes32,
    #[serde(rename(deserialize = "responseCID"))]
    pub response_cid: Bytes32,
    #[serde(rename(deserialize = "subgraphDeploymentID"))]
    pub deployment: Bytes32,
    pub v: u8,
    pub r: Bytes32,
    pub s: Bytes32,
}

#[derive(Clone)]
pub struct IndexerClient {
    pub client: reqwest::Client,
}

#[async_trait]
impl IndexerInterface for IndexerClient {
    #[tracing::instrument(skip(self, query))]
    async fn query_indexer(&self, query: &IndexerQuery) -> Result<IndexerResponse, IndexerError> {
        let receipt = hex::encode(&query.receipt.commitment);
        let receipt = &receipt[0..(receipt.len() - 64)];
        let result = self
            .client
            .post(format!(
                "{}/subgraphs/id/{:?}",
                query.url, query.indexing.deployment
            ))
            .header("Content-Type", "application/json")
            .header("Scalar-Receipt", receipt)
            .body(query.query.clone())
            .send()
            .await;
        // We need to observe timeouts differently in the ISA, so we discriminate them here.
        let response = match result {
            Ok(response) => response,
            Err(err) if err.is_timeout() => return Err(IndexerError::Timeout),
            Err(err) => return Err(IndexerError::Other(err.to_string())),
        };
        let response_status = response.status();
        tracing::info!(%response_status);
        let payload = response
            .json::<IndexerResponsePayload>()
            .await
            .map_err(|err| IndexerError::Other(err.to_string()))?;
        Ok(IndexerResponse {
            status: response_status.as_u16(),
            payload: payload.graphql_response,
            attestation: payload.attestation,
        })
    }
}