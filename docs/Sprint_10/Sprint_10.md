# Sprint_10 — Diário de bordo

**Milestone:** M5a (parte 2, gate) — Validação de qualidade do LLM local
**Planning:** ver `Sprint_Planning_10.md`
**Início:** 2026-11-03
**Fim:** (em aberto — gate por rever)

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-11-03

- Sessão cloud. Sprint_09 fechada; esta é a seguinte (gate M5a).
- Preparado `docs/Sprint_10/corpus_pt.txt`: 100 segmentos PT (frases curtas de reunião, 1–12 palavras, com números, nomes, perguntas e interjeições).
- Preparado `crates/vad-ai/src/quality_gate.rs`: pré-triagem automática (`screen`, 2 testes unitários verdes) e o teste `#[ignore] gate_sprint10_translate_corpus`, que passa os 100 segmentos por `LlmTranslator` + LocalQwen real para PT→EN/ES/FR e escreve a folha de revisão (`VAD_GATE_OUT`, por omissão `gate_results.md`).
- Instalado `libmpv-dev` no container (faltava; sem ele nada de `vad-ai` ligava).
- **Bloqueio:** o proxy da sessão devolve 403 para `huggingface.co`, logo não foi possível descarregar o GGUF Qwen2.5-0.5B nem o tokenizer. **O LocalQwen não foi executado; não existem resultados de tradução.**
- Para correr: `VAD_TEST_QWEN_MODEL=<gguf> VAD_TEST_QWEN_TOKENIZER=<tokenizer.json> cargo test -p vad-ai gate_sprint10 -- --ignored --nocapture`.

## Desvios face ao Sprint_Planning_10.md

- Os 100 segmentos são **escritos à mão** com a forma de legendas de reunião, não saídos de transcrições Whisper reais (não há áudio nem rede nesta sessão). O planeamento pede segmentos reais: se o utilizador tiver transcrições próprias, substituir `corpus_pt.txt`.
- A revisão é pelo utilizador; as flags automáticas só pré-triam e nunca aprovam.

## Problemas encontrados

- `huggingface.co` bloqueado pela política de rede do ambiente cloud (403).
- `libmpv-dev` em falta no container.
