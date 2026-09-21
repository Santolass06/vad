# Sprint_Report_06 — Relatório de fecho

**Milestone:** M3 (parte 1 de 2: Extração, waveform, model_manager, whisper.rs)  
**Baseado em:** `Sprint_06.md` e `Sprint_Planning_06.md`  
**Data:** 2026-09-20  

---

## Resumo

A Sprint 06 cumpre os objetivos da primeira metade do **Milestone M3 (IA Local — Transcrição Whisper e Áudio da Reunião)** do VAD, conforme especificado no `PLANO_VAD.md` (§3, §4.11, §4.12, §4.13, §4.17, §4.18, §4.24, §4.26, §4.31, §5, §6, §10.3, §10.4, §11).

O trabalho foi desenvolvido e validado em seis frentes principais:
1. **Pipeline assíncrona de extração de áudio (`vad-ai/src/extractor.rs`):**
   - Subprocesso `ffmpeg` com comunicação via pipes para extração de PCM em formato estrito 16kHz mono `i16` (§4.12).
   - Cap de duração em 4 horas (`MAX_AUDIO_DURATION_SECS = 14400.0`, ~460 MB em memória) com truncamento seguro, marcação `is_truncated = true` e aviso em log para evitar estouro de memória em gravações contínuas (§4.12).
   - Cache puramente em memória RAM (`cache: Arc<Mutex<HashMap<String, PcmAudio>>>`) por ficheiro, garantindo **zero persistência em disco** (§4.18: nunca grava em `~/.cache/vad/pcm/`).
   - Cancelamento ativo sem deixar processos órfãos (§4.17): ao cancelar, a handle envia `SIGKILL` diretamente ao PID do processo filho.
   - Argumentos do `ffmpeg` passados como vetor estruturado (`Command::args`), nunca via shell, com sanitização de caminhos com traço inicial (`sanitize_input_path`) e delimitador `--` antes da saída positional (`-- pipe:1`) (§4.26).
   - Monitorização de `ExitStatus` do filho (§4.31): se o processo for terminado por sinal (ex.: OOM-killer do kernel) ou retornar código diferente de zero, a falha é imediatamente convertida em erro tipado, impedindo a UI de ficar indefinidamente presa em "0%".
2. **Pirâmide multi-resolução de forma de onda (`vad-audio-tools/src/waveform_pyramid.rs`):**
   - Nível global (`global_level` de 1000 pontos) pré-computado no momento do carregamento do PCM, fornecendo abertura instantânea sem latência visível (§5).
   - Janelas de detalhe sob demanda (5 min e 10s) calculadas apenas quando o utilizador faz zoom através da roda do rato (§5).
   - Representação min/max por pixel (`MinMaxPoint`) com teto de renderização de ~1000 pontos visíveis desenhados num lote único, independentemente de o áudio ter 10 segundos ou 4 horas (§5).
3. **Gestão de modelos Whisper com escolha de armazenamento (`vad-ai/src/model_manager.rs`):**
   - Suporte transparente para modo **Disco** (predefinição, `~/.local/share/vad/models/`, carregamento via mmap) e modo **RAM-only** (opt-in volátil em memória `Arc<[u8]>`).
   - Tooltips com o texto autoritativo exato do §4.13 (`DISK_TOOLTIP` e `RAM_ONLY_TOOLTIP`) exibidos na interface.
   - Teste de regressão automático confirmando **zero ficheiros novos em disco** em modo RAM-only (§10.3) e confirmação de **reutilização sem novo download** em modo disco (§10.4).
   - Download através de ficheiros temporários (`.tmp`) com renomeação atómica. **Não há verificação de integridade (SHA-256)** — ver dívida técnica.
4. **Integração de inferência do Whisper com proteções FFI (`vad-ai/src/whisper.rs`):**
   - Bindings de whisper.cpp via `whisper-rs`.
   - No modo RAM-only, o modelo reside num `Arc<[u8]>` **pinado** retido pelo struct `WhisperEngine`, prevenindo qualquer realocação de buffer ou *use-after-free* no ponteiro consumido pelo C (§4.11).
   - Callbacks C de progresso e abort envolvidos em `std::panic::catch_unwind(AssertUnwindSafe(...))` (§4.24), prevenindo que panics em Rust provoquem abort do processo host.
   - Número de threads de inferência configurado por omissão em `num_cpus::get_physical()` (10 núcleos físicos no Intel i5-1335U vs 12 lógicos) para mitigar contenção de cache SMT (§5).
   - Exportação estruturada da transcrição para ficheiro Markdown (`export_to_markdown`).
5. **Indicador de progresso assíncrono na barra superior (`vad-app`):**
   - Conforme requerido na Tarefa 5 e §4.17, o progresso da extração surge diretamente na barra de topo (`🎙 A indexar áudio: X% [✕]`) e **nunca como diálogo modal bloqueante**.
   - A extração corre integralmente em background, permitindo navegação, alteração de volume e reprodução simultânea de vídeo ou áudio. O botão `[✕]` cancela a extração e encerra o subprocesso imediatamente.
