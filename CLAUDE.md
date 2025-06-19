# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is the Graph Network Gateway - a Rust-based service that manages relationships between data consumers and indexers in The Graph Network. The gateway routes GraphQL queries to indexers and handles indexer payments via the TAP (Timeline Aggregation Protocol).

## Development Commands

```bash
# Build and check
cargo build                    # Compile the project  
cargo check                    # Fast compilation check
cargo build --release          # Production build with optimizations

# Code quality
cargo clippy                   # Lint (warnings treated as errors)
cargo fmt                      # Format code (uses rustfmt.toml config)

# Testing
cargo test                     # Run all unit and integration tests
cargo test <test_name>         # Run specific test

# Running
cargo run <config_file.json>   # Run gateway with config file
```

## Architecture Overview

The gateway is built on Axum web framework with Tokio async runtime. Key architectural components:

- **Authentication** (`auth.rs`, `auth/`): API key validation, Studio API integration, Kafka-based auth
- **Query Handling** (`client_query.rs`, `client_query/`): Client request processing and context management  
- **Network Layer** (`network.rs`, `network/`): Indexer discovery, subgraph management, cost models
- **Indexer Communication** (`indexer_client.rs`): HTTP client for indexer-service interactions
- **Payment System** (`receipts.rs`, `budgets.rs`): TAP receipts and fee management
- **Data Export** (`kafka.rs`): Analytics export to gateway_queries and gateway_attestations topics
- **Middleware** (`middleware/`): HTTP middleware for auth, tracing, and request processing

## Configuration

All configuration is done via a single JSON file passed as the first argument:
```bash
graph-gateway path/to/config.json
```

Configuration structure is defined in `src/config.rs` (`graph_gateway::config::Config`).

Use `RUST_LOG` environment variable for log filtering:
```bash
RUST_LOG="info,graph_gateway=debug" cargo run config.json
```

## Testing Strategy

- Unit tests are embedded in source files using `#[cfg(test)]` modules
- Integration tests use `tokio-test`, `tower-test`, and `assert_matches` 
- Test HTTP services with `http-body-util` and `hyper`
- CI enforces formatting (`rustfmt`) and linting (`clippy -Dwarnings`)

## Key Dependencies

- **Axum 0.8.1**: HTTP server framework
- **Tokio**: Async runtime
- **GraphQL tooling**: Custom GraphQL parsing and execution
- **indexer-selection**: Candidate selection algorithm for indexer routing
- **tap_graph**: TAP protocol implementation
- **rdkafka**: Kafka client for data export
- **prometheus**: Metrics collection

## Code Formatting Rules

The project uses rustfmt with custom configuration (`rustfmt.toml`):
- Nightly features enabled
- Crate-level import granularity  
- Standard/External/Crate import grouping
- Module reordering enabled

## Monitoring and Observability

- **Metrics**: Prometheus metrics served at `:${METRICS_PORT}/metrics` (defined in `src/metrics.rs`)
- **Tracing**: Distributed tracing with span labels `client_request` and `indexer_request`
- **Error Handling**: All client errors defined in `src/errors.rs` (`gateway_framework::errors::Error`)

## Payment Integration

The gateway acts as a TAP sender, requiring coordination with:
- `tap-aggregator`: Public endpoint for receipt aggregation into RAVs
- `tap-escrow-manager`: Maintains escrow balances for payments

Two wallet types required:
- **Sender wallet**: ETH for gas, GRT for escrow allocation  
- **Authorized signer**: Signs receipts and RAVs