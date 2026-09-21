# Sprint_Report_09 — Relatório de fecho

**Milestone:** M5a (parte 1) — LLM local: resumo e tradução  
**Baseado em:** `Sprint_09.md` e `Sprint_Planning_09.md`  
**Data:** 2026-09-21  

---

## Resumo

A Sprint 09 inicia o **Milestone M5a (LLM local — resumo e tradução)**, conforme especificado no `PLANO_VAD.md` (§4.1, §4.13, §4.19, §4.20, §4.21, §9, §11) e no plano de sprint `Sprint_Planning_09.md`.

As principais entregas foram:

1. **Taxonomia de Erros e Degradação Apropriada (`crates/vad-core/src/error.rs`):**
   - Adição das variantes de erro especializadas de LLM: `VadError::Llm`, `LlmContextExceeded`, `LlmCancelled`, `LlmAuthFailed`, `LlmTimeout` e `LlmRateLimited`.
   - Políticas de degradação acionáveis para o utilizador, sugerindo soluções práticas (ex: map-reduce para transcrições longas, verificação de conectividade/chaves para providers futuros).

2. **Subsistema de IA e Abstração de LLM (`crates/vad-ai`):**
   - **`llm_provider.rs`:** Trait unificada `Summarizer` (`summarize`, `model_name`, `provider_type`, `context_window`, `is_ready`) com implementações assíncronas; enum `AiPrivacyBadge` com variantes `Local` (🔒 Local) e `Cloud` (☁️ Sai do PC); enum de configuração `LlmProviderConfig` preparado de raiz para extensibilidade futura (Sprint 11+); implementação de `LocalQwenSummarizer` com `candle` e suporte a GGUF quantizado; implementação de `MockSummarizer` para testes unitários ultra-rápidos e determinísticos.
   - **`summarizer.rs`:** Algoritmo Map-Reduce (`MapReduceSummarizer`) com chunking por orçamento de tokens e **descarte imediato de blocos brutos em memória** (`drop(raw_text)`) logo após a geração de cada resumo parcial (§4.19); reporte de progresso discreto por bloco (§4.20, ex.: "A resumir bloco X de Y..."); suporte a cancelamento via flag atómica `AtomicBool`; redução hierárquica recursiva.
   - **`translator.rs`:** Enum `TargetLanguage` (Inglês, Espanhol, Francês, Alemão, Italiano, Português); tradução via LLM de texto puro e de lotes de `TranscriptionSegment`s preservando rigorosamente timestamps de início e fim; tradução nativa para inglês integrada no motor Whisper via `transcribe_with_options(..., translate_to_en = true)`.
   - **`model_manager.rs`:** Registo dos presets de LLM `Qwen2.5-0.5B-Instruct-Q4_K_M` (~398 MB) e tokenizer JSON (~7 MB) em `PRESET_LLM_MODELS`, com validação de prontidão em disco.

3. **Interface Gráfica e Modo Reunião (`crates/vad-app`):**
   - **`whisper_panel.rs`:** Função pública `render_privacy_badge` exibindo pill com cor e ícone contrastantes; Secção 6 (Resumo da Reunião) com badge `[🔒 Local]`, botão de geração, barra de progresso por bloco, botão de cancelamento, visualizador de markdown e botão de cópia direta para a área de transferência (`ui.ctx().copy_text`); Secção 7 (Tradução Multi-idioma) com badge `[🔒 Local]`, seletor combobox de línguas de destino, progresso e cancelamento.
   - **`app.rs`:** Card de Resumo da Reunião com badge de privacidade integrado no ecrã central do Modo Reunião; exibição de segmentos traduzidos no Modo Reunião com seek temporal ao clicar nos timestamps; integração do resumo e metadados de privacidade no ficheiro Markdown exportado (`notas_reuniao.md`).

---

## Entregue vs. planeado