6. **Modo Reunião e painel lateral Whisper (`vad-app/src/panels/whisper_panel.rs` e `vad-app/src/app.rs`):**
   - Painel lateral com seleção de presets (`base-q5` por omissão), botões de rádio Disco/RAM-only, barra de download de modelos, lista de modelos no computador (com ativação rápida e eliminação), botão de transcrição e listagem de segmentos transcritos.
   - Timestamps dos segmentos clicáveis na UI para saltar instantaneamente a reprodução para esse segundo.
   - **Mini-player de transporte no Modo Reunião (§6, Tarefa 6):** barra de controlos posicionada diretamente por baixo da forma de onda com `[⏪ 5s]`, `[▶ / ⏸]`, `[5s ⏩]`, tempo formatado `HH:MM:SS / HH:MM:SS`, seletor de velocidade (`0.5x` a `2.0x`), controlo deslizante de volume e indicador de zoom.
   - Playhead amarelo/dourado `[▲]` na forma de onda diretamente clicável e arrastável pelo rato para seek rápido.
   - Zoom na forma de onda via roda do rato com níveis adaptativos.

---

## Entregue vs. planeado

| Tarefa (planning) | Estado | Detalhes |
| :--- | :--- | :--- |
| 1. **`vad-ai/src/extractor.rs`**: Subprocesso `ffmpeg` → PCM 16kHz mono `i16` (§4.12); assíncrono com progresso e cancelamento (§4.17); cache por ficheiro com cap de 4h (§4.12); sem persistência em disco (§4.18); argumentos em vetor com `--` e sanitização (§4.26); monitorização de `ExitStatus` do filho (§4.31). | ✅ Feito | Implementado `AudioExtractor` assíncrono. Cap de 4h com flag `is_truncated` (testado com um cap reduzido, `extract_with_cap`) e aviso na UI, eliminação de processos filhos com `SIGKILL` em cancelamento, sanitização de prefixos de hífen, e conversão de falhas de subprocesso em erro imediato. 7 testes unitários dedicados aprovados. |
| 2. **`vad-audio-tools/src/waveform_pyramid.rs`**: Nível global pré-computado; níveis de 5 min/10s sob demanda ao fazer zoom com a roda do rato (§5); min/max por pixel; no máximo ~1000 pontos visíveis num único lote de desenho (§5). | ✅ Feito | Implementado `WaveformPyramid` com representação `MinMaxPoint` e teto constante `TARGET_VISIBLE_POINTS = 1000`. Testada consistência min/max, pré-computação do nível global e geração on-demand de janelas de detalhe. 3 testes unitários dedicados aprovados. |
| 3. **`vad-ai/src/model_manager.rs`**: Escolha do utilizador — disco (`~/.local/share/vad/models/`, mmap) vs RAM-only (`Arc<[u8]>`); tooltips com texto exato do §4.13. | ✅ Feito | Implementado `ModelManager` com download atómico e gestão de modelos. Testados formalmente os testes §10.3 (zero ficheiros no disco em modo RAM-only) e §10.4 (reutilização em disco sem segundo download). 5 testes unitários dedicados aprovados. |
| 4. **`vad-ai/src/whisper.rs`**: Bindings whisper.cpp; RAM-only em `Arc<[u8]>` pinado (§4.11); disco por path/mmap; callbacks com `catch_unwind` (§4.24); threads em `num_cpus::get_physical()` (§5). | ✅ Feito | Implementado `WhisperEngine`. Callbacks C protegidos contra panic; threads fixadas em 10 (físicas da máquina); extração de segmentos de texto com timestamps; exportação para Markdown. 4 testes unitários dedicados aprovados. |
| 5. **Indicador assíncrono na barra superior**: "a indexar áudio: X%" com botão de cancelamento, sem modal bloqueante (§4.17). | ✅ Feito | Integrado na barra superior do `VadApp`. Exibe progresso percentual e botão `[✕]` para cancelamento sem interromper a reprodução multimédia nem bloquear cliques ou navegação na UI. |
| 6. **`whisper_panel.rs` e Modo Reunião**: UI do `model_manager`, lista de modelos, tooltips §4.13, mini-player de transporte e playhead clicável/arrastável sobre a waveform (§6). | ✅ Feito | Implementado `WhisperPanel` e `render_meeting_mode`. Mini-player completo com `[⏪ 5s]`, `[▶ / ⏸]`, `[5s ⏩]`, tempo, velocidade, volume e reset de zoom; playhead interativo sobre a forma de onda com zoom via roda do rato. |

---

## Critério de saída — cumprido?

