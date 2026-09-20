# Sprint_06 — Diário de bordo

**Milestone:** M3 (parte 1) — Extração de PCM, waveform, model_manager, whisper.rs
**Planning:** ver `Sprint_Planning_06.md`
**Início:** 2026-09-20
**Fim:** 2026-09-20

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-20

- Início da implementação do Sprint 06 (Milestone M3, parte 1).
- Validação prévia de ambiente: `cmake 4.4.3` instalado em `~/.local/bin/cmake` para compilação FFI do `whisper-rs-sys`.
- Reconhecimento de hardware para parametrização de threads do Whisper: CPU Intel Core i5-1335U com 10 núcleos físicos (2P + 8E) e 12 tópicos lógicos.
- Estruturação dos componentes por dependência: `vad-core` (erros, caminhos e detetor de vídeo), `vad-ai` (`extractor`, `model_manager`, `whisper`), `vad-audio-tools` (`waveform_pyramid`) e `vad-app` (`whisper_panel`, top bar indicator, mini-player de transporte no Modo Reunião).
- `vad-core`: adicionada função `vad_models_dir()` em `util.rs` apontando para `~/.local/share/vad/models/` sem novas dependências externas; adicionadas variantes `Whisper(String)` e `WhisperCallbackPanic(String)` em `VadError` com respetivo mapeamento em `ErrorAction`; implementado `Player::has_video(&self) -> bool` para deteção de faixa de vídeo em reprodução.
- `vad-ai/src/extractor.rs`: implementado `AudioExtractor` assíncrono com subprocesso `ffmpeg` para PCM 16kHz mono `i16` (§4.12); cap de 4 horas de duração em RAM (`MAX_AUDIO_DURATION_SECS = 14400.0`, ~460MB) com flag `is_truncated = true` e aviso (§4.12); cache em memória `cache: Arc<Mutex<HashMap<String, PcmAudio>>>` com zero ficheiros em disco (§4.18); cancelamento ativo via `ExtractionHandle` que termina o processo filho via `SIGKILL` sem deixar processos órfãos (§4.17); monitorização de `ExitStatus` do filho convertendo saída anormal ou sinal OOM em `VadError::ExtractionFailed` (§4.31). 7 testes unitários criados e aprovados.
- `vad-audio-tools/src/waveform_pyramid.rs`: implementada estrutura `WaveformPyramid` com representação min/max por pixel (`MinMaxPoint`), nível global pré-computado ao carregar (`global_level` de 1000 pontos) e cálculo sob demanda para janelas de zoom (5 min e 10s) sem atraso na abertura do ficheiro (§5); renderização limitada ao teto de 1000 pontos visíveis em lote único; 3 testes unitários criados e aprovados.
- `vad-ai/src/model_manager.rs`: implementado `ModelManager` com suporte aos modos `Disk` (persistente em `~/.local/share/vad/models/` com escrita atómica `.tmp`+`rename`, carregado via mmap) e `RamOnly` (memória volátil sem tocar no disco); tooltips autoritativos exatos do §4.13 (`RAM_ONLY_TOOLTIP` e `DISK_TOOLTIP`); tabela de presets oficiais (`tiny`, `base-q5`, `base`, `small`, `medium`); validação automática de zero ficheiros no disco em RAM-only (§10.3) e de reutilização de ficheiro sem novo download em modo disco (§10.4); 5 testes unitários criados e aprovados.
- `vad-ai/src/whisper.rs`: implementado `WhisperEngine` sobre `whisper-rs`; buffer em RAM-only retido como `Arc<[u8]>` fixo no `WhisperEngine` prevenindo use-after-free (§4.11); carregamento em disco por path/mmap; trampolines C para progresso e abort envolvidos em `std::panic::catch_unwind` prevenindo que panics em Rust provoquem aborto do processo (§4.24); threads fixadas por omissão em `num_cpus::get_physical()` (10 núcleos físicos no i5-1335U vs 12 lógicos) (§5); método `transcribe()` com extração de segmentos e timestamps (em centissegundos convertidos para ms); método de exportação para Markdown `export_to_markdown()`; 4 testes unitários criados e aprovados (total de 16 testes em `vad-ai`).
- `vad-app/src/panels/whisper_panel.rs`: implementado painel lateral com seleção de modelo predefinido (`base-q5` por omissão), botões de rádio Disco/RAM-only com tooltips exatos do §4.13 (`DISK_TOOLTIP` e `RAM_ONLY_TOOLTIP`), descarregamento assíncrono com barra de progresso, gestão de modelos em disco (ativar/eliminar), botão de transcrição com progresso percentual, lista de segmentos com timestamp clicável para seek instantâneo na reprodução, e exportação para ficheiro Markdown.
- Indicador assíncrono na barra superior (Task 5 / §4.17): implementado indicador `🎙 A indexar áudio: X% [✕]`. Nunca bloqueia a interface do utilizador nem interrompe reprodução de vídeo/áudio em curso. O botão `[✕]` cancela a extração e elimina o subprocesso filho via `SIGKILL` sem órfãos.
- Modo Reunião (Task 6 / §6): implementada visualização central para ficheiros de áudio ou vídeo de conferência (`render_meeting_mode`):
  - Forma de onda renderizada através de `WaveformPyramid` respeitando o teto de ~1000 pontos visíveis por lote único de desenho (§5).
  - Zoom de forma de onda via roda do rato (`smooth_scroll_delta.y`) com níveis de detalhe (5 min, 10s) e retorno suave ao nível global completo (§5).
  - Playhead amarelo/dourado com cursor `[▲]` diretamente clicável e arrastável sobre a forma de onda para seek de alta precisão (§6).
  - Mini-player de transporte logo abaixo da forma de onda: botões `[⏪ 5s]`, `[▶ / ⏸]`, `[5s ⏩]`, tempo decorrido/duração (`HH:MM:SS / HH:MM:SS`), seletor de velocidade (`0.5x` a `2.0x`), controlo deslizante de volume (`0%` a `150%`), e botão de reset de zoom (§6).
  - Seção de controlos rápidos de reunião e pré-visualização dos segmentos da transcrição do Whisper.
