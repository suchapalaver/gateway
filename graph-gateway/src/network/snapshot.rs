//! Entities that are used to represent the network topology.

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    ops::Deref,
    sync::{Arc, OnceLock},
};

pub use alloy_primitives::{Address, BlockNumber};
use cost_model::CostModel;
use custom_debug::CustomDebug;
use eventuals::Ptr;
use semver::Version;
pub use thegraph_core::types::{DeploymentId, SubgraphId};
use url::Url;

use super::internal::types::{DeploymentInfo, IndexerInfo, SubgraphInfo};

/// The minimum indexer agent version required to support Scalar TAP.
fn min_required_indexer_agent_version_scalar_tap_support() -> &'static Version {
    static VERSION: OnceLock<Version> = OnceLock::new();
    VERSION.get_or_init(|| "1.0.0-alpha".parse().expect("valid version"))
}

/// The [`IndexingId`] struct represents the unique identifier of an indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct IndexingId {
    /// The indexer's ID indexing the subgraph's deployment.
    pub indexer: Address,
    /// The subgraph's deployment ID indexed by the indexer.
    pub deployment: DeploymentId,
}

#[derive(Clone)]
pub struct Indexing {
    /// The indexing unique identifier.
    pub id: IndexingId,

    /// The versions behind the highest version of the subgraph being indexed.
    pub versions_behind: u8,

    /// The largest allocation address.
    ///
    /// This is, among all allocations associated with the indexer and deployment, the address
    /// with the largest amount of allocated tokens.
    pub largest_allocation: Address,
    /// The indexer's indexing total allocated tokens.
    ///
    /// This is, the sum of all allocated tokens associated with the indexer and deployment.
    pub total_allocated_tokens: u128,

    /// The indexer
    pub indexer: Arc<Indexer>,

    /// The indexer's indexing status
    pub status: Option<IndexingStatus>,
    /// The indexer's indexing cost model
    pub cost_model: Option<Ptr<CostModel>>,
}

/// The [`IndexingStatus`] struct represents the indexer's indexing status.
#[derive(Debug, Clone)]
pub struct IndexingStatus {
    /// The latest block the indexer has indexed for the deployment.
    pub latest_block: BlockNumber,
    /// The minimum block the indexer has indexed for the deployment.
    pub min_block: Option<BlockNumber>,
}

/// The [`Indexer`] struct represents an indexer in the network topology.
///
/// The indexer is a network  node that indexes a subgraph using one of its versions, i.e., one of
/// the subgraph's deployments. The [`Indexing`] struct represents the indexer's indexing of a
/// subgraph's deployment.
#[derive(CustomDebug, Clone)]
pub struct Indexer {
    /// The indexer's ID.
    pub id: Address,

    /// The indexer's URL.
    ///
    /// It is guaranteed that the URL scheme is either HTTP or HTTPS and the URL has a host.
    #[debug(with = Display::fmt)]
    pub url: Url,

    /// The indexer's "indexer service" version.
    pub indexer_agent_version: Version,
    /// The indexer's "graph node" version.
    pub graph_node_version: Version,

    /// Whether the indexer supports using Scalar TAP.
    pub scalar_tap_support: bool,

    /// The indexer's indexings set.
    ///
    /// It is a set of deployment IDs that the indexer is indexing.
    pub indexings: HashSet<DeploymentId>,

    /// The indexer's staked tokens.
    pub staked_tokens: u128,
}

#[derive(Clone)]
pub struct Subgraph {
    /// Subgraph ID
    pub id: SubgraphId,

    /// The subgraph's chain name.
    ///
    /// This information is extracted from the highest version of the subgraph deployment's
    /// manifest.
    pub chain: String,
    /// The subgraph's start block number.
    ///
    /// This information is extracted from the highest version of the subgraph deployment's
    /// manifest.
    pub start_block: BlockNumber,

    /// The subgraph's deployments.
    ///
    /// A list of deployment IDs known to be healthy and currently serving queries.
    pub deployments: HashSet<DeploymentId>,

    /// The subgraph's indexings.
    ///
    /// A table holding all the known healthy indexings for the subgraph.
    pub indexings: HashMap<IndexingId, Indexing>,
}

#[derive(Clone)]
pub struct Deployment {
    /// Deployment ID.
    ///
    /// The IPFS content ID of the subgraph manifest.
    pub id: DeploymentId,

    /// The deployment chain name.
    ///
    /// This field is extracted from the deployment manifest.
    pub chain: String,
    /// The deployment start block number.
    ///
    /// This field is extracted from the deployment manifest.
    pub start_block: BlockNumber,

