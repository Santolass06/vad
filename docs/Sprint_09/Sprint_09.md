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

## Revisão pós-sprint (2026-09-21/22)

Esta entrada **corrige** o registo acima (que não é apagado). O trabalho original foi validado só com o
`MockSummarizer` e o modelo real nunca foi carregado; o que segue vem de o correr a sério (Qwen2.5-0.5B
Q4_K_M, 491 400 032 B, sha256 `74a4da8c…a9db`, descarregado do Hugging Face para o scratchpad).

### Defeitos encontrados e corrigidos

1. **`LocalQwenSummarizer` nunca gerou um token.** O `forward` do candle devolve `(1, vocab)` e o código fazia
   `.get(dim-1)`, reduzindo a um escalar: `argmax: dimension index 0 out of range for shape []`. Reproduzido
   com o modelo real antes de corrigir; depois: "Qual é a capital de França?" → "Paris".
2. **Fallback silencioso para o Mock** (modelo em falta ou a falhar a carregar): a UI mostrava um resumo/
   tradução fabricados ("Síntese consolidada…", "[English from Português]: …") sob o badge 🔒 Local e
   exportava-os para o `.md`. Contradiz §4.21. Agora é erro visível e a UI oferece a descarga explícita
   (§4.37). Apurado por leitura de código; o comportamento antigo não foi reproduzido na UI.
3. **Modelo (~491 MB) carregado na thread da UI** dentro de `start_summary`/`start_translation` — congelamento.
   Apurado por leitura de código. Agora carrega no worker.
4. **Chunking por heurística de 3,8 caracteres/token.** Com o tokenizer real o texto sintético dá ~3,0
   caracteres/token (bloco de 6 min = 2324 tokens). Não provei que a heurística estourasse a janela (a margem
   era ~2%); passou a orçamentar por tokens reais (`Summarizer::count_tokens`). 90 min → 13 blocos, pior
   prompt completo **3055 de 4096** tokens.
5. **Cancelar só entre blocos** (cada bloco demora ~160 s): o §4.20 pede que cancelar seja significativo.
   Agora o abort é verificado por token e o prefill corre em fatias de 128 tokens: **3,2 s** medidos com
   prefill de ~2000 tokens.
6. **Worker sem guarda:** um pânico deixava `is_summarizing` a `true` (spinner eterno) e `Disconnected` era
   ignorado. `ClearOnDrop` + tratamento de `Disconnected`, com teste.
7. **Badge 🔒 Local ilegível** (`from_rgba_premultiplied` com valores não pré-multiplicados, o mesmo erro da
   Sprint_08): visto em `vad_screenshot`, corrigido e revisto (`from_rgba_unmultiplied`).
8. **Redução:** o caminho de síntese final não levava título (só os outros levavam); as passagens
   hierárquicas não reportavam progresso (a UI ficava em "bloco 13 de 13"); cancelar não era verificado entre
   lotes. Novo `ReducingBatch`, título unificado, testes.
9. **Preset com tamanho errado** (~398 MB; o real é 491,4 MB) e dependência `candle-nn` sem uso (removida).
10. **Export:** o resumo trazia um H1 aninhado dentro de "## Resumo da Reunião".

### Alegações do relatório original que estavam erradas

- «Critério de saída cumprido plenamente… 13.500 palavras (~18.000 tokens), 2 blocos»: o teste usa 1350
  segmentos, contexto 1024 e `MockSummarizer`; não fala com o modelo. Corrido a sério: ver abaixo.
- «RSS 207–215 MiB com painel de IA ativo»: o modelo **nunca estava carregado**; não é o custo do M5a.
- «Visualizador de markdown»: é texto cru. «Tradução nativa Whisper integrada»: existe no motor
  (`transcribe_with_options`), **sem uso na UI nem teste**.

### Medições reais (candle 0.11, i5-1335U, 12 threads)

- **90 min, modelo real, saída 768 tokens (a da app):** 13 blocos, síntese final, **2128 s (35,5 min)**, sem
  estourar a janela; RSS: modelo carregado **976 MiB**, pico **1157 MiB** (só o LLM; Whisper não carregado).
  Corrida com `-C target-cpu=native`.
- **Velocidade** (prompt de 1015 tokens): build normal, prefill 21 tok/s e geração 7,4 tok/s; com
  `target-cpu=native`, 55 e 21,6 tok/s. Não alterei as flags (SIGILL em CPUs sem AVX2): decisão do utilizador.
- **`llama.cpp` b11081 (binário oficial, CPU), mesmo GGUF, 12 threads:** prefill **255 tok/s** (pp1024) e
  geração 18 tok/s (tg64). O candle é ~4,6× (native) a ~12× mais lento no prefill; a geração é equivalente.
  Não troquei de motor (risco de colisão de símbolos `ggml` com o `whisper-rs-sys`, ver a investigação acima).
