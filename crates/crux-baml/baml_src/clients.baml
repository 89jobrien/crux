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

// -- Auto (round-robin across providers) ------------------------------------

client<llm> Auto {
  provider fallback
  options {
    strategy [OpenAI, Anthropic]
  }
}
