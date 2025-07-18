use std::{collections::HashSet, time::Duration};

use anyhow::{Context, anyhow};
use ordered_float::NotNan;
use prost::Message;
use thegraph_core::{DeploymentId, IndexerId, SubgraphId, alloy::primitives::Address};
use tokio::{sync::mpsc, time::Instant};

use crate::{concat_bytes, errors, indexer_client::IndexerResponse, receipts::Receipt};

pub struct ClientRequest {
    pub id: String,
    pub response_time_ms: u16,
    pub result: Result<(), errors::Error>,
    pub api_key: String,
    pub user: String,
    pub subgraph: Option<SubgraphId>,
    pub grt_per_usd: NotNan<f64>,
    pub indexer_requests: Vec<IndexerRequest>,
    pub request_bytes: u32,
    pub response_bytes: Option<u32>,
}

pub struct IndexerRequest {
    pub indexer: IndexerId,
    pub deployment: DeploymentId,
    pub url: String,
    /// Receipt contains either v1 (allocation-based) or v2 (collection-based) receipt.
    /// For reporting, collection IDs are converted to addresses for backward compatibility.
    pub receipt: Receipt,
    pub subgraph_chain: String,
    pub result: Result<IndexerResponse, errors::IndexerError>,
    pub response_time_ms: u16,
    pub seconds_behind: u32,
    pub blocks_behind: u64, // TODO: rm
    pub request: String,
}

pub struct Topics {
    pub queries: &'static str,
    pub attestations: &'static str,
}

pub struct Reporter {
    tap_signer: Address,
    graph_env: String,
    topics: Topics,
    write_buf: Vec<u8>,
    kafka_producer: rdkafka::producer::ThreadedProducer<
        rdkafka::producer::DefaultProducerContext,
        rdkafka::producer::NoCustomPartitioner,
    >,
    attestation_sampler: AttestationSampler,
}

struct AttestationSampler {
    records: HashSet<(DeploymentId, Address)>,
    last_eviction: Instant,
}

impl AttestationSampler {
    fn new() -> Self {
        Self {
            records: Default::default(),
            last_eviction: Instant::now(),
        }
    }

    fn should_sample(&mut self, now: Instant, deployment: DeploymentId, indexer: Address) -> bool {
        if now.duration_since(self.last_eviction) > Duration::from_secs(10) {
            self.records.clear();
            self.last_eviction = now;
        }
        self.records.insert((deployment, indexer))
    }
}

