// Default LLM clients used by crux-agentic extraction functions.

client<llm> OpenAI {
  provider openai
  options {
    model "gpt-4o-mini"
    api_key env.OPENAI_API_KEY
  }
}

client<llm> Anthropic {
  provider anthropic
  options {
    model "claude-haiku-20240307"
    api_key env.ANTHROPIC_API_KEY
  }
}

client<llm> Auto {
  provider round-robin
  options {
    strategy [Anthropic, OpenAI]
  }
}
