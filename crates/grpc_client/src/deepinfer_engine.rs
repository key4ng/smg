use openai_protocol::{
    chat::ChatCompletionRequest, completion::CompletionRequest, generate::GenerateRequest,
    messages::CreateMessageRequest, responses::ResponsesRequest,
};
use tonic::{transport::Channel, Request};
use tracing::{debug, warn};

use crate::{
    request_fields::{
        completion_constraint, plain_constraint, response_format_schema, ConstraintKind,
        PlainSampling,
    },
    BoxedTraceInjector,
};

// Include the generated protobuf code
#[expect(clippy::allow_attributes)]
pub mod proto {
    #![allow(clippy::all, clippy::absolute_paths, unused_qualifications)]
    tonic::include_proto!("deepinfer.grpc.engine");
}

/// Streaming `generate()` response that auto-aborts on drop. Concrete
/// alias for the generic `crate::AbortOnDropStream`.
pub type AbortOnDropStream =
    crate::AbortOnDropStream<proto::GenerateResponse, DeepInferEngineClient>;

/// gRPC client for a DeepInfer engine rank (served by
/// `smg_grpc_servicer.deepinfer`).
#[derive(Clone)]
pub struct DeepInferEngineClient {
    client: proto::deep_infer_engine_client::DeepInferEngineClient<Channel>,
    trace_injector: BoxedTraceInjector,
}

crate::impl_abort_for_drop_with_reason!(DeepInferEngineClient);

impl DeepInferEngineClient {
    crate::impl_engine_client_basics!(
        proto,
        proto::deep_infer_engine_client::DeepInferEngineClient<Channel>,
        "DeepInfer"
    );

    /// Abort a request
    pub async fn abort_request(
        &self,
        request_id: String,
        _reason: String,
    ) -> Result<(), tonic::Status> {
        debug!("Sending abort request for {}", request_id);
        let request = Request::new(proto::AbortRequest {
            request_ids: vec![request_id.clone()],
        });

        let mut client = self.client.clone();
        let _response = client.abort(request).await?;
        debug!("Abort response received for {}", request_id);
        Ok(())
    }

    crate::impl_get_tokenizer!();

    // ── Unsupported feature validation ────────────────────────────────
    //
    // DeepInfer's sampler implements temperature, top-p, top-k, a seed, top
    // logprobs and xgrammar constraints, and stops only on the model's EOS
    // (the servicer adds stop token ids). Anything else is refused here
    // rather than silently ignored. Neutral values (a penalty of 0, a
    // repetition penalty of 1, n = 1) are not a request for the feature and
    // pass.

    fn reject_n(n: Option<u32>) -> Result<(), String> {
        if n.is_some_and(|n| n > 1) {
            return Err("DeepInfer backend does not support n > 1 (parallel samples)".to_string());
        }
        Ok(())
    }

    fn reject_stop_strings(has_stop_strings: bool) -> Result<(), String> {
        if has_stop_strings {
            return Err(
                "DeepInfer backend does not support string stop sequences; use stop_token_ids"
                    .to_string(),
            );
        }
        Ok(())
    }

    fn reject_min_p(min_p: Option<f32>) -> Result<(), String> {
        if min_p.is_some_and(|v| v != 0.0) {
            return Err("DeepInfer backend does not support min_p".to_string());
        }
        Ok(())
    }

    fn reject_min_tokens(min_tokens: Option<u32>) -> Result<(), String> {
        if min_tokens.is_some_and(|v| v > 0) {
            return Err("DeepInfer backend does not support min_tokens".to_string());
        }
        Ok(())
    }

    fn reject_penalties(
        frequency: Option<f32>,
        presence: Option<f32>,
        repetition: Option<f32>,
    ) -> Result<(), String> {
        let named = [
            ("frequency_penalty", frequency.is_some_and(|v| v != 0.0)),
            ("presence_penalty", presence.is_some_and(|v| v != 0.0)),
            ("repetition_penalty", repetition.is_some_and(|v| v != 1.0)),
        ];
        if let Some((name, _)) = named.iter().find(|(_, set)| *set) {
            return Err(format!("DeepInfer backend does not support {name}"));
        }
        Ok(())
    }

