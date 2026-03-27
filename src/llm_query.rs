use std::path::PathBuf;
use std::sync::Arc;

use crate::executors::types::{LlmExecutor, Usage};
use consult_llm_core::llm_cost::calculate_cost;

pub struct QueryResult {
    pub response: String,
    pub cost_info: String,
    pub thread_id: Option<String>,
    pub usage: Option<Usage>,
}

pub async fn query_llm(
    prompt: &str,
    model: &str,
    executor: &Arc<dyn LlmExecutor>,
    file_paths: Option<&[PathBuf]>,
    thread_id: Option<&str>,
    system_prompt: &str,
    consultation_id: Option<&str>,
) -> anyhow::Result<QueryResult> {
    let result = executor
        .execute(
            prompt,
            model,
            system_prompt,
            file_paths,
            thread_id,
            consultation_id,
        )
        .await?;

    if result.response.is_empty() {
        anyhow::bail!("No response from the model");
    }

    let cost_info = match &result.usage {
        Some(usage) => {
            let cost = calculate_cost(usage.prompt_tokens, usage.completion_tokens, model);
            format!(
                "Tokens: {} input, {} output | Cost: ${:.6} (input: ${:.6}, output: ${:.6})",
                usage.prompt_tokens,
                usage.completion_tokens,
                cost.total_cost,
                cost.input_cost,
                cost.output_cost
            )
        }
        None => "Cost data not available (using CLI mode)".to_string(),
    };

    Ok(QueryResult {
        response: result.response,
        cost_info,
        thread_id: result.thread_id,
        usage: result.usage,
    })
}
