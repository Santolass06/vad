# Sprint_Planning_02 — M1 (parte 1): HUD, CLI, drag-and-drop, error.rs

**Milestone:** M1, parte 1 de 2 (`PLANO_VAD.md` §9/§11)
**Pré-requisito:** gate M0.5 (Sprint_01) validado — confirmar SIM no `Sprint_Report_01.md`.

## Objetivo

HUD completo de reprodução, entrada por CLI/arrasto, e degradação graciosa quando
faltam dependências externas.

## Tarefas

1. `vad-app/src/panels/hud.rs`: HUD flutuante auto-hide (2s), conforme mockup §6 —
   scrubber, transporte (skip ±5s/10s, play/pause, A-B repeat, captura de frame),
   botão Whisper visível (ainda sem função — isso é M3), seletor de velocidade,
   seletor de faixa de áudio, volume.
2. `vad-core/src/error.rs`: `VadError` via `thiserror` + tabela erro→ação→UI (§4.14).
   **Implementar já aqui**, não adiar — o critério de saída desta sprint depende
   disto.
3. `vad-app/src/main.rs`: `clap` para abrir ficheiro por argumento
   (`vad ficheiro.mkv`) e `--fullscreen`.
4. `main.rs`: `probe_dependencies` — verificar `ffmpeg`/`yt-dlp` no `$PATH` ao
   arrancar; se em falta, aplicar a entrada correspondente da tabela do `error.rs`
   (desativar waveform/Whisper + mostrar o comando de instalação — **nunca invocar
   `sudo` a partir da app**, §4.14).
5. `vad-app/src/app.rs`: drag-and-drop via `egui` (`raw.dropped_files`).

## Fora de âmbito

MPRIS, inibidor de screensaver, guarda de foco de teclado — Sprint_03.

## Critério de saída

`vad ficheiro.mkv` e arrastar um ficheiro abrem reprodução. Sem `ffmpeg`/`yt-dlp`
instalados, a app arranca e degrada (botões desativados, comando de instalação
visível) **via o `error.rs`**, não via lógica ad-hoc espalhada pelo código.

## Nota de dependência entre tarefas

`probe_dependencies` e `error.rs` têm de ser entregues **juntos** nesta sprint — um
sem o outro não cumpre o critério de saída (o "degrada graciosamente" do §9 M1 é
literalmente a tabela do `error.rs` em ação).