- **Tradução real, 10 segmentos PT→EN** (`#[ignore]`): timestamps preservados, 0 segmentos devolvidos por
  traduzir, ~24 s. Um erro de qualidade visível ("latência" → "lateness") — é o assunto do gate da Sprint_10.
- **Pesquisa de modelos alternativos** (fontes no relatório): LFM2 não lista português; Qwen3.5-0.8B existe
  mas não confirmei suporte no candle 0.11 (que tem `quantized_qwen3`, `gemma3`, `phi3`, `lfm2`, `llama`);
  os de 3–4B são maiores. Nenhum foi testado.

### Visto no ecrã (`vad_screenshot`, Xvfb)

Painel Whisper sem modelo LLM (botão "Descarregar modelo de linguagem (~498 MB)", resumo/tradução
desativados); clique real na descarga (0 % → concluído em 43 s, ficheiros com os tamanhos exatos, botão
desaparece); badge legível; com o LLM presente e o Whisper ativado pela UI.

### Não visto / não medido

- O clique em "Gerar Resumo"/"Traduzir" com transcrição real **na UI** (só o motor e o `MapReduce`
  foram corridos com o modelo real, por testes `#[ignore]`).
- Qualidade do resumo: a transcrição sintética repete 6 frases e o resumo saiu repetitivo (descodificação
  gananciosa, sem penalização de repetição). Não é avaliação; o gate é a Sprint_10.
- RSS de Whisper + Qwen em simultâneo; a corrida de 90 min com o build normal (só a velocidade foi medida).

## Revisão pós-revisão (2026-09-22)

O utilizador perguntou explicitamente por correções simples e não previstas que tivessem ficado por fazer
na revisão anterior. Achei três e apliquei-as de imediato:

1. **`from_rgba_premultiplied` mal usado nos outros ~48 sítios do workspace** (só o badge e o painel de
   clip tinham sido corrigidos): `audio_panel.rs`, `playlist_panel.rs`, `video_panel.rs`, `hud.rs`,
   `whisper_panel.rs`, `app.rs`. Substituído por `from_rgba_unmultiplied` em todos. Revisto em
   `vad_screenshot`: playlist, equalizador e painel de corte de clip, ilegíveis antes (fundos em cores
   quase opacas), legíveis depois — sem regressão visual nos outros painéis já corrigidos.
2. **Download de modelo truncado não era detetado.** `download_to_file`/`download_to_memory` paravam de
   copiar quando `read()` devolvia 0 bytes, sem comparar com o `Content-Length` anunciado; um servidor ou
   proxy que fechasse a ligação a meio (sem erro de I/O explícito) deixava passar um `.tmp`→rename como se
   o modelo estivesse completo, e só falhava mais tarde, ao carregar. Adicionado `check_complete` (compara
   bytes recebidos com `Content-Length` quando este é conhecido) e um teste com servidor HTTP local que
   anuncia mais bytes do que envia (`test_truncated_download_is_rejected_not_saved_as_complete`): confirma
   erro e zero ficheiros deixados (nem `.tmp`).
3. **Penalização de repetição: investigada, não aplicada.** Testei com o modelo real (1.1 sobre os últimos
   64 tokens, via `candle_transformers::utils::apply_repeat_penalty`) numa transcrição de reunião realista.
   Resultado: a fração de 4-gramas distintos melhorou de 0,98 para 1,00 (deixou de repetir "Rui e Rui..."),
   mas o resumo **perdeu a data da decisão** ("22 de outubro") que sobrevivia sem penalização. Para um
   resumo de reunião, um facto omitido é pior do que uma repetição benigna, por isso não fica ativada; ficou
   documentado num teste real (`test_real_qwen_summary_of_realistic_text_does_not_loop`) que serve de
   regressão ao comportamento atual (gananciosa, sem penalização) e ao raciocínio da decisão.

Não alterei `target-cpu`: medi `x86-64-v3` vs `native` de novo, desta vez com cuidado para não deixar
processos de medição anteriores a correr em paralelo (o que já tinha estragado uma leitura, com "decode"
negativo — CPU deste portátil não tem AVX-512, por isso os dois ficam parecidos: prefill 67 tok/s (v3) vs
62 tok/s (native), decode 9,6 vs 5,2 tok/s, 1015 tokens de prompt, medição única). Continua a ser decisão
do utilizador, por definir o requisito mínimo de CPU da distribuição.

Testes: 133 passados (vad-ai 43, vad-core 36, vad-app 33, vad-audio-tools 21), 12 ignorados. Clippy limpo.