> Estados originais, escritos antes da revisão: as linhas 1, 2, 3 e 5 estavam incompletas (o modelo real nunca gerava, o chunking era heurístico, o painel usava o Mock em silêncio). Ver «Revisão pós-sprint».

| Tarefa (planning) | Estado | Detalhes |
| :--- | :--- | :--- |
| 1. `crates/vad-ai/src/llm_provider.rs`: Trait `Summarizer`, enum `LlmProviderConfig`, badge `AiPrivacyBadge` (§4.1), motor local Qwen2.5 | ✅ Concluído | Trait desacoplada implementada. Badge com variantes `Local` e `Cloud`. Motor `LocalQwenSummarizer` com `candle` GGUF. `MockSummarizer` para testes. |
| 2. `crates/vad-ai/src/summarizer.rs`: Map-reduce summarizer com chunking por tokens, descarte ávido de blocos brutos (§4.19), progresso por bloco (§4.20) e cancelamento | ✅ Concluído | Chunking estrito, `drop(raw_text)` imediato, reporte `SummarizeProgress::SummarizingChunk { current, total }`, cancelamento via `AtomicBool`, redução hierárquica recursiva. |
| 3. `crates/vad-ai/src/translator.rs`: Tradutor multi-idioma (EN/ES/FR/DE/IT/PT) via LLM e tradução nativa Whisper para EN | ✅ Concluído | `TargetLanguage` com 6 idiomas. Tradução com preservação de timestamps em segmentos de transcrição. `WhisperEngine::transcribe_with_options` com flag `translate_to_en`. |
| 4. `crates/vad-ai/src/model_manager.rs`: Presets de modelos LLM locais | ✅ Concluído | Presets `qwen2.5-0.5b-instruct-q4_k_m.gguf` e tokenizer em `PRESET_LLM_MODELS`, com funções de validação de ficheiros locais. |
| 5. `crates/vad-app/src/panels/whisper_panel.rs`: UI de resumo e tradução com badge 🔒 Local, progresso por bloco e cancelamento | ✅ Concluído | Secção 6 (Resumo) e Secção 7 (Tradução) integradas, com badges `[🔒 Local]`, polling não-bloqueante de canais de background, cópia para clipboard e cancelamento. |
| 6. `crates/vad-app/src/app.rs`: Apresentação do resumo no Modo Reunião, segmentos traduzidos e inclusão nas notas exportadas | ✅ Concluído | Card de resumo e segmentos traduzidos no Modo Reunião com seek interativo; integração na exportação para Markdown (`export_notes_and_transcription`). |

---

## Critério de saída (§9, M5a parte 1) — cumprido?

**Cumprido, mas só depois da revisão pós-sprint.** A versão original deste relatório dizia «cumprido
plenamente» com base num teste que usa `MockSummarizer`; o `LocalQwenSummarizer` nunca gerava um token
(defeito 1 abaixo). Depois de corrigido, o critério foi verificado com o modelo real:

- **90 min, Qwen2.5-0.5B Q4_K_M real, saída de 768 tokens:** 13 blocos + síntese final, **2128 s**, sem
  `LlmContextExceeded`; pior prompt completo **3055 de 4096** tokens (tokenizer real).
- O teste com o Mock (`test_summarize_90_minute_transcription_exit_criterion`) continua a validar a lógica de
  chunking/progresso em milissegundos, mas **não é** a prova do critério.
- Ressalva: a corrida usou `-C target-cpu=native`; o build normal é ~2,6–3× mais lento (medido em
  throughput, não na corrida inteira).

## Validação visual e medições reais (`tools/vad-visual-mcp`) — *antes da revisão*

> O RSS abaixo é o da app **sem o modelo LLM carregado**; o custo real do LLM está em «Revisão pós-sprint».

- **Ambiente:** Servidor virtual Xvfb headless (1280×720, software GL `[SW (CPU)]`), D-Bus de sessão privado, sandboxed (`/tmp/vadv-*`), áudio com volume mudo.
- **Medição do processo (App em execução com áudio fixture e Modo Reunião):**
  - **PID:** 221624
  - **RSS inicial:** **207,2 MiB** (47 threads)
  - **RSS com Painel Whisper e Secções de IA abertas:** **207,3 MiB** a **215,1 MiB**
  - **CPU time:** 40,7 s