impl Reporter {
    pub fn create(
        tap_signer: Address,
        graph_env: String,
        topics: Topics,
        kafka_config: impl Into<rdkafka::ClientConfig>,
    ) -> anyhow::Result<mpsc::UnboundedSender<ClientRequest>> {
        let kafka_producer = kafka_config
            .into()
            .create()
            .context("kafka producer error")?;
        let mut reporter = Self {
            tap_signer,
            graph_env,
            topics,
            write_buf: Default::default(),
            kafka_producer,
            attestation_sampler: AttestationSampler::new(),
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Err(report_err) = reporter.report(msg) {
                    tracing::error!(%report_err);
                }
            }
        });
        Ok(tx)
    }

    fn report(&mut self, client_request: ClientRequest) -> anyhow::Result<()> {
        let indexer_queries = client_request
            .indexer_requests
            .iter()
            .map(|indexer_request| {
                let (allocation, collection) = if indexer_request.receipt.is_v1() {
                    (
                        Some(indexer_request.receipt.allocation().as_ref().to_vec()),
                        None,
                    )
                } else {
                    (
                        None,
                        Some(indexer_request.receipt.collection().as_ref().to_vec()),
                    )
                };

                IndexerQueryProtobuf {
                    indexer: indexer_request.indexer.to_vec(),
                    deployment: indexer_request.deployment.to_vec(),
                    allocation,
                    collection,
                    indexed_chain: indexer_request.subgraph_chain.clone(),
                    url: indexer_request.url.clone(),
                    fee_grt: indexer_request.receipt.value() as f64 * 1e-18,
                    response_time_ms: indexer_request.response_time_ms as u32,
                    seconds_behind: indexer_request.seconds_behind,
                    result: indexer_request
                        .result
                        .as_ref()
                        .map(|_| "success".to_string())
                        .unwrap_or_else(|err| err.to_string()),
                    indexer_errors: indexer_request
                        .result
                        .as_ref()
                        .map(|r| {
                            r.errors
                                .iter()
                                .map(|err| err.as_str())
                                .collect::<Vec<&str>>()
                                .join("; ")
                        })
                        .unwrap_or_default(),
                    blocks_behind: indexer_request.blocks_behind,
                }
            })
            .collect();

        let total_fees_grt: f64 = client_request
            .indexer_requests
            .iter()
            .map(|i| i.receipt.value() as f64 * 1e-18)
            .sum();
        let total_fees_usd: f64 = total_fees_grt / *client_request.grt_per_usd;

        let client_query_msg = ClientQueryProtobuf {
            gateway_id: self.graph_env.clone(),
            receipt_signer: self.tap_signer.to_vec(),
            query_id: client_request.id,
            api_key: client_request.api_key,
            user_id: client_request.user,
            subgraph: client_request.subgraph.map(|s| s.to_string()),
            result: client_request
                .result
                .map(|()| "success".to_string())
                .unwrap_or_else(|err| err.to_string()),
            response_time_ms: client_request.response_time_ms as u32,
            request_bytes: client_request.request_bytes,
            response_bytes: client_request.response_bytes,
            total_fees_usd,
            indexer_queries,
        };

        client_query_msg.encode(&mut self.write_buf).unwrap();
        let record: rdkafka::producer::BaseRecord<(), [u8], ()> =
            rdkafka::producer::BaseRecord::to(self.topics.queries).payload(&self.write_buf);
        self.kafka_producer
            .send(record)
            .map_err(|(err, _)| err)
            .context(anyhow!("failed to send to topic {}", self.topics.queries))?;
        self.write_buf.clear();

        let now = Instant::now();
        for indexer_request in client_request.indexer_requests {
            if !self.attestation_sampler.should_sample(
                now,
                indexer_request.deployment,
                *indexer_request.indexer,
            ) {
                continue;
            }
            if let Some((original_response, attestation)) = indexer_request
                .result
                .ok()
                .and_then(|r| Some((r.original_response, r.attestation?)))
            {
                const MAX_PAYLOAD_BYTES: usize = 100_000;
                let allocation = if indexer_request.receipt.is_v1() {
                    indexer_request.receipt.allocation().as_ref().to_vec() // Direct allocation for v1
                } else {
                    indexer_request.receipt.collection().as_address().to_vec() // Collection as address for v2
                };

                AttestationProtobuf {
                    request: Some(indexer_request.request).filter(|r| r.len() <= MAX_PAYLOAD_BYTES),
                    response: Some(original_response).filter(|r| r.len() <= MAX_PAYLOAD_BYTES),
                    allocation,
                    subgraph_deployment: attestation.deployment.0.into(),
                    request_cid: attestation.request_cid.0.into(),
                    response_cid: attestation.response_cid.0.into(),
                    signature: concat_bytes!(
                        65,
                        [&[attestation.v], &attestation.r.0, &attestation.s.0]
                    )
                    .into(),
                }
                .encode(&mut self.write_buf)
                .unwrap();
                let record: rdkafka::producer::BaseRecord<(), [u8], ()> =
                    rdkafka::producer::BaseRecord::to(self.topics.attestations)
                        .payload(&self.write_buf);
                self.kafka_producer
                    .send(record)
                    .map_err(|(err, _)| err)
                    .context(anyhow!(
                        "failed to send to topic {}",
                        self.topics.attestations
                    ))?;
                self.write_buf.clear();
            }
        }

        Ok(())
    }
}

