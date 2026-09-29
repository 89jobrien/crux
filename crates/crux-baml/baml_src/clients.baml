// Default LLM clients used by cruxx-agentic extraction functions.
//
// Model selection: set OPENAI_MODEL / ANTHROPIC_MODEL env vars to override
// the defaults below. If unset, falls back to the hardcoded defaults.

// -- OpenAI -----------------------------------------------------------------

client<llm> OpenAICustom {
  provider openai
  options {
    model env.OPENAI_MODEL
    api_key env.OPENAI_API_KEY
  }
}

client<llm> OpenAIDefault {
  provider openai
  options {
    model "gpt-4o-mini"
    api_key env.OPENAI_API_KEY
  }
}

client<llm> OpenAI {
  provider fallback
  options {
    strategy [OpenAICustom, OpenAIDefault]
  }
}

// -- Anthropic --------------------------------------------------------------

client<llm> AnthropicCustom {
  provider anthropic
  options {
    model env.ANTHROPIC_MODEL
    api_key env.ANTHROPIC_API_KEY
  }
}

client<llm> AnthropicDefault {
  provider anthropic
  options {
    model "claude-haiku-4-5-20251001"
    api_key env.ANTHROPIC_API_KEY
  }
}

client<llm> Anthropic {
  provider fallback
  options {
    strategy [AnthropicCustom, AnthropicDefault]
  }
}

// -- Ollama (local, no API key) ---------------------------------------------
//
// Ollama is reached through its OpenAI-compatible `/v1` endpoint, so BAML
// uses the `openai-generic` provider with an overridden base_url. Serve it
// with `OLLAMA_ORIGINS='*' ollama serve` so non-browser clients are allowed.
//
// `OllamaDefault` is fully hardcoded so it always resolves; `OllamaCustom`
// reads OLLAMA_BASE_URL / OLLAMA_MODEL and is only safe to reference from a
// pipeline via `client: OllamaCustom` once both variables are exported.

client<llm> OllamaCustom {
  provider openai-generic
  options {
    base_url env.OLLAMA_BASE_URL
    model env.OLLAMA_MODEL
    default_role "user"
  }
}

client<llm> OllamaDefault {
  provider openai-generic
  options {
    base_url "http://localhost:11434/v1"
    model "llama3.2"
    default_role "user"
  }
}

client<llm> Ollama {
  provider fallback
  options {
    strategy [OllamaDefault]
  }
}

// -- Local-first (no credentials required) -----------------------------------
//
// Preferred by the analysis functions (`llm::analyze`, `llm::confidence`).
// Runs entirely against a local Ollama when one is reachable and falls back
// to the hosted providers only if the local call cannot be made.
//
// Two constraints shape this list:
//
// 1. It must stay flat. BAML 0.221 does not resolve a fallback client nested
//    inside another fallback strategy; the outer chain ends up empty and the
//    call fails with "No events in the chain".
// 2. Every member must resolve without environment variables. A client that
//    references an unset `env.*` aborts the whole chain rather than being
//    skipped, so the `*Custom` clients are deliberately excluded.

client<llm> Local {
  provider fallback
  options {
    strategy [OllamaDefault, OpenAIDefault, AnthropicDefault]
  }
}

// -- Auto (round-robin across providers) ------------------------------------

client<llm> Auto {
  provider fallback
  options {
    strategy [OpenAI, Anthropic]
  }
}
