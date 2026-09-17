# Sprint_Planning_09 — M5a (parte 1): LLM local — resumo e tradução

**Milestone:** M5a, parte 1 de 2 (`PLANO_VAD.md` §9/§11)
**Pré-requisito:** Sprint_08 fechada (M4 completo).

## Objetivo

Infraestrutura de LLM local para resumo e tradução, com chunking para transcrições
longas.

## Tarefas

1. `vad-ai/src/llm_provider.rs`: trait `Summarizer` + enum com a variante `LocalQwen`
   (`Qwen2.5-0.5B-Instruct-Q4_K_M` ~350MB, ou `Llama-3.2-1B-Instruct-Q4_K_M` ~700MB,
   via `llama-cpp-rs` ou `candle`). As variantes de cloud (`OpenAiCompatible`,
   `Anthropic`, `Gemini`) só entram na Sprint_11+, mas **desenhar o enum já a prever
   esse espaço** (§4.2), para não obrigar a um refactor mais tarde.
2. `vad-ai/src/summarizer.rs`: chunking/map-reduce (§4.19) — resume por blocos e funde
   os resumos parciais. A janela de contexto é por provider, não uma constante global
   — desenhar a interface já com isso em mente, mesmo só com `LocalQwen` implementado
   nesta sprint.
3. `vad-ai/src/translator.rs`: modo `translate` do Whisper (PT→EN, já disponível desde
   M3) + M5a: reutilizar o `LocalQwen` via prompt para PT↔outro idioma (§4.1).
4. Progresso por bloco (§4.20): "a resumir bloco 3 de 7" em vez de streaming SSE
   token-a-token (exigiria parsing por provider para um caso de uso batch, não chat ao
   vivo).
5. Badge 🔒 local visível na UI antes/durante cada operação (§4.1). Mesmo só havendo o
   caminho local nesta sprint, o **mecanismo de badge tem de existir desde já** — a
   Sprint_11+ só acrescenta a variante ☁️, não constrói o mecanismo de raiz.

## Fora de âmbito

Qualquer provider cloud, keyring, `settings_panel.rs` de IA — Sprint_11+. O gate de
qualidade em si — Sprint_10.

## Critério de saída

Resumo de uma transcrição de 90 min sem exceder a janela de contexto do `LocalQwen` —
o map-reduce tem de estar de facto a funcionar, não só presente no código.

## Nota de RAM (§5)

O modelo local soma-se aos ~55MB do Whisper quantizado quando ambos carregados —
registar o RSS real desta combinação no diário da sprint.
