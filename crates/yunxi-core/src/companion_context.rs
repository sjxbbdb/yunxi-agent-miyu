//! Stable, optional companion context carried in an agent system prompt.
//!
//! This is deliberately a small transport value rather than a profile model:
//! later layers may fill its payload fields, while the prompt representation
//! remains deterministic and versioned.

use serde::{Deserialize, Serialize};

/// Schema version for the companion context wire value.
pub const COMPANION_CONTEXT_SCHEMA_VERSION: u16 = 1;

const MAX_METADATA_CHARS: usize = 64;
const MAX_FIELD_CHARS: usize = 512;
const MAX_BOUNDARY_CHARS: usize = 256;
const MAX_BOUNDARIES: usize = 16;

/// Optional companion state supplied by a caller for the current agent.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CompanionContext {
    pub source: String,
    pub version: u16,
    pub scope: String,
    pub relationship_stage: Option<String>,
    pub stable_tone: Option<String>,
    pub boundaries: Vec<String>,
    pub response_preference: Option<String>,
    pub current_companion_state: Option<String>,
}

impl CompanionContext {
    /// Construct an empty context at the current schema version.
    pub fn new(source: impl Into<String>, scope: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            version: COMPANION_CONTEXT_SCHEMA_VERSION,
            scope: scope.into(),
            relationship_stage: None,
            stable_tone: None,
            boundaries: Vec::new(),
            response_preference: None,
            current_companion_state: None,
        }
    }

    pub fn with_relationship_stage(mut self, value: impl Into<String>) -> Self {
        self.relationship_stage = Some(value.into());
        self
    }

    pub fn with_stable_tone(mut self, value: impl Into<String>) -> Self {
        self.stable_tone = Some(value.into());
        self
    }

    pub fn with_boundaries<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.boundaries = values.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_response_preference(mut self, value: impl Into<String>) -> Self {
        self.response_preference = Some(value.into());
        self
    }

    pub fn with_current_companion_state(mut self, value: impl Into<String>) -> Self {
        self.current_companion_state = Some(value.into());
        self
    }

    pub fn set_relationship_stage(&mut self, value: Option<String>) {
        self.relationship_stage = value;
    }

    pub fn set_stable_tone(&mut self, value: Option<String>) {
        self.stable_tone = value;
    }

    pub fn set_boundaries(&mut self, values: Vec<String>) {
        self.boundaries = values;
    }

    pub fn set_response_preference(&mut self, value: Option<String>) {
        self.response_preference = value;
    }

    pub fn set_current_companion_state(&mut self, value: Option<String>) {
        self.current_companion_state = value;
    }

    /// Whether this value is safe to accept and render.
    pub fn is_supported(&self) -> bool {
        let source = self.source.trim();
        let scope = self.scope.trim();
        self.version == COMPANION_CONTEXT_SCHEMA_VERSION
            && !source.is_empty()
            && !scope.is_empty()
            && source.chars().count() <= MAX_METADATA_CHARS
            && scope.chars().count() <= MAX_METADATA_CHARS
            && optional_field_supported(self.relationship_stage.as_deref())
            && optional_field_supported(self.stable_tone.as_deref())
            && optional_field_supported(self.response_preference.as_deref())
            && optional_field_supported(self.current_companion_state.as_deref())
            && self
                .boundaries
                .iter()
                .filter_map(|value| non_empty_trimmed(value))
                .all(|value| value.chars().count() <= MAX_BOUNDARY_CHARS)
            && self
                .boundaries
                .iter()
                .filter(|value| !value.trim().is_empty())
                .count()
                <= MAX_BOUNDARIES
    }

    /// Parse and validate a JSON context without panicking or logging input.
    pub fn from_json(input: &[u8]) -> Option<Self> {
        let mut context: Self = serde_json::from_slice(input).ok()?;
        context.normalize();
        context.is_supported().then_some(context)
    }

    /// Render the supported payload as a deterministic XML-style block.
    pub fn to_prompt_block(&self) -> Option<String> {
        if !self.is_supported() {
            return None;
        }

        let relationship_stage =
            non_empty_trimmed(self.relationship_stage.as_deref().unwrap_or(""));
        let stable_tone = non_empty_trimmed(self.stable_tone.as_deref().unwrap_or(""));
        let response_preference =
            non_empty_trimmed(self.response_preference.as_deref().unwrap_or(""));
        let current_companion_state =
            non_empty_trimmed(self.current_companion_state.as_deref().unwrap_or(""));
        let boundaries = self
            .boundaries
            .iter()
            .filter_map(|value| non_empty_trimmed(value))
            .collect::<Vec<_>>();

        if relationship_stage.is_none()
            && stable_tone.is_none()
            && boundaries.is_empty()
            && response_preference.is_none()
            && current_companion_state.is_none()
        {
            return None;
        }

        let mut lines = vec![format!(
            "<companion-context source=\"{}\" version=\"{}\" scope=\"{}\">",
            xml_escape(self.source.trim()),
            self.version,
            xml_escape(self.scope.trim()),
        )];
        if let Some(value) = relationship_stage {
            lines.push(format!(
                "<relationship-stage>{}</relationship-stage>",
                xml_escape(value)
            ));
        }
        if let Some(value) = stable_tone {
            lines.push(format!("<stable-tone>{}</stable-tone>", xml_escape(value)));
        }
        for value in boundaries {
            lines.push(format!("<boundary>{}</boundary>", xml_escape(value)));
        }
        if let Some(value) = response_preference {
            lines.push(format!(
                "<response-preference>{}</response-preference>",
                xml_escape(value)
            ));
        }
        if let Some(value) = current_companion_state {
            lines.push(format!(
                "<current-companion-state>{}</current-companion-state>",
                xml_escape(value)
            ));
        }
        lines.push("</companion-context>".to_string());
        Some(lines.join("\n"))
    }

    fn normalize(&mut self) {
        self.source = self.source.trim().to_string();
        self.scope = self.scope.trim().to_string();
        self.relationship_stage = trim_optional(self.relationship_stage.take());
        self.stable_tone = trim_optional(self.stable_tone.take());
        self.response_preference = trim_optional(self.response_preference.take());
        self.current_companion_state = trim_optional(self.current_companion_state.take());
        self.boundaries = self
            .boundaries
            .drain(..)
            .filter_map(|value| {
                let value = value.trim();
                (!value.is_empty()).then(|| value.to_string())
            })
            .collect();
    }
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| non_empty_trimmed(&value).map(str::to_string))
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn optional_field_supported(value: Option<&str>) -> bool {
    value
        .and_then(non_empty_trimmed)
        .is_none_or(|value| value.chars().count() <= MAX_FIELD_CHARS)
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_does_not_render() {
        assert!(CompanionContext::new("test", "session")
            .to_prompt_block()
            .is_none());
    }

    #[test]
    fn fields_render_in_stable_order() {
        let context = CompanionContext::new("source", "scope")
            .with_current_companion_state("state")
            .with_boundaries(["one", "two"])
            .with_relationship_stage("stage")
            .with_stable_tone("tone")
            .with_response_preference("brief");
        assert_eq!(
            context.to_prompt_block().as_deref(),
            Some(
                "<companion-context source=\"source\" version=\"1\" scope=\"scope\">\n<relationship-stage>stage</relationship-stage>\n<stable-tone>tone</stable-tone>\n<boundary>one</boundary>\n<boundary>two</boundary>\n<response-preference>brief</response-preference>\n<current-companion-state>state</current-companion-state>\n</companion-context>"
            )
        );
    }

    #[test]
    fn xml_is_escaped() {
        let context = CompanionContext::new("a&\"", "s<'").with_relationship_stage("x<&>\"'");
        assert_eq!(
            context.to_prompt_block().as_deref(),
            Some(
                "<companion-context source=\"a&amp;&quot;\" version=\"1\" scope=\"s&lt;&apos;\">\n<relationship-stage>x&lt;&amp;&gt;&quot;&apos;</relationship-stage>\n</companion-context>"
            )
        );
    }

    #[test]
    fn invalid_json_and_metadata_fall_back_to_none() {
        assert!(CompanionContext::from_json(b"not-json").is_none());
        let old = serde_json::json!({"source":"a","version":0,"scope":"b"});
        assert!(CompanionContext::from_json(&serde_json::to_vec(&old).unwrap()).is_none());
        let empty = serde_json::json!({"source":" ","version":1,"scope":"b"});
        assert!(CompanionContext::from_json(&serde_json::to_vec(&empty).unwrap()).is_none());
    }

    #[test]
    fn oversized_payload_is_rejected() {
        let context = CompanionContext::new("source", "scope")
            .with_stable_tone("x".repeat(MAX_FIELD_CHARS + 1));
        assert!(!context.is_supported());
        let context = CompanionContext::new("source", "scope")
            .with_boundaries(std::iter::repeat_n("x", MAX_BOUNDARIES + 1));
        assert!(!context.is_supported());
    }

    #[test]
    fn json_roundtrip_preserves_context() {
        let context = CompanionContext::new("source", "scope")
            .with_relationship_stage("stage")
            .with_boundaries(["one", "two"]);
        let encoded = serde_json::to_vec(&context).unwrap();
        assert_eq!(CompanionContext::from_json(&encoded), Some(context));
    }
}