    /// A deployment may be associated with multiple subgraphs.
    pub subgraphs: HashSet<SubgraphId>,

    /// The deployment's indexings.
    ///
    /// A table holding all the known healthy indexings for the deployment.
    pub indexings: HashMap<IndexingId, Indexing>,
}

/// A snapshot of the network topology.
pub struct NetworkTopologySnapshot {
    /// Table holding the subgraph ID of the transferred subgraphs and the L2 subgraph ID.
    transferred_subgraphs: HashMap<SubgraphId, SubgraphId>,
    /// Table holding the deployment ID of the transferred deployments.
    transferred_deployments: HashSet<DeploymentId>,

    /// Subgraphs network topology table.
    subgraphs: HashMap<SubgraphId, Subgraph>,
    /// Deployments network topology table.
    deployments: HashMap<DeploymentId, Deployment>,
}

impl NetworkTopologySnapshot {
    /// Get the [`Subgraph`] by [`SubgraphId`].
    ///
    /// If the subgraph is not found, it returns `None`.
    pub fn get_subgraph_by_id(&self, id: &SubgraphId) -> Option<&Subgraph> {
        self.subgraphs.get(id)
    }

    /// Get the [`Deployment`] by [`DeploymentId`].
    ///
    /// If the deployment is not found, it returns `None`.
    pub fn get_deployment_by_id(&self, id: &DeploymentId) -> Option<&Deployment> {
        self.deployments.get(id)
    }

    /// Get the snapshot subgraphs.
    pub fn subgraphs(&self) -> impl Deref<Target = HashMap<SubgraphId, Subgraph>> + '_ {
        &self.subgraphs
    }

    /// Get the snapshot deployments.
    pub fn deployments(&self) -> impl Deref<Target = HashMap<DeploymentId, Deployment>> + '_ {
        &self.deployments
    }

    /// Get the snapshot transferred subgraphs.
    pub fn transferred_subgraphs(
        &self,
    ) -> impl Deref<Target = HashMap<SubgraphId, SubgraphId>> + '_ {
        &self.transferred_subgraphs
    }

    /// Get the snapshot transferred deployments.
    pub fn transferred_deployments(&self) -> impl Deref<Target = HashSet<DeploymentId>> + '_ {
        &self.transferred_deployments
    }
}