    fn reject_logit_bias(has_logit_bias: bool) -> Result<(), String> {
        if has_logit_bias {
            return Err("DeepInfer backend does not support logit_bias".to_string());
        }
        Ok(())
    }

    // ── Public request builders ────────────────────────────────────────

    /// Build a GenerateRequest from OpenAI ChatCompletionRequest
    #[expect(
        clippy::unused_self,
        reason = "method receiver kept for consistent public API across gRPC backends"
    )]
    #[expect(
        deprecated,
        reason = "ChatCompletionRequest.seed is marked Legacy by openai-protocol but still forwarded"
    )]
    pub fn build_generate_request_from_chat(
        &self,
        request_id: String,
        body: &ChatCompletionRequest,
        processed_text: String,
        token_ids: Vec<u32>,
        tool_call_constraint: Option<(String, String)>,
    ) -> Result<proto::GenerateRequest, String> {
        Self::reject_n(body.n)?;
        Self::reject_stop_strings(body.stop.as_ref().is_some_and(|s| !s.is_empty()))?;
        Self::reject_min_p(body.min_p)?;
        Self::reject_min_tokens(body.min_tokens)?;
        Self::reject_penalties(
            body.frequency_penalty,
            body.presence_penalty,
            body.repetition_penalty,
        )?;
        Self::reject_logit_bias(body.logit_bias.as_ref().is_some_and(|b| !b.is_empty()))?;

        let logprobs = if body.logprobs {
            Some(body.top_logprobs.unwrap_or(1).min(20) as i32)
        } else {
            None
        };

        let sampling_params = proto::SamplingParams {
            temperature: body.temperature,
            top_p: body.top_p.unwrap_or(1.0),
            top_k: body.top_k.map(|v| v.max(0) as u32).unwrap_or(0),
            max_tokens: body.output_token_cap(),
            stop_token_ids: body.stop_token_ids.clone().unwrap_or_default(),
            ignore_eos: body.ignore_eos,
            logprobs,
            seed: seed_of(body.sampling_seed, body.seed),
            constraint: Self::build_constraint_for_chat(body, tool_call_constraint)?,
        };
        Ok(Self::make_generate_request(
            request_id,
            processed_text,
            token_ids,
            sampling_params,
            body.stream,
        ))
    }

    /// Build a GenerateRequest from CompletionRequest (`/v1/completions`)
    #[expect(
        clippy::unused_self,
        reason = "method receiver kept for consistent public API"
    )]
    pub fn build_generate_request_from_completion(
        &self,
        request_id: String,
        body: &CompletionRequest,
        original_text: String,
        token_ids: Vec<u32>,
    ) -> Result<proto::GenerateRequest, String> {
        Self::reject_n(body.n)?;
        Self::reject_n(body.best_of)?;
        Self::reject_stop_strings(body.stop.as_ref().is_some_and(|s| !s.is_empty()))?;
        Self::reject_min_p(body.min_p)?;
        Self::reject_min_tokens(body.min_tokens)?;
        Self::reject_penalties(
            body.frequency_penalty,
            body.presence_penalty,
            body.repetition_penalty,
        )?;
        Self::reject_logit_bias(body.logit_bias.as_ref().is_some_and(|b| !b.is_empty()))?;

        let sampling_params = proto::SamplingParams {
            temperature: body.temperature,
            top_p: body.top_p.unwrap_or(1.0),
            top_k: body.top_k.map(|v| v.max(0) as u32).unwrap_or(0),
            max_tokens: body.max_tokens,
            stop_token_ids: body.stop_token_ids.clone().unwrap_or_default(),
            ignore_eos: body.ignore_eos,
            logprobs: body.logprobs.map(|v| v.min(20) as i32),
            seed: seed_of(body.sampling_seed, body.seed),
            constraint: Self::constraint_of(completion_constraint(body)?),
        };
        Ok(Self::make_generate_request(
            request_id,
            original_text,
            token_ids,
            sampling_params,
            body.stream,
        ))
    }

    /// Build a GenerateRequest from CreateMessageRequest (Anthropic Messages API)
    #[expect(
        clippy::unused_self,
        reason = "method receiver kept for consistent public API across gRPC backends"
    )]
    pub fn build_generate_request_from_messages(
        &self,
        request_id: String,
        body: &CreateMessageRequest,
        processed_text: String,
        token_ids: Vec<u32>,
        tool_call_constraint: Option<(String, String)>,
    ) -> Result<proto::GenerateRequest, String> {
        Self::reject_stop_strings(body.stop_sequences.as_ref().is_some_and(|s| !s.is_empty()))?;

        let sampling_params = proto::SamplingParams {
            temperature: Some(body.temperature.unwrap_or(1.0) as f32),
            top_p: body.top_p.unwrap_or(1.0) as f32,
            top_k: body.top_k.unwrap_or(0),
            max_tokens: Some(body.max_tokens),
            constraint: Self::parse_constraint(tool_call_constraint)?,
            ..Default::default()
        };
        Ok(Self::make_generate_request(
            request_id,
            processed_text,
            token_ids,
            sampling_params,
            body.stream.unwrap_or(false),
        ))
    }

    /// Build a basic GenerateRequest from the native GenerateRequest
    #[expect(
        clippy::unused_self,
        reason = "method receiver kept for consistent public API across gRPC backends"
    )]
    pub fn build_plain_generate_request(
        &self,
        request_id: String,
        body: &GenerateRequest,
        original_text: Option<String>,
        token_ids: Vec<u32>,
    ) -> Result<proto::GenerateRequest, String> {
        let constraint = match body.sampling_params.as_ref() {
            Some(params) => Self::constraint_of(plain_constraint(params)?),
            None => None,
        };
        let p = PlainSampling::read(body.sampling_params.as_ref());
        Self::reject_n(Some(p.n))?;
        Self::reject_stop_strings(!p.stop.is_empty())?;
        Self::reject_min_p(p.min_p)?;
        Self::reject_min_tokens(p.min_new_tokens)?;
        Self::reject_penalties(
            p.frequency_penalty,
            p.presence_penalty,
            Some(p.repetition_penalty),
        )?;

        let sampling_params = proto::SamplingParams {
            temperature: Some(p.temperature),
            top_p: p.top_p,
            top_k: p.top_k.map(|v| v.max(0) as u32).unwrap_or(0),
            max_tokens: p.max_new_tokens,
            stop_token_ids: p.stop_token_ids,
            ignore_eos: p.ignore_eos,
            logprobs: None,
            seed: p.sampling_seed,
            constraint,
        };
        Ok(Self::make_generate_request(
            request_id,
            original_text.unwrap_or_default(),
            token_ids,
            sampling_params,
            body.stream,
        ))
    }

    /// Build a GenerateRequest from ResponsesRequest (OpenAI Responses API)
    #[expect(
        clippy::unused_self,
        reason = "method receiver kept for consistent public API across gRPC backends"
    )]
    pub fn build_generate_request_from_responses(
        &self,
        request_id: String,
        body: &ResponsesRequest,
        processed_text: String,
        token_ids: Vec<u32>,
        constraint: Option<(String, String)>,
    ) -> Result<proto::GenerateRequest, String> {
        Self::reject_stop_strings(body.stop.as_ref().is_some_and(|s| !s.is_empty()))?;
        Self::reject_min_p(Some(body.min_p))?;
        Self::reject_penalties(
            body.frequency_penalty,
            body.presence_penalty,
            Some(body.repetition_penalty),
        )?;

        let sampling_params = proto::SamplingParams {
            temperature: body.temperature,
            top_p: body.top_p.unwrap_or(1.0),
            top_k: body.top_k.max(0) as u32,
            max_tokens: body.max_output_tokens,
            logprobs: body.top_logprobs.map(|v| v as i32),
            constraint: Self::parse_constraint(constraint)?,
            ..Default::default()
        };
        Ok(Self::make_generate_request(
            request_id,
            processed_text,
            token_ids,
            sampling_params,
            body.stream.unwrap_or(false),
        ))
    }

    /// Shared helper to construct the proto GenerateRequest.
    fn make_generate_request(
        request_id: String,
        text: String,
        token_ids: Vec<u32>,
        sampling_params: proto::SamplingParams,
        stream: bool,
    ) -> proto::GenerateRequest {
        proto::GenerateRequest {
            request_id,
            tokenized: Some(proto::TokenizedInput {
                original_text: text,
                input_ids: token_ids,
            }),
            sampling_params: Some(sampling_params),
            stream,
        }
    }

    // ── Constraints ─────────────────────────────────────────────────────

    /// A chat request's constraint: its `response_format`, `regex` or `ebnf`,
    /// overridden by a tool-call constraint (as vLLM does over HTTP).
    fn build_constraint_for_chat(
        request: &ChatCompletionRequest,
        tool_call_constraint: Option<(String, String)>,
    ) -> Result<Option<proto::sampling_params::Constraint>, String> {
        if tool_call_constraint.is_some() {
            return Self::parse_constraint(tool_call_constraint);
        }

        let mut constraints = Vec::new();
        if let Some(schema) = response_format_schema(request)? {
            constraints.push(proto::sampling_params::Constraint::JsonSchema(schema));
        }
        if let Some(ebnf) = &request.ebnf {
            constraints.push(proto::sampling_params::Constraint::Ebnf(ebnf.clone()));
        }
        if let Some(regex) = &request.regex {
            constraints.push(proto::sampling_params::Constraint::Regex(regex.clone()));
        }
        match constraints.len() {
            0 | 1 => Ok(constraints.pop()),
            _ => Err("Multiple constraints are not allowed.".to_string()),
        }
    }

    /// The gateway's `(kind, value)` constraint as the engine takes it.
    /// DeepInfer's xgrammar backend compiles JSON schemas, regexes and EBNF
    /// grammars; structural tags have no counterpart.
    fn parse_constraint(
        constraint: Option<(String, String)>,
    ) -> Result<Option<proto::sampling_params::Constraint>, String> {
        let Some((kind, value)) = constraint else {
            return Ok(None);
        };
        let parsed = match kind.as_str() {
            "json_schema" => proto::sampling_params::Constraint::JsonSchema(value),
            "regex" => proto::sampling_params::Constraint::Regex(value),
            "grammar" | "ebnf" => proto::sampling_params::Constraint::Ebnf(value),
            "structural_tag" => {
                return Err(
                    "DeepInfer backend does not support structural_tag constraints".to_string(),
                )
            }
            _ => return Err(format!("Unknown constraint type: {kind}")),
        };
        Ok(Some(parsed))
    }

    fn constraint_of(
        constraint: Option<(ConstraintKind, String)>,
    ) -> Option<proto::sampling_params::Constraint> {
        constraint.map(|(kind, value)| match kind {
            ConstraintKind::JsonSchema => proto::sampling_params::Constraint::JsonSchema(value),
            ConstraintKind::Regex => proto::sampling_params::Constraint::Regex(value),
            ConstraintKind::Ebnf => proto::sampling_params::Constraint::Ebnf(value),
        })
    }
}

