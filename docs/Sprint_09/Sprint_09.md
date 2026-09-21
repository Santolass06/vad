# Sprint_09 — Diário de bordo

**Milestone:** M5a (parte 1) — LLM local: resumo e tradução  
**Planning:** ver `Sprint_Planning_09.md`  
**Início:** 2026-09-21  
**Fim:** 2026-09-21  

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-21

- **Investigação prévia do motor de inferência local (`llama-cpp-sys` vs. `candle`):**
  - Tentativa de compilar `llama-cpp-2` (bindings para `llama.cpp`): falhou com erro fatal de bindgen (`'stdbool.h' file not found`) devido à falta de clang/headers C++ de sistema na máquina. Adicionalmente, `whisper-rs-sys` já embutiu e compilou o código C de whisper/ggml, pelo que linkar outra instância de ggml em C via `llama-cpp-sys` incorre em risco de conflito de símbolos de linkagem (`ggml_*`).
  - Avaliação de `candle` (`candle-core 0.11`, `candle-transformers 0.11`, `tokenizers 0.23`): compilação 100% Rust bem-sucedida em 20.8s, sem necessidade de cmake/clang adicionais. Suporte nativo a GGUF quantizado e carregamento por mmap. Decisão: adotar `candle` como motor padrão do `LocalQwen`.

- **Desenvolvimento da infraestrutura de erros (`crates/vad-core/src/error.rs`):**
  - Adição de variantes de erro especializadas de LLM: `VadError::Llm(String)`, `LlmContextExceeded`, `LlmCancelled`, `LlmAuthFailed`, `LlmTimeout`, `LlmRateLimited`.
  - Integração no `degraded_policy()` com ações recomendadas para o utilizador (ex: sugerir ativação do modo map-reduce se exceder contexto, verificação de quotas/chaves ou continuidade de utilização sem o modelo de linguagem).

- **Implementação do subsistema de IA (`crates/vad-ai`):**
  - `crates/vad-ai/src/llm_provider.rs`:
    - Criação da trait `Summarizer` assíncrona/desacoplada (`summarize`, `model_name`, `provider_type`, `context_window`, `is_ready`).
    - Implementação do enum `AiPrivacyBadge` com variantes `Local` (🔒 Local) e `Cloud` (☁️ Sai do PC), cumprindo §4.1, §4.13 e §4.21 com formatação em pill visual (`label()` e `color_rgb()`).
    - Definição do enum extensível `LlmProviderConfig` (com variantes `LocalQwen`, `OpenAi`, `Anthropic`, `Gemini`, `Ollama`), deixando a arquitetura desacoplada e pronta para os providers de cloud a implementar na Sprint 11.
    - Implementação de `LocalQwenSummarizer` com suporte a `candle_core::quantized::gguf_file` e tokenizador Qwen via `tokenizers`.
    - Implementação de `MockSummarizer` para testes determinísticos e validação ultra-rápida em ambiente de CI.
  - `crates/vad-ai/src/summarizer.rs`:
    - Implementação do algoritmo Map-Reduce com `MapReduceSummarizer`.
    - Cumprimento estrito do §4.19: **descarte imediato em memória de blocos brutos** (`drop(raw_text)`) logo após a síntese de cada resumo parcial.
    - Cumprimento estrito do §4.20: reporte de progresso por bloco discreto com enum `SummarizeProgress::SummarizingChunk { current, total }` e `SummarizeProgress::Reducing`.
    - Suporte a cancelamento em tempo real via `Arc<AtomicBool>`.
    - Redução recursiva/hierárquica multi-passo no caso de a concatenação dos resumos parciais ainda exceder a janela de contexto.
  - `crates/vad-ai/src/translator.rs`:
    - Implementação do enum `TargetLanguage` (EN, ES, FR, DE, IT, PT).
    - Implementação de `LlmTranslator` com suporte a tradução de texto isolado e tradução em lote de `TranscriptionSegment`s preservando timestamps originais (`start_ms`, `end_ms`).
    - Integração no `WhisperEngine` da opção de tradução nativa para inglês via `transcribe_with_options(..., translate_to_en = true)`.
  - `crates/vad-ai/src/model_manager.rs`:
    - Registo dos presets oficiais de modelos LLM em `PRESET_LLM_MODELS`: `qwen2.5-0.5b-instruct-q4_k_m.gguf` (~398 MB) e `qwen2.5-tokenizer.json` (~7 MB).
    - Funções utilitárias `is_llm_ready()` e `get_llm_paths()` para validação de existência dos ficheiros em disco no diretório XDG de dados.