/// Construct the [`NetworkTopologySnapshot`] from the indexers and subgraphs information.
pub fn new_from(
    indexers_info: HashMap<Address, IndexerInfo>,
    subgraphs_info: HashMap<SubgraphId, SubgraphInfo>,
) -> NetworkTopologySnapshot {
    // Construct the deployments table
    let deployments_info = subgraphs_info
        .values()
        .flat_map(|subgraph| {
            subgraph
                .versions
                .iter()
                .map(|version| (version.deployment.id, version.deployment.clone()))
        })
        .collect::<HashMap<_, _>>();

    // Construct the indexers table
    let indexers = indexers_info
        .iter()
        .map(|(indexer_id, indexer)| {
            // The indexer agent version must be greater than or equal to the minimum required
            // version to support Scalar TAP.
            let indexer_scalar_tap_support = indexer.indexer_agent_version
                >= *min_required_indexer_agent_version_scalar_tap_support();

            (
                indexer_id,
                Arc::new(Indexer {
                    id: indexer.id,
                    url: indexer.url.clone(),
                    indexer_agent_version: indexer.indexer_agent_version.clone(),
                    graph_node_version: indexer.graph_node_version.clone(),
                    scalar_tap_support: indexer_scalar_tap_support,
                    indexings: indexer.deployments.iter().copied().collect(),
                    staked_tokens: indexer.staked_tokens,
                }),
            )
        })
        .collect::<HashMap<_, _>>();

    // Construct the transferred subgraphs and deployments tables
    let transferred_subgraphs = construct_transferred_subgraphs_table(&subgraphs_info);
    let transferred_deployments = construct_transferred_deployments_table(&deployments_info);

    // Construct the subgraphs table
    let subgraphs = subgraphs_info
        .into_iter()
        .filter_map(|(subgraph_id, subgraph)| {
            // If the subgraph is transferred to L2, exclude it
            if transferred_subgraphs.contains_key(&subgraph_id) {
                return None;
            }

            // Filter-out the subgraphs' invalid versions-deployments.
            let versions = subgraph
                .versions
                .into_iter()
                .filter(|version| {
                    // Valid version must have a deployment with:
                    // - Valid manifest info (i.e., network).
                    // - Not marked as transferred to L2.
                    version.deployment.manifest_network.is_some()
                        && !transferred_deployments.contains(&version.deployment.id)
                })
                .collect::<Vec<_>>();

            // If all the subgraph's versions are invalid, exclude the subgraph.
            if versions.is_empty() {
                return None;
            }

            // As versions are ordered in descending order, the first version is the highest
            let highest_version = versions.first()?;

            let highest_version_number = highest_version.version;
            let highest_version_deployment_manifest_chain = highest_version
                .deployment
                .manifest_network
                .as_ref()?
                .clone();
            let highest_version_deployment_manifest_start_block =
                highest_version.deployment.manifest_start_block.unwrap_or(0);

            let versions_behind_table = versions
                .iter()
                .map(|version| {
                    let deployment_id = version.deployment.id;
                    let deployment_versions_behind = highest_version_number
                        .saturating_sub(version.version)
                        .try_into()
                        .unwrap_or(u8::MAX);
                    (deployment_id, deployment_versions_behind)
                })
                .collect::<HashMap<_, _>>();

            let subgraph_indexings = versions
                .into_iter()
                .flat_map(|version| {
                    let deployment_id = version.deployment.id;
                    let indexing_deployment_versions_behind = versions_behind_table
                        .get(&deployment_id)
                        .copied()
                        .unwrap_or(u8::MAX);

                    version
                        .deployment
                        .allocations
                        .into_iter()
                        .filter_map(|alloc| {
                            // If the indexer is not in the indexers table, exclude it. It might
                            // have been filtered out due to different reasons, e.g., invalid info.
                            let indexing_indexer_id = alloc.indexer;
                            let indexing_indexer_info = indexers_info.get(&indexing_indexer_id)?;

                            // The indexer deployments list contains the healthy deployments. It
                            // must contain the deployment ID, otherwise, that means it was filtered
                            // out, e.g., invalid POI blocklist, etc.
                            if !indexing_indexer_info.deployments.contains(&deployment_id) {
                                return None;
                            }

                            let indexing_indexer = indexers.get(&indexing_indexer_id)?;

                            // If the indexing has no allocations, exclude it
                            let indexing_largest_allocation_addr = indexing_indexer_info
                                .largest_allocation
                                .get(&deployment_id)?;

                            // If the indexing has no total allocated tokens, exclude it
                            let indexing_total_allocated_tokens = indexing_indexer_info
                                .total_allocated_tokens
                                .get(&deployment_id)?;

                            let indexing_status = indexing_indexer_info
                                .indexings_progress
                                .get(&deployment_id)
                                .map(|status| IndexingStatus {
                                    latest_block: status.latest_block,
                                    min_block: status.min_block,
                                });

                            let indexing_cost_model = indexing_indexer_info
                                .indexings_cost_model
                                .get(&deployment_id)
                                .cloned();

                            let indexing_id = IndexingId {
                                indexer: indexing_indexer_id,
                                deployment: deployment_id,
                            };
                            let indexing = Indexing {
                                id: indexing_id,
                                versions_behind: indexing_deployment_versions_behind,
                                largest_allocation: *indexing_largest_allocation_addr,
                                total_allocated_tokens: *indexing_total_allocated_tokens,
                                indexer: indexing_indexer.clone(),
                                status: indexing_status,
                                cost_model: indexing_cost_model,
                            };
                            Some((indexing_id, indexing))
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<HashMap<_, _>>();
            if subgraph_indexings.is_empty() {
                return None;
            }

            let subgraph_deployments = subgraph_indexings
                .keys()
                .map(|indexing_id| indexing_id.deployment)
                .collect::<HashSet<_>>();
            if subgraph_deployments.is_empty() {
                return None;
            }

            Some((
                subgraph_id,
                Subgraph {
                    id: subgraph.id,
                    chain: highest_version_deployment_manifest_chain,
                    start_block: highest_version_deployment_manifest_start_block,
                    deployments: subgraph_deployments,
                    indexings: subgraph_indexings,
                },
            ))
        })
        .collect::<HashMap<_, _>>();

    // Construct the deployments table
    let deployments = deployments_info
        .into_iter()
        .filter_map(|(deployment_id, deployment)| {
            // If the deployment is transferred to L2, exclude it
            if transferred_deployments.contains(&deployment_id) {
                return None;
            }

            let deployment_versions_behind = 0;
            let deployment_manifest_chain = deployment.manifest_network?.clone();
            let deployment_manifest_start_block = deployment.manifest_start_block?;

            let deployment_indexings = deployment
                .allocations
                .into_iter()
                .filter_map(|alloc| {
                    // If the indexer is not in the indexers table, exclude it. It might
                    // have been filtered out due to different reasons, e.g., invalid info.
                    let indexing_indexer_id = alloc.indexer;
                    let indexing_indexer_info = indexers_info.get(&indexing_indexer_id)?;

                    // The indexer deployments list contains the healthy deployments. It must
                    // contain the deployment ID, otherwise, that means it was filtered out,
                    // e.g., invalid POI blocklist, etc.
                    if !indexing_indexer_info.deployments.contains(&deployment_id) {
                        return None;
                    }

                    let indexing_indexer = indexers.get(&indexing_indexer_id)?;

                    let indexing_largest_allocation_addr = indexing_indexer_info
                        .largest_allocation
                        .get(&deployment_id)?;

                    let indexing_total_allocated_tokens = indexing_indexer_info
                        .total_allocated_tokens
                        .get(&deployment_id)?;

                    let indexing_status = indexing_indexer_info
                        .indexings_progress
                        .get(&deployment_id)
                        .map(|status| IndexingStatus {
                            latest_block: status.latest_block,
                            min_block: status.min_block,
                        });

                    let indexing_cost_model = indexing_indexer_info
                        .indexings_cost_model
                        .get(&deployment_id)
                        .cloned();

                    let indexing_id = IndexingId {
                        indexer: indexing_indexer_id,
                        deployment: deployment_id,
                    };
                    let indexing = Indexing {
                        id: indexing_id,
                        versions_behind: deployment_versions_behind,
                        largest_allocation: *indexing_largest_allocation_addr,
                        total_allocated_tokens: *indexing_total_allocated_tokens,
                        indexer: indexing_indexer.clone(),
                        status: indexing_status,
                        cost_model: indexing_cost_model,
                    };
                    Some((indexing_id, indexing))
                })
                .collect::<HashMap<_, _>>();
            if deployment_indexings.is_empty() {
                return None;
            }

            let deployment_subgraphs = subgraphs
                .iter()
                .filter_map(|(subgraph_id, subgraph)| {
                    if subgraph.deployments.contains(&deployment_id) {
                        Some(*subgraph_id)
                    } else {
                        None
                    }
                })
                .collect::<HashSet<_>>();
            if deployment_subgraphs.is_empty() {
                return None;
            }

            Some((
                deployment_id,
                Deployment {
                    id: deployment_id,
                    chain: deployment_manifest_chain,
                    start_block: deployment_manifest_start_block,
                    subgraphs: deployment_subgraphs,
                    indexings: deployment_indexings,
                },
            ))
        })
        .collect();

    NetworkTopologySnapshot {
        transferred_subgraphs,
        transferred_deployments,
        deployments,
        subgraphs,
    }
}

/// Extracts from the subgraphs info table the subgraph IDs that:
/// - All its versions-deployments are marked as transferred to L2.
/// - All its versions-deployments have no allocations.
fn construct_transferred_subgraphs_table(
    subgraphs_info: &HashMap<SubgraphId, SubgraphInfo>,
) -> HashMap<SubgraphId, SubgraphId> {
    subgraphs_info
        .iter()
        .filter_map(|(subgraph_id, subgraph)| {
            // A subgraph is considered to be transferred to L2 if all its versions-deployments
            // are transferred to L2 (i.e., `transferred_to_l2` is `true`) and have no allocations.
            let transferred_to_l2 = subgraph.versions.iter().all(|version| {
                version.deployment.transferred_to_l2 && version.deployment.allocations.is_empty()
            });

            // If the subgraph is transferred to L2 and has an ID on L2, return the pair.
            // Otherwise, exclude the subgraph.
            if transferred_to_l2 && subgraph.id_on_l2.is_some() {
                Some((*subgraph_id, subgraph.id_on_l2?))
            } else {
                None
            }
        })
        .collect::<HashMap<_, _>>()
}

/// Extracts from the deployments info table the deployment IDs that:
///  - Are marked as transferred to L2.
///  - Have no associated allocations.
fn construct_transferred_deployments_table(
    deployments_info: &HashMap<DeploymentId, DeploymentInfo>,
) -> HashSet<DeploymentId> {
    deployments_info
        .iter()
        .filter_map(|(deployment_id, deployment)| {
            if deployment.transferred_to_l2 && deployment.allocations.is_empty() {
                Some(*deployment_id)
            } else {
                None
            }
        })
        .collect::<HashSet<_>>()
}
