pub mod subgraph;

use std::str::FromStr;

use alloy_primitives::Address;
use chrono::{DateTime, Utc};
use serde::{de::Error, Deserialize, Deserializer};

#[derive(Clone, Debug)]
pub struct Subscription {
    pub signers: Vec<Address>,
    pub rate: u128,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: Address,
    #[serde(default)]
    pub authorized_signers: Vec<AuthorizedSigner>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AuthorizedSigner {
    pub signer: Address,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ActiveSubscription {
    pub user: User,
    #[serde(deserialize_with = "deserialize_datetime_utc")]
    pub start: DateTime<Utc>,
    #[serde(deserialize_with = "deserialize_datetime_utc")]
    pub end: DateTime<Utc>,
    #[serde(deserialize_with = "deserialize_u128")]
    pub rate: u128,
}

fn deserialize_datetime_utc<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let input = String::deserialize(deserializer)?;
    let timestamp = input.parse::<i64>().map_err(D::Error::custom)?;
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .ok_or_else(|| D::Error::custom("invalid timestamp"))
}

fn deserialize_u128<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    let input = String::deserialize(deserializer)?;
    u128::from_str(&input).map_err(D::Error::custom)
}

#[cfg(test)]
mod tests {
    use anyhow::ensure;
    use serde_json::json;

    use super::*;

    #[test]
    fn should_parse_active_subscriptions_query() -> anyhow::Result<()> {
        let result = serde_json::from_str::<ActiveSubscription>(
            &json!({
                "user": {
                    "id": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
                    "authorizedSigners": [
                        {
                            "signer": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
                        }
                    ],
                },
                "start": "1676507163",
                "end": "1676507701",
                "rate": "100000000000000",
            })
            .to_string(),
        );
        ensure!(result.is_ok(), "failed to parse example: {:?}", result);
        Ok(())
    }
}