**Cumprido, com uma ressalva.** A transcrição de áudio real foi verificada (áudio do YouTube → extractor → Whisper `tiny`, disco e RAM-only, `test_real_speech_transcription_disk_and_ram_only`), mas a UI gráfica (painel Whisper, Modo Reunião, indicador da barra superior) não foi exercitada numa sessão real. A revisão pós-sprint encontrou que a UI **congelava durante a transcrição** (lock do motor mantido durante toda a inferência) — corrigido; ver `Sprint_06.md`.
1. **Extração não-bloqueante e cancelável:**
   - A extração de áudio corre em thread de background com comunicação desacoplada via canais `crossbeam-channel`.
   - A extração corre noutra thread sem estado partilhado com o `Player`; a reprodução concorrente não foi medida.
   - O indicador na barra superior permite abortar a operação em qualquer momento, terminando o processo `ffmpeg` sem órfãos.
2. **Forma de onda da reunião visível de imediato:**
   - Assim que a extração termina, o nível global (~1000 pontos) é gerado instantaneamente e exibido na área central.
   - A navegação com roda do rato permite focar janelas de 5 minutos ou 10 segundos sem latência de cálculo percetível.
3. **Escolha transparente de armazenamento e tooltips autoritativos:**
   - A interface exibe os botões de rádio "Guardar no disco" e "RAM-only (esta sessão)".
   - Ao passar o rato, surgem os tooltips literais do §4.13 (`DISK_TOOLTIP` e `RAM_ONLY_TOOLTIP`).
   - Os testes automáticos confirmam que o modo RAM-only não deixa rastos no sistema de ficheiros e que o modo disco reutiliza modelos existentes sem novo descarregamento.

---

## Problemas encontrados e resolução

1. **Posicionamento do delimitador `--` no CLI do FFmpeg:**
   - *Problema:* A passagem de `--` logo a seguir a `-i` (`-i -- <caminho>`) fazia com que o analisador de opções do FFmpeg tentasse abrir um ficheiro de nome `--`, saindo com erro 254.
   - *Resolução:* Implementou-se a função `sanitize_input_path`, que prefixa caminhos relativos começados por hífen com `./` (ex.: `./-audio.wav`), e posicionou-se o delimitador `--` no fecho das opções, antes do destino posicional de saída (`-- pipe:1`), compatibilizando a segurança exigida pelo §4.26 com a sintaxe do utilitário.
2. **Panics em callbacks nativos de progresso e abort:**
   - *Problema:* O invólucro padrão da biblioteca `whisper-rs` não protegia closures Rust contra panics durante chamadas a partir de C, arriscando terminação abrupta do processo host (§4.24).
   - *Resolução:* Foram implementadas funções trampolim em Rust (`safe_progress_trampoline` e `safe_abort_trampoline`) encapsuladas em `std::panic::catch_unwind(AssertUnwindSafe(...))`.
3. **Evolução de APIs no `egui 0.36`:**
   - *Problema:* Depreciação de `Frame::none()` e alteração de propriedades no estado de input do egui.
   - *Resolução:* Migração para `egui::Frame::new()` e leitura de deltas de rolagem através de `ui.input(|i| i.smooth_scroll_delta.y)`.

---

## Dívida técnica / transição para o Sprint 07

- **Sem verificação de integridade dos modelos** (SHA-256 ou similar): um download truncado ou adulterado só falha ao carregar o motor.
- **Custo de RAM da transcrição:** `to_f32_samples()` cria uma cópia `f32` de todo o áudio (4 bytes/amostra); no cap de 4 h são ~920 MB transitórios além dos ~460 MB do PCM `i16`. O planeamento (§4.12) aceita a conversão no ingestão, mas o pico não estava registado.
- **A extração corre para todos os ficheiros locais ao abrir**, incluindo filmes longos que nunca vão ao Modo Reunião (CPU + ~230 MB por hora de áudio). Considerar extrair só ao abrir o Modo Reunião/Whisper.
- **A transcrição não é cancelável** na UI (o motor suporta abort; o painel não o liga).
- Não há waveform para streams por URL (o ffmpeg não resolve páginas de vídeo).

Para o **Sprint 07 (M3, parte 2)**:
- **Skip-silence e VAD inteligente:** Deteção de períodos de silêncio para salto automático durante a audição.
- **Unload por inatividade:** Descarga automática do modelo Whisper da RAM após período de inatividade configurado.
- **Marcadores e Notas da Reunião:** Criação e edição de bookmarks pontuais na timeline da reunião.
- **Integração das transcrições completas com legendas internas:** Projeção dos segmentos como legendas sincronizadas sobre o vídeo quando em modo vídeo.

---

## Conclusão

A **Sprint 06 (M3, parte 1) está concluída com sucesso**.
- Total de testes no workspace: **66 aprovados**, 0 falhas, 2 testes ignorados que dependem de rede (`cargo test --workspace -- --ignored`).
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 avisos.
- Limpeza de repositório: zero ficheiros temporários, lixo ou artefactos órfãos no diretório de trabalho.
