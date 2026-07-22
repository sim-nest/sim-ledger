//! Data-backed bookkeeping profiles.

use serde::{Deserialize, Serialize};

use crate::BooksError;

/// Tax and VAT profile loaded from data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxProfile {
    /// Stable profile id.
    pub id: String,
    /// VAT rates available to tests or local hosts.
    #[serde(default)]
    pub vat_rates: Vec<VatRate>,
}

/// One named VAT rate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VatRate {
    /// Stable rate code.
    pub code: String,
    /// Decimal percent kept as data text, not a hardwired float.
    pub percent: String,
}

/// Parse a tax profile from TOML text.
pub fn load_profile_str(text: &str) -> Result<TaxProfile, BooksError> {
    toml::from_str(text).map_err(|error| BooksError::Profile(error.to_string()))
}