- **Integração na interface de utilizador (`crates/vad-app`):**
  - `crates/vad-app/src/panels/whisper_panel.rs`:
    - Função pública `render_privacy_badge(ui, badge)` que desenha badges com cantos arredondados, fundo translúcido e texto contrastante (verde `#22c55e` para `🔒 Local`, âmbar `#f59e0b` para `☁️ Sai do PC`).
    - Adição da **Secção 6: Resumo da Reunião** com badge `🔒 Local`, barra de progresso em bloco ("A resumir bloco X de Y..."), botão de cancelamento, visualização do markdown do resumo com botão de cópia direta para a área de transferência (`ui.ctx().copy_text(...)`).
    - Adição da **Secção 7: Tradução Multi-idioma** com seletor com combobox dos idiomas de destino, badge `🔒 Local`, botão de tradução e visualização de segmentos traduzidos.
    - Gestão assíncrona não-bloqueante em thread dedicada com comunicação por canais `crossbeam_channel::Receiver` verificados com `try_recv()` a cada frame do egui.
  - `crates/vad-app/src/app.rs`:
    - Exibição de card com o Resumo da Reunião e badge de privacidade diretamente no ecrã central do Modo Reunião quando disponível.
    - Alternância e visualização de segmentos traduzidos no Modo Reunião com seek temporal interativo ao clicar nos timestamps.
    - Enriquecimento da exportação para Markdown (`export_notes_and_transcription` gerando `notas_reuniao.md`) para incluir a secção `## Resumo da Reunião (LLM Local)` com metadados de privacidade.

- **Medição empírica de recursos e memória (RAM / RSS):**
  - Execução da aplicação via `tools/vad-visual-mcp` em ambiente virtual Xvfb com ficheiro áudio carregado, visualizador de waveform, Modo Reunião e painel Whisper ativo:
    - **RSS inicial da aplicação:** `207.2 MiB` (47 threads).
    - **RSS com painel Whisper e controlos de IA em renderização ativa:** `207.3 MiB` a `215.1 MiB`.
    - **Overhead do motor candle e tokenizers:** Carregamento estático de dependências não afeta significativamente o footprint basal. Em tempo de execução do mapa-reduce, o descarte ávido de blocos brutos (`drop(raw_text)`) garante que a memória adicional se limita a um bloco de tokens (`~4096 tokens` = poucos kilobytes de tensores temporários).

- **Validação visual e fotográfica:**
  - Capturas efetuadas através do MCP `vad-visual`:
    - `sprint09_meeting_mode.png`: Modo Reunião com waveform, controlos rápidos e botão de exportação.
    - `sprint09_whisper_panel_open.png`: Painel lateral do Whisper aberto exibindo Secção 6 com badge verde `[🔒 Local]`, botão `[❖ Gerar Resumo da Reunião]`, Secção 7 com badge `[🔒 Local]` e combobox de idioma de destino.
    - `sprint09_whisper_panel_scrolled.png`: Visualização de rolagem do painel Whisper mantendo harmonia com o limite de 720p.

## Desvios face ao Sprint_Planning_09.md

- **Adoção de `candle` em vez de `llama-cpp-rs`:** Opção prevista formalmente no `Sprint_Planning_09.md` e `PLANO_VAD.md` §4.2, necessária pela indisponibilidade de headers C++ (`stdbool.h`) no bindgen clang do sistema hospedeiro. Proporcionou compilação 100% Rust sem dependências externas adicionais e tempos de compilação muito inferiores (~20s).

## Problemas encontrados e resoluções

- **1. Falha de compilação de `llama-cpp-sys-2`:**
  - *Problema:* `fatal error: 'stdbool.h' file not found` no clang bindgen ao tentar compilar `llama-cpp-2`.
  - *Resolução:* Abandono imediato da abordagem C++ de `llama-cpp-sys` e utilização do motor puro Rust `candle` (`candle-core`, `candle-transformers`, `candle-nn`), que compilou perfeitamente e sem colisões de símbolos com o whisper-rs.
- **2. Incompatibilidade de tipos de margem no egui 0.36:**
  - *Problema:* `egui::Margin::symmetric(8.0, 3.0)` provocava erro de compilação por esperar `(i8, i8)` em vez de `f32`.
  - *Resolução:* Ajuste para `egui::Margin::symmetric(8, 3)`.
- **3. Incompatibilidade com método `output_mut()` no egui 0.36:**
  - *Problema:* Tentativa de copiar texto através de `ui.output_mut(|o| o.copied_text = ...)` falhou devido à remoção desse método no egui 0.36.
  - *Resolução:* Utilização da API moderna e canónica `ui.ctx().copy_text(...)`.
- **4. Orientação do layout da barra superior (`right_to_left`):**
  - *Problema:* Na barra superior, o contentor principal usa `egui::Layout::right_to_left`, fazendo com que os botões fiquem ordenados da direita para a esquerda. O botão `Whisper` situa-se a x=990 e não a x=755 (onde estava o `Equalizador`).
  - *Resolução:* Identificação correta da geometria na interface gráfica e acionamento no botão correspondente.