- `vad-app`: adicionados testes unitários para transições de estado do painel lateral Whisper, cálculos de zoom de waveform e tolerância de limites, e configurações do painel. Total de 15 testes aprovados em `vad-app` (60 testes no workspace completo).

## Desvios face ao Sprint_Planning_06.md

(nenhum desvio de âmbito)

## Problemas encontrados

1. **Posicionamento do delimitador `--` no CLI do FFmpeg:**
   - *Problema:* O planeamento referia passar `--` antes do caminho do ficheiro (`-i -- <caminho>`). No analisador de argumentos do FFmpeg, a opção `-i` consome imediatamente o argumento seguinte como o URL de entrada; ao receber `--`, o FFmpeg tentava abrir um ficheiro literal de nome `--` e falhava com código 254 (`Error opening input file --: No such file or directory`).
   - *Resolução:* Implementou-se `sanitize_input_path(path)` que sanitiza ficheiros relativos começados por `-` prefixando `./` (ex.: `./-video.mp4`), impedindo que sejam interpretados como opções pelo CLI; o delimitador `--` foi colocado no final das opções, antes do URL de saída (`-- pipe:1`), onde é aceite e válido no parser do FFmpeg. Testado e validado com sucesso com ficheiros com hífen.

2. **Segurança de panics em callbacks C de progresso e abort no Whisper:**
   - *Problema:* A biblioteca `whisper-rs` em `set_progress_callback_safe` não envolvia o closure Rust em `std::panic::catch_unwind`. Um panic eventual dentro de um callback chamado pelo C da biblioteca nativa resultaria em abort instantâneo do processo (§4.24), contornando a estratégia de tolerância a falhas do VAD.
   - *Resolução:* Implementaram-se trampolines C manuais `safe_progress_trampoline` e `safe_abort_trampoline` protegidos com `std::panic::catch_unwind(AssertUnwindSafe(...))`, garantindo que qualquer panic em Rust devolva controle seguro ao whisper.cpp para abortar a inferência sem crashear o binário (§4.24). Testado com asserções dedicadas.

3. **Compatibilidade com APIs do egui 0.36:**
   - *Problema:* `egui::Frame::none()` foi descontinuado na versão 0.36 e o acesso a deltas de rotação do rato migrou de `raw_scroll_delta` para `smooth_scroll_delta`.
   - *Resolução:* Ajustado para `egui::Frame::new()` e `ui.input(|i| i.smooth_scroll_delta.y)`.


