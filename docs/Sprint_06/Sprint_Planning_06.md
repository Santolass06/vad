# Sprint_Planning_06 — M3 (parte 1): Extração, waveform, model_manager, whisper.rs

**Milestone:** M3, parte 1 de 2 (`PLANO_VAD.md` §9/§11)
**Pré-requisito:** Sprint_05 fechada (M2 completo).

## Objetivo

Pipeline de extração de áudio + waveform + escolha de armazenamento do modelo Whisper
+ transcrição básica.

## Tarefas

1. `vad-ai/src/extractor.rs`: subprocesso `ffmpeg` → PCM 16kHz mono **i16** (não
   `f32`, §4.12), assíncrono com progresso e cancelamento (§4.17 — mata o subprocesso
   ao cancelar, não deixa órfão), **cache por ficheiro** com cap de duração (ex. 4h,
   §4.12), **sem persistência em disco** (§4.18 — nunca gravar em
   `~/.cache/vad/pcm/`).
2. `vad-audio-tools/src/waveform_pyramid.rs`: nível global pré-computado ao abrir o
   ficheiro; níveis de 5 min/10s calculados sob demanda ao dar zoom (§5); armazenamento
   min/max por pixel; UI renderiza no máximo ~1000 pontos visíveis, independente da
   duração do ficheiro. **Esta peça pertence a esta sprint, não a M4** — partilha a
   cache de PCM do `extractor.rs` (§4.12) e o mockup de reunião do §6 já assume a
   waveform pronta.
3. `vad-ai/src/model_manager.rs`: escolha do utilizador — **disco** (predefinição,
   `~/.local/share/vad/models/`, carregado por `mmap`) vs **RAM-only** (opt-in,
   `Arc<[u8]>`) — tooltips com o texto exato do §4.13 ao passar o rato.
4. `vad-ai/src/whisper.rs`: bindings whisper.cpp. Caminho RAM-only carrega para
   `Arc<[u8]>` **pinado** (nunca `Vec<u8>` — §4.11: `whisper_init_from_buffer` não
   copia o modelo, um `Vec` realocado seria use-after-free real, não excesso de
   cautela); caminho disco carrega por path/`mmap`. **Os callbacks de
   progresso/abort passados ao whisper.cpp (Rust chamado de volta pelo C) vão
   envolvidos em `std::panic::catch_unwind`** (§4.24) — pela mesma razão do callback
   de update do mpv na Sprint_01: um panic sem guarda aqui aborta o processo, anulando
   a decisão do §5 de não usar `panic="abort"`.
5. Diálogo "A extrair áudio" com progresso (design `Dialogs.dc.html`).
6. `vad-app/src/panels/whisper_panel.rs`: UI do `model_manager` (lista de modelos no
   disco, radio disco/RAM-only, tooltips) — design `Meeting.dc.html`.

## Fora de âmbito

Skip-silence, unload por inatividade, bookmarks — Sprint_07.

## Critério de saída

Transcrição de um ficheiro de reunião real sem congelar a UI durante a extração nem
interromper outra reprodução em curso; waveform da reunião inteira visível assim que
o ficheiro abre; utilizador escolhe disco/RAM-only e vê o tooltip antes de descarregar.

## Testes a preparar (§10.3/§10.4)

- RAM-only: confirmar **zero ficheiros novos** em `~/.local/share/vad`, `~/.cache` e
  `/tmp` depois de carregar um modelo.
- Modo disco: confirmar que o ficheiro é criado em `~/.local/share/vad/models/` e
  **reutilizado sem novo download** na ativação seguinte.

## Risco conhecido

A cache de PCM em `i16` ainda assim custa ~115MB/hora — confirmar que o cap de
duração está mesmo a truncar com aviso, não a crescer sem limite numa gravação longa.
