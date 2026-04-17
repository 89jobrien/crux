// Structured extraction functions for crux-agentic llm::extract handler.

// ============================================================================
// Entity Extraction
// ============================================================================

class Entity {
  name string
  entity_type string
  description string?
}

class ExtractEntitiesOutput {
  entities Entity[]
}

function ExtractEntities(text: string) -> ExtractEntitiesOutput {
  client OpenAI
  prompt #"
    Extract all named entities from the following text.

    Text: {{ text }}

    For each entity, provide:
    - name: the entity as it appears in the text
    - entity_type: one of PERSON, ORGANIZATION, LOCATION, DATE, CONCEPT, OTHER
    - description: brief description if context is available (optional)

    {{ ctx.output_format }}
  "#
}

// ============================================================================
// Summarization
// ============================================================================

class SummarizeOutput {
  summary string
  key_points string[]
  word_count int
}

function Summarize(text: string, max_sentences: int) -> SummarizeOutput {
  client OpenAI
  prompt #"
    Summarize the following text concisely.

    Text: {{ text }}
    Target length: {{ max_sentences }} sentences or fewer.

    Provide:
    - summary: the condensed version
    - key_points: 2-5 most important points as bullet items
    - word_count: approximate word count of the original text

    {{ ctx.output_format }}
  "#
}

// ============================================================================
// Classification
// ============================================================================

class ClassifyOutput {
  label string
  confidence float
  reasoning string?
}

function Classify(text: string, labels: string[]) -> ClassifyOutput {
  client OpenAI
  prompt #"
    Classify the following text into exactly one of the provided labels.

    Text: {{ text }}
    Labels: {{ labels }}

    Provide:
    - label: the chosen label (must be one of the provided labels)
    - confidence: confidence score from 0.0 to 1.0
    - reasoning: brief explanation of why this label was chosen (optional)

    {{ ctx.output_format }}
  "#
}