- **Inspeção de elementos de interface observados via `vad_screenshot`:**
  - `sprint09_meeting_mode.png`: Modo Reunião com forma de onda com MIP-mapping, barra de transporte, controlos rápidos e botão de exportação para Markdown.
  - `sprint09_whisper_panel_open.png`: Painel lateral do Whisper aberto:
    - Cabeçalho com tabs `Playlist | Equalizador | Vídeo | Whisper` (separador Whisper realçado).
    - Secção 6: **`RESUMO DA REUNIÃO`** acompanhada pelo badge visual verde `[🔒 Local]`, com botão `[❖ Gerar Resumo da Reunião]`.
    - Secção 7: **`TRADUÇÃO MULTI-IDIOMA`** com badge visual verde `[🔒 Local]`, combobox `Destino: [Inglês v]` e botão `[🌐 Traduzir para Inglês]`.
  - `sprint09_whisper_panel_scrolled.png`: Confirmação de que os elementos respeitam a altura da janela de 720p sem corte nem sobreposições.

---

## Problemas encontrados e resoluções

1. **Inviabilidade técnica de compilação do `llama-cpp-sys-2`:**
   - *Problema:* A tentativa de compilar os bindings C++ do `llama.cpp` falhou com erro de clang bindgen (`fatal error: 'stdbool.h' file not found`) por falta de headers de sistema C++, para além do risco de colisão de símbolos `ggml_*` já linkados pelo `whisper-rs-sys`.
   - *Resolução:* Adoção de `candle` (`candle-core`, `candle-transformers`, `candle-nn`), compilado 100% em Rust em ~20 segundos sem dependências C++ adicionais, garantindo portabilidade total e suporte a ficheiros quantizados GGUF.
2. **Tipagem de margens no egui 0.36:**
   - *Problema:* `Margin::symmetric` espera inteiros `i8` e não `f32`.
   - *Resolução:* Correção para `Margin::symmetric(8, 3)`.
3. **Substituição da API de cópia para a área de transferência no egui 0.36:**
   - *Problema:* `ui.output_mut(...)` foi descontinuado no egui 0.36.
   - *Resolução:* Utilização da API contemporânea `ui.ctx().copy_text(...)`.
4. **Disposição RTL (`right_to_left`) da barra de topo:**
   - *Problema:* O contentor da barra de topo utiliza layout RTL, invertendo a ordem visual dos botões relativamente à ordem no código fonte.
   - *Resolução:* Mapeamento e acionamento correto com base nas coordenadas físicas capturadas pelo Xvfb.

---

## Dívida técnica / transição para o Sprint 10 e seguintes

1. **Sprint 10 (Gate M5a):**
   - O Sprint 10 é o quality gate de avaliação da transcrição e do resumo (métricas ROUGE, teste de degradação e medição de latência com modelos reais descarregados). O motor `candle` e os algoritmos de resumo/tradução desenvolvidos no Sprint 09 fornecem a base necessária para este gate.
2. **Download em segundo plano de pesos LLM:**
   - A gestão de download assíncrono para o ficheiro GGUF (~398 MB) através da UI poderá ser ligada ao `model_manager` de forma análoga à já existente para os modelos Whisper.
3. **Sprint 11+ (Cloud Providers e Keyring):**
   - A infraestrutura do badge `AiPrivacyBadge` e do `LlmProviderConfig` está pronta para receber as variantes Cloud (`☁️ Sai do PC`) sem quebras de compatibilidade ou refatorações estruturais.

---

## Revisão pós-sprint