/// The engine seed: an explicit `sampling_seed`, else the OpenAI `seed` when
/// it is non-negative (DeepInfer seeds are unsigned).
fn seed_of(sampling_seed: Option<u64>, seed: Option<i64>) -> Option<u64> {
    sampling_seed.or_else(|| {
        seed.and_then(|s| {
            let unsigned = u64::try_from(s).ok();
            if unsigned.is_none() {
                warn!(seed = s, "Dropping negative seed for DeepInfer backend");
            }
            unsigned
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::testing::refused_endpoint;

    fn chat(json: serde_json::Value) -> ChatCompletionRequest {
        let mut base = serde_json::json!({
            "model": "deepseek",
            "messages": [{"role": "user", "content": "hi"}],
        });
        if let (Some(base), Some(extra)) = (base.as_object_mut(), json.as_object()) {
            base.extend(extra.clone());
        }
        serde_json::from_value(base).expect("valid chat request")
    }

    fn build_chat(
        body: &ChatCompletionRequest,
        constraint: Option<(String, String)>,
    ) -> Result<proto::GenerateRequest, String> {
        // The builders never touch the connection, so a lazy channel suffices.
        let channel = Channel::from_static("http://127.0.0.1:1").connect_lazy();
        let client = DeepInferEngineClient {
            client: proto::deep_infer_engine_client::DeepInferEngineClient::new(channel),
            trace_injector: std::sync::Arc::new(crate::NoopTraceInjector),
        };
        client.build_generate_request_from_chat(
            "r".to_string(),
            body,
            String::new(),
            vec![1, 2],
            constraint,
        )
    }

    #[tokio::test]
    async fn chat_refuses_features_the_engine_lacks() {
        for (body, needle) in [
            (serde_json::json!({"n": 2}), "n > 1"),
            (serde_json::json!({"stop": ["\n"]}), "stop sequences"),
            (serde_json::json!({"min_p": 0.1}), "min_p"),
            (
                serde_json::json!({"frequency_penalty": 0.5}),
                "frequency_penalty",
            ),
            (
                serde_json::json!({"repetition_penalty": 1.1}),
                "repetition_penalty",
            ),
            (serde_json::json!({"logit_bias": {"5": 1.0}}), "logit_bias"),
        ] {
            let err = build_chat(&chat(body.clone()), None).expect_err("refused");
            assert!(err.contains(needle), "{body}: {err}");
        }
    }

    #[tokio::test]
    async fn chat_accepts_neutral_values_and_maps_sampling() {
        let body = chat(serde_json::json!({
            "n": 1,
            "min_p": 0.0,
            "frequency_penalty": 0.0,
            "repetition_penalty": 1.0,
            "temperature": 0.5,
            "top_k": 20,
            "logprobs": true,
            "top_logprobs": 3,
            "seed": 42,
            "stop_token_ids": [7],
        }));
        let req = build_chat(&body, None).expect("built");
        let sp = req.sampling_params.expect("sampling params");
        assert_eq!(sp.temperature, Some(0.5));
        assert_eq!(sp.top_k, 20);
        assert_eq!(sp.logprobs, Some(3));
        assert_eq!(sp.seed, Some(42));
        assert_eq!(sp.stop_token_ids, vec![7]);
        assert_eq!(req.tokenized.expect("tokenized").input_ids, vec![1, 2]);
    }

    #[tokio::test]
    async fn tool_constraint_overrides_response_format_and_structural_tags_are_refused() {
        let body = chat(serde_json::json!({"response_format": {"type": "json_object"}}));
        let req = build_chat(&body, None).expect("built");
        assert_eq!(
            req.sampling_params.expect("sp").constraint,
            Some(proto::sampling_params::Constraint::JsonSchema(
                r#"{"type":"object"}"#.to_string()
            ))
        );

        let req = build_chat(
            &body,
            Some((
                "json_schema".to_string(),
                "{\"type\":\"array\"}".to_string(),
            )),
        )
        .expect("built");
        assert_eq!(
            req.sampling_params.expect("sp").constraint,
            Some(proto::sampling_params::Constraint::JsonSchema(
                "{\"type\":\"array\"}".to_string()
            ))
        );

        let err = build_chat(
            &body,
            Some(("structural_tag".to_string(), "{}".to_string())),
        )
        .expect_err("refused");
        assert!(err.contains("structural_tag"), "{err}");
    }

    #[test]
    fn negative_seeds_are_dropped_and_sampling_seed_wins() {
        assert_eq!(seed_of(None, Some(-1)), None);
        assert_eq!(seed_of(None, Some(9)), Some(9));
        assert_eq!(seed_of(Some(3), Some(9)), Some(3));
    }

    #[tokio::test]
    async fn plain_request_refuses_stop_strings() {
        let channel = Channel::from_static("http://127.0.0.1:1").connect_lazy();
        let client = DeepInferEngineClient {
            client: proto::deep_infer_engine_client::DeepInferEngineClient::new(channel),
            trace_injector: std::sync::Arc::new(crate::NoopTraceInjector),
        };
        let body: GenerateRequest = serde_json::from_value(serde_json::json!({
            "text": "hi",
            "sampling_params": {"stop": "x"},
        }))
        .expect("valid generate request");
        let err = client
            .build_plain_generate_request("r".to_string(), &body, None, vec![1])
            .expect_err("refused");
        assert!(err.contains("stop sequences"), "{err}");
    }

    #[tokio::test]
    async fn connect_fails_on_a_malformed_or_refused_endpoint() {
        let malformed = DeepInferEngineClient::connect("not an endpoint").await;
        assert!(
            malformed.is_err(),
            "a malformed endpoint fails before the dial"
        );
        let refused = refused_endpoint();
        let result = DeepInferEngineClient::connect(&refused.url).await;
        assert!(result.is_err(), "a refused dial fails the connect");
    }
}
