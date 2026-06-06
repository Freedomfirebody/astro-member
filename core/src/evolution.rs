use crate::models::MemoryLayer;

#[derive(Debug, Clone, Copy)]
pub struct EvolutionConfig {
    pub success_multiplier: f64,
    pub failure_multiplier: f64,
    pub min_score: f64,
    pub max_score: f64,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            success_multiplier: 1.1,
            failure_multiplier: 0.8,
            min_score: 0.1,
            max_score: 5.0,
        }
    }
}

pub struct EvolutionEngine {
    config: EvolutionConfig,
}

impl EvolutionEngine {
    pub fn new(config: EvolutionConfig) -> Self {
        Self { config }
    }

    pub fn evaluate(&self, current_score: f64, success: bool, layer: &MemoryLayer) -> f64 {
        if *layer != MemoryLayer::Experience {
            return current_score;
        }

        // Handle NaN/Infinity input safety
        if !current_score.is_finite() {
            return self.config.min_score; // return safe fallback
        }

        let multiplier = if success {
            self.config.success_multiplier
        } else {
            self.config.failure_multiplier
        };

        let new_score = current_score * multiplier;
        new_score.clamp(self.config.min_score, self.config.max_score)
    }
}