#[derive(prost::Message)]
pub struct ClientQueryProtobuf {
    #[prost(string, tag = "1")]
    gateway_id: String,
    // 20 bytes
    #[prost(bytes, tag = "2")]
    receipt_signer: Vec<u8>,
    #[prost(string, tag = "3")]
    query_id: String,
    #[prost(string, tag = "4")]
    api_key: String,
    #[prost(string, tag = "11")]
    user_id: String,
    #[prost(string, optional, tag = "12")]
    subgraph: Option<String>,
    #[prost(string, tag = "5")]
    result: String,
    #[prost(uint32, tag = "6")]
    response_time_ms: u32,
    #[prost(uint32, tag = "7")]
    request_bytes: u32,
    #[prost(uint32, optional, tag = "8")]
    response_bytes: Option<u32>,
    #[prost(double, tag = "9")]
    total_fees_usd: f64,
    #[prost(message, repeated, tag = "10")]
    indexer_queries: Vec<IndexerQueryProtobuf>,
}

#[derive(prost::Message)]
pub struct IndexerQueryProtobuf {
    /// 20 bytes
    #[prost(bytes, tag = "1")]
    indexer: Vec<u8>,
    /// 32 bytes
    #[prost(bytes, tag = "2")]
    deployment: Vec<u8>,
    /// 20 bytes - Allocation ID for v1 receipts
    #[prost(bytes, optional, tag = "3")]
    allocation: Option<Vec<u8>>,
    /// 32 bytes - Collection ID for v2 receipts
    #[prost(bytes, optional, tag = "12")]
    collection: Option<Vec<u8>>,
    #[prost(string, tag = "4")]
    indexed_chain: String,
    #[prost(string, tag = "5")]
    url: String,
    #[prost(double, tag = "6")]
    fee_grt: f64,
    #[prost(uint32, tag = "7")]
    response_time_ms: u32,
    #[prost(uint32, tag = "8")]
    seconds_behind: u32,
    #[prost(string, tag = "9")]
    result: String,
    #[prost(string, tag = "10")]
    indexer_errors: String,
    #[prost(uint64, tag = "11")]
    blocks_behind: u64,
}

#[derive(prost::Message)]
pub struct AttestationProtobuf {
    #[prost(string, optional, tag = "1")]
    request: Option<String>,
    #[prost(string, optional, tag = "2")]
    response: Option<String>,
    /// 20 bytes - Collection ID converted to address for backward compatibility
    #[prost(bytes, tag = "3")]
    allocation: Vec<u8>,
    /// 32 bytes
    #[prost(bytes, tag = "4")]
    subgraph_deployment: Vec<u8>,
    /// 32 bytes
    #[prost(bytes, tag = "5")]
    request_cid: Vec<u8>,
    /// 32 bytes
    #[prost(bytes, tag = "6")]
    response_cid: Vec<u8>,
    /// 65 bytes, ECDSA signature (v, r, s)
    #[prost(bytes, tag = "7")]
    signature: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use thegraph_core::{
        allocation_id,
        alloy::{primitives::address, signers::local::PrivateKeySigner},
        collection_id,
    };

    use crate::receipts::{Receipt, ReceiptSigner};

    fn create_test_signer() -> ReceiptSigner {
        let secret_key = PrivateKeySigner::from_slice(&[0xcd; 32]).expect("invalid secret key");
        ReceiptSigner::new(
            secret_key,
            1.try_into().expect("invalid chain id"),
            address!("177b557b12f22bb17a9d73dcc994d978dd6f5f89"),
        )
    }

    #[test]
    fn test_protobuf_fields_v1_receipt() {
        let allocation = allocation_id!("89b23fea4e46d40e8a4c6cca723e2a03fdd4bec2");
        let fee = 1000;
        let v1_receipt = Receipt::create_v1_for_processing(allocation, fee, 1234567890, 42);

        // Test IndexerQueryProtobuf fields
        let (allocation_field, collection_field) = if v1_receipt.is_v1() {
            (Some(v1_receipt.allocation().as_ref().to_vec()), None)
        } else {
            (None, Some(v1_receipt.collection().as_ref().to_vec()))
        };

        assert!(
            allocation_field.is_some(),
            "v1 receipt should have allocation field"
        );
        assert!(
            collection_field.is_none(),
            "v1 receipt should not have collection field"
        );
        assert_eq!(allocation_field.unwrap(), allocation.as_ref().to_vec());
    }

