struct ModelPricing {
    input_per_million: f64,
    output_per_million: f64,
}

pub struct CostResult {
    pub input_cost: f64,
    pub output_cost: f64,
    pub total_cost: f64,
}

fn get_pricing(model: &str) -> Option<ModelPricing> {
    Some(match model {
        "gpt-5.4" => ModelPricing {
            input_per_million: 2.5,
            output_per_million: 15.0,
        },
        "gpt-5.3-codex" => ModelPricing {
            input_per_million: 2.5,
            output_per_million: 10.0,
        },
        "gpt-5.2" => ModelPricing {
            input_per_million: 1.75,
            output_per_million: 14.0,
        },
        "gpt-5.2-codex" => ModelPricing {
            input_per_million: 1.75,
            output_per_million: 7.0,
        },
        "gemini-2.5-pro" => ModelPricing {
            input_per_million: 1.25,
            output_per_million: 10.0,
        },
        "gemini-3-pro-preview" | "gemini-3.1-pro-preview" => ModelPricing {
            input_per_million: 2.0,
            output_per_million: 12.0,
        },
        "deepseek-reasoner" => ModelPricing {
            input_per_million: 0.55,
            output_per_million: 2.19,
        },
        "MiniMax-M2.7" => ModelPricing {
            input_per_million: 0.30,
            output_per_million: 1.20,
        },
        _ => return None,
    })
}

pub fn calculate_cost(prompt_tokens: u64, completion_tokens: u64, model: &str) -> CostResult {
    match get_pricing(model) {
        Some(p) => {
            let input_cost = (prompt_tokens as f64 / 1_000_000.0) * p.input_per_million;
            let output_cost = (completion_tokens as f64 / 1_000_000.0) * p.output_per_million;
            CostResult {
                input_cost,
                output_cost,
                total_cost: input_cost + output_cost,
            }
        }
        None => CostResult {
            input_cost: 0.0,
            output_cost: 0.0,
            total_cost: 0.0,
        },
    }
}