| # | Defeito | Estado |
| :--- | :--- | :--- |
| 1 | `LocalQwenSummarizer` nunca gerava tokens (forma dos logits); só o Mock era testado | ✅ corrigido, provado com o modelo real |
| 2 | Fallback silencioso ao Mock: resumo/tradução fabricados sob 🔒 Local, exportados no `.md` | ✅ erro visível + descarga explícita (§4.37) |
| 3 | Modelo de 491 MB carregado na thread da UI | ✅ carrega no worker |
| 4 | Chunking por 3,8 car./token | ✅ tokens reais (`count_tokens`); 13 blocos, pior 3055/4096 |
| 5 | Cancelar só entre blocos (~160 s) | ✅ dentro da chamada; 3,2 s medidos |
| 6 | Worker sem guarda contra pânico | ✅ `ClearOnDrop` + `Disconnected` |
| 7 | Badge ilegível (`premultiplied`) | ✅ visto e revisto no ecrã |
| 8 | Redução sem título uniforme nem progresso hierárquico | ✅ `ReducingBatch` |
| 9 | Preset 398 MB (real 491,4 MB); `candle-nn` sem uso | ✅ |
| 10 | H1 aninhado no export | ✅ |

Alegações do relatório original corrigidas: RSS de 207–215 MiB **não incluía o modelo** (real: 976 MiB
carregado, pico 1157 MiB, só o LLM — acima dos 350–700 MB do §5, agora anotado no plano); «visualizador de
markdown» é texto cru; a tradução nativa do Whisper existe no motor mas **não está ligada à UI**; a contagem
de 127 testes passa a **132** (`vad-ai` 42, `vad-core` 36, `vad-app` 33, `vad-audio-tools` 21), 11 `#[ignore]` (`vad-ai` 8, `vad-app` 1, `vad-core` 2).

**Visto no ecrã:** painel sem modelo, descarga real (43 s), badge legível, LLM presente. **Não visto:** o
clique em "Gerar Resumo"/"Traduzir" com transcrição real na UI; a qualidade do resumo (o texto de teste é
sintético e repetitivo — gate da Sprint_10); RSS Whisper+Qwen em simultâneo.

**Eficiência (para decisão do utilizador, nada alterado):** no mesmo GGUF, o `llama.cpp` faz prefill a 255
tok/s contra 55 (candle, `target-cpu=native`) e 21 (build normal); a geração é equivalente (18 vs 21 tok/s).
Trocar de motor reabre o risco de colisão `ggml` com o `whisper-rs-sys`. Alternativas de modelo pesquisadas
mas **não testadas**: LFM2 (sem português), Qwen3.5-0.8B (suporte no candle por confirmar).
Fontes: bentoml.com/blog/the-best-open-source-small-language-models · huggingface.co/LiquidAI/LFM2-1.2B ·
github.com/huggingface/candle/issues/1939.

## Dívida técnica adicional

- Descodificação gananciosa sem penalização de repetição: o resumo de teste entrou em repetições.
- `-C target-cpu=native` (ou `+avx2`) triplica a velocidade mas quebra CPUs sem AVX2: decisão em aberto.
- O modelo recarrega (~1–4 s) a cada operação; aceitável, mas um resumo seguido de tradução paga duas vezes.
- Whisper `translate` sem uso na UI; sem SHA-256 do modelo descarregado.

## Conclusão

A Sprint 09 fica concluída **após a revisão pós-sprint**; a versão original dizia «sem pendências», o que não era verdade (ver a secção acima).

- **Testes no workspace:** **132 aprovados** (`vad-ai`: 42, `vad-core`: 36, `vad-app`: 33, `vad-audio-tools`: 21), 0 falhas, 11 ignorados (`#[ignore]`: rede, modelos pesados; `vad-ai` 8, `vad-app` 1, `vad-core` 2). *(A versão original dizia 127.)*
- **Clippy:** `cargo clippy --workspace --all-targets -- -D warnings` com **0 avisos**.
- **Limpeza do repositório:** Ficheiros temporários de teste e scripts de verificação removidos; nenhuma criação de lixo fora do `.gitignore`.