    #[test]
    fn test_protobuf_fields_v2_receipt() {
        let signer = create_test_signer();
        let collection =
            collection_id!("89b23fea4e46d40e8a4c6cca723e2a03fdd4bec2a00000000000000000000000");
        let fee = 1000;

        let v2_receipt = signer
            .create_receipt(
                collection,
                fee,
                address!("1111111111111111111111111111111111111111"),
                address!("2222222222222222222222222222222222222222"),
                address!("3333333333333333333333333333333333333333"),
            )
            .expect("failed to create v2 receipt");

        // Test IndexerQueryProtobuf fields
        let (allocation_field, collection_field) = if v2_receipt.is_v1() {
            (Some(v2_receipt.allocation().as_ref().to_vec()), None)
        } else {
            (None, Some(v2_receipt.collection().as_ref().to_vec()))
        };

        assert!(
            allocation_field.is_none(),
            "v2 receipt should not have allocation field"
        );
        assert!(
            collection_field.is_some(),
            "v2 receipt should have collection field"
        );
        assert_eq!(collection_field.unwrap(), collection.as_ref().to_vec());
    }

    /// Test that simulates tap-aggregator receipt processing
    #[test]
    fn test_tap_aggregator_v1_compatibility() {
        let allocation = allocation_id!("89b23fea4e46d40e8a4c6cca723e2a03fdd4bec2");
        let fee = 1000u128;
        let v1_receipt = Receipt::create_v1_for_processing(allocation, fee, 1234567890, 42);

        // Simulate IndexerQueryProtobuf creation (what gateway sends)
        let (allocation_field, collection_field) = if v1_receipt.is_v1() {
            (Some(v1_receipt.allocation().as_ref().to_vec()), None)
        } else {
            (None, Some(v1_receipt.collection().as_ref().to_vec()))
        };

        // Verify format matches tap-aggregator v1 expectations
        assert!(
            allocation_field.is_some(),
            "v1 receipt must have allocation field"
        );
        assert!(
            collection_field.is_none(),
            "v1 receipt must not have collection field"
        );

        let allocation_bytes = allocation_field.unwrap();
        assert_eq!(allocation_bytes.len(), 20, "allocation should be 20 bytes");
        assert_eq!(allocation_bytes, allocation.as_ref().to_vec());
    }

    /// Test that simulates tap-aggregator v2 receipt processing
    #[test]
    fn test_tap_aggregator_v2_compatibility() {
        let signer = create_test_signer();
        let collection =
            collection_id!("89b23fea4e46d40e8a4c6cca723e2a03fdd4bec2a00000000000000000000000");
        let fee = 1000u128;

        let v2_receipt = signer
            .create_receipt(
                collection,
                fee,
                address!("1111111111111111111111111111111111111111"),
                address!("2222222222222222222222222222222222222222"),
                address!("3333333333333333333333333333333333333333"),
            )
            .expect("failed to create v2 receipt");

        // Simulate IndexerQueryProtobuf creation (what gateway sends)
        let (allocation_field, collection_field) = if v2_receipt.is_v1() {
            (Some(v2_receipt.allocation().as_ref().to_vec()), None)
        } else {
            (None, Some(v2_receipt.collection().as_ref().to_vec()))
        };

        // Verify format matches tap-aggregator v2 expectations
        assert!(
            allocation_field.is_none(),
            "v2 receipt must not have allocation field"
        );
        assert!(
            collection_field.is_some(),
            "v2 receipt must have collection field"
        );

        let collection_bytes = collection_field.unwrap();
        assert_eq!(collection_bytes.len(), 32, "collection should be 32 bytes");
        assert_eq!(collection_bytes, collection.as_ref().to_vec());

        // Also verify v2-specific fields are available
        assert!(v2_receipt.payer().is_some(), "v2 receipt should have payer");
        assert!(
            v2_receipt.data_service().is_some(),
            "v2 receipt should have data_service"
        );
        assert!(
            v2_receipt.service_provider().is_some(),
            "v2 receipt should have service_provider"
        );
    }
}
