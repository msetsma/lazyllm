/// Model pricing registry for cost calculation.
///
/// Prices are per million tokens. Built-in defaults cover common models.
/// Users can override via config (Phase 4).

/// Per-million-token pricing for a model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPricing {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_read_per_million: f64,
    pub cache_write_per_million: f64,
}

impl ModelPricing {
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input_per_million: input,
            output_per_million: output,
            cache_read_per_million: 0.0,
            cache_write_per_million: 0.0,
        }
    }

    pub fn with_cache(mut self, read: f64, write: f64) -> Self {
        self.cache_read_per_million = read;
        self.cache_write_per_million = write;
        self
    }

    /// Calculate cost in USD for given token counts.
    pub fn calculate_cost(
        &self,
        input_tokens: u32,
        output_tokens: u32,
        cache_read_tokens: u32,
        cache_creation_tokens: u32,
    ) -> f64 {
        let input = input_tokens as f64 * self.input_per_million / 1_000_000.0;
        let output = output_tokens as f64 * self.output_per_million / 1_000_000.0;
        let cache_read = cache_read_tokens as f64 * self.cache_read_per_million / 1_000_000.0;
        let cache_write = cache_creation_tokens as f64 * self.cache_write_per_million / 1_000_000.0;
        input + output + cache_read + cache_write
    }
}

/// Look up pricing for a model by its ID.
///
/// Returns `None` for unknown models (e.g. Ollama local models).
pub fn get_pricing(model: &str) -> Option<ModelPricing> {
    // Normalize: strip date suffixes like "claude-sonnet-4-20250514" -> match on prefix
    let model_lower = model.to_lowercase();

    // OpenAI models
    if model_lower.starts_with("gpt-4o-mini") {
        return Some(ModelPricing::new(0.15, 0.60));
    }
    if model_lower.starts_with("gpt-4o") {
        return Some(ModelPricing::new(2.50, 10.00));
    }
    if model_lower.starts_with("gpt-4-turbo") {
        return Some(ModelPricing::new(10.00, 30.00));
    }
    if model_lower.starts_with("gpt-4") {
        return Some(ModelPricing::new(30.00, 60.00));
    }
    if model_lower.starts_with("gpt-3.5-turbo") {
        return Some(ModelPricing::new(0.50, 1.50));
    }
    if model_lower.starts_with("o1-mini") {
        return Some(ModelPricing::new(3.00, 12.00));
    }
    if model_lower.starts_with("o1-preview") || model_lower.starts_with("o1") {
        return Some(ModelPricing::new(15.00, 60.00));
    }

    // Anthropic models
    if model_lower.contains("claude-opus") || model_lower.contains("claude-4-opus") {
        return Some(ModelPricing::new(15.00, 75.00).with_cache(1.50, 18.75));
    }
    if model_lower.contains("claude-sonnet") || model_lower.contains("claude-4-sonnet") {
        return Some(ModelPricing::new(3.00, 15.00).with_cache(0.30, 3.75));
    }
    if model_lower.contains("claude-haiku") || model_lower.contains("claude-3-haiku") {
        return Some(ModelPricing::new(0.25, 1.25).with_cache(0.03, 0.30));
    }

    // Google Gemini models
    if model_lower.starts_with("gemini-2.0-flash") || model_lower.starts_with("gemini-2.5-flash") {
        return Some(ModelPricing::new(0.075, 0.30));
    }
    if model_lower.starts_with("gemini-2.5-pro") || model_lower.starts_with("gemini-2.0-pro") {
        return Some(ModelPricing::new(1.25, 10.00));
    }
    if model_lower.starts_with("gemini-1.5-pro") {
        return Some(ModelPricing::new(1.25, 5.00));
    }
    if model_lower.starts_with("gemini-1.5-flash") {
        return Some(ModelPricing::new(0.075, 0.30));
    }

    // Ollama/local models: no pricing
    None
}

/// Format a cost as a human-readable string.
pub fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        format!("${:.4}", cost)
    } else {
        format!("${:.2}", cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt4o_pricing() {
        let p = get_pricing("gpt-4o").unwrap();
        assert!((p.input_per_million - 2.50).abs() < f64::EPSILON);
        assert!((p.output_per_million - 10.00).abs() < f64::EPSILON);
    }

    #[test]
    fn gpt4o_mini_pricing() {
        let p = get_pricing("gpt-4o-mini").unwrap();
        assert!((p.input_per_million - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn claude_sonnet_pricing() {
        let p = get_pricing("claude-sonnet-4-20250514").unwrap();
        assert!((p.input_per_million - 3.00).abs() < f64::EPSILON);
        assert!((p.cache_read_per_million - 0.30).abs() < f64::EPSILON);
    }

    #[test]
    fn claude_opus_pricing() {
        let p = get_pricing("claude-opus-4-20250514").unwrap();
        assert!((p.input_per_million - 15.00).abs() < f64::EPSILON);
    }

    #[test]
    fn gemini_flash_pricing() {
        let p = get_pricing("gemini-2.0-flash").unwrap();
        assert!((p.input_per_million - 0.075).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(get_pricing("llama3.2:8b").is_none());
        assert!(get_pricing("my-custom-model").is_none());
    }

    #[test]
    fn cost_calculation() {
        let p = ModelPricing::new(3.00, 15.00).with_cache(0.30, 3.75);
        let cost = p.calculate_cost(1000, 500, 200, 100);
        let expected = 1000.0 * 3.0 / 1e6 + 500.0 * 15.0 / 1e6 + 200.0 * 0.3 / 1e6 + 100.0 * 3.75 / 1e6;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn cost_calculation_no_cache() {
        let p = ModelPricing::new(2.50, 10.00);
        let cost = p.calculate_cost(1000, 500, 0, 0);
        let expected = 1000.0 * 2.5 / 1e6 + 500.0 * 10.0 / 1e6;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn format_cost_small() {
        assert_eq!(format_cost(0.0035), "$0.0035");
    }

    #[test]
    fn format_cost_large() {
        assert_eq!(format_cost(1.23), "$1.23");
    }

    #[test]
    fn claude_haiku_pricing() {
        let p = get_pricing("claude-3-haiku-20240307").unwrap();
        assert!((p.input_per_million - 0.25).abs() < f64::EPSILON);
        assert!(p.cache_read_per_million > 0.0);
        assert!(p.cache_write_per_million > 0.0);
    }

    #[test]
    fn gpt35_turbo_pricing() {
        let p = get_pricing("gpt-3.5-turbo").unwrap();
        assert!((p.input_per_million - 0.50).abs() < f64::EPSILON);
    }

    #[test]
    fn o1_pricing() {
        let p = get_pricing("o1-preview").unwrap();
        assert!((p.input_per_million - 15.00).abs() < f64::EPSILON);
    }

    #[test]
    fn gemini_pro_pricing() {
        let p = get_pricing("gemini-2.5-pro").unwrap();
        assert!((p.input_per_million - 1.25).abs() < f64::EPSILON);
    }

    #[test]
    fn format_cost_zero() {
        assert_eq!(format_cost(0.0), "$0.0000");
    }

    #[test]
    fn model_pricing_new_defaults_cache_to_zero() {
        let p = ModelPricing::new(1.0, 2.0);
        assert!((p.cache_read_per_million).abs() < f64::EPSILON);
        assert!((p.cache_write_per_million).abs() < f64::EPSILON);
    }
}
