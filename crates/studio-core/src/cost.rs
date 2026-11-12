//! Models and per-image prices, loaded from `library/pricing.toml`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pricing {
    /// Date the prices were last read from Google's pricing page.
    pub checked_on: String,
    pub default_model: String,
    pub default_size: String,
    #[serde(rename = "model")]
    pub models: Vec<ModelPricing>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPricing {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub note: String,
    /// US dollars per generated image, keyed by image size ("1K", "2K", "4K").
    pub per_image: BTreeMap<String, f64>,
    /// Same, through the half-price batch service, where Google offers it.
    #[serde(default)]
    pub batch_per_image: BTreeMap<String, f64>,
}

impl Pricing {
    pub fn model(&self, id: &str) -> Option<&ModelPricing> {
        self.models.iter().find(|model| model.id == id)
    }

    /// Price of one generated image, if this model and size are on file.
    pub fn per_image(&self, model: &str, size: &str) -> Option<f64> {
        self.model(model)?.per_image.get(size).copied()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.models.is_empty() {
            return Err("at least one [[model]] is required".into());
        }
        for model in &self.models {
            if model.per_image.is_empty() {
                return Err(format!("model \"{}\" has no prices", model.id));
            }
            let prices = model.per_image.iter().chain(&model.batch_per_image);
            for (size, price) in prices {
                if !price.is_finite() || *price <= 0.0 {
                    return Err(format!(
                        "model \"{}\" has an invalid price for {size}",
                        model.id
                    ));
                }
            }
        }
        if self
            .per_image(&self.default_model, &self.default_size)
            .is_none()
        {
            return Err(format!(
                "default_model \"{}\" has no price for default_size \"{}\"",
                self.default_model, self.default_size
            ));
        }
        Ok(())
    }
}

/// Formats a dollar amount. Amounts under $10 keep three decimals because
/// single images cost a few cents.
pub fn format_usd(amount: f64) -> String {
    if amount < 10.0 {
        format!("${amount:.3}")
    } else {
        format!("${amount:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pricing() -> Pricing {
        toml::from_str(
            r#"
            checked_on = "2026-11-12"
            default_model = "fast"
            default_size = "2K"

            [[model]]
            id = "fast"
            label = "Fast"
            [model.per_image]
            "1K" = 0.03
            "2K" = 0.05
            "#,
        )
        .unwrap()
    }

    #[test]
    fn looks_up_price_by_model_and_size() {
        let pricing = pricing();
        assert_eq!(pricing.per_image("fast", "2K"), Some(0.05));
        assert_eq!(pricing.per_image("fast", "4K"), None);
        assert_eq!(pricing.per_image("unknown", "2K"), None);
        assert!(pricing.validate().is_ok());
    }

    #[test]
    fn default_must_be_priced() {
        let mut pricing = pricing();
        pricing.default_size = "4K".into();
        assert!(pricing.validate().is_err());
    }

    #[test]
    fn formats_small_and_large_amounts() {
        assert_eq!(format_usd(0.0504), "$0.050");
        assert_eq!(format_usd(1.2096), "$1.210");
        assert_eq!(format_usd(25.5), "$25.50");
    }
}
