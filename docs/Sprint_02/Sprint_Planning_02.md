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
   seletor de faixa de áudio, volume. **Superfície do HUD em opacidade fixa ~92%
   (§4.29), nunca blur** — o `egui_glow` não tem `backdrop-filter` sobre o FBO do mpv,
   e simular isso por shader multi-pass sobrecarrega GPUs integradas sem necessidade.
   Barra de scrubbing e slider de volume com área de toque de ~20px de altura (mais
   larga que a barra visual de 4px), expandindo para 8px em hover e manípulo de 16px
   durante o arrasto — evita alvos demasiado estreitos em ecrãs HiDPI.
2. `vad-core/src/error.rs`: `VadError` via `thiserror` + tabela erro→ação→UI (§4.14).
   **Implementar já aqui**, não adiar — o critério de saída desta sprint depende
   disto. Diálogos de dependência em falta fecham com `Escape` (§4.34) e têm um botão
   de copiar o comando de instalação sugerido (`sudo apt install ffmpeg`) para a
   área de transferência.
3. `vad-app/src/main.rs`: `clap` para abrir ficheiro por argumento
   (`vad ficheiro.mkv`) e `--fullscreen`.
4. `main.rs`: `probe_dependencies` — verificar `ffmpeg`/`yt-dlp` no `$PATH` ao
   arrancar; se em falta, aplicar a entrada correspondente da tabela do `error.rs`
   (desativar waveform/Whisper + mostrar o comando de instalação — **nunca invocar
   `sudo` a partir da app**, §4.14).
5. `vad-app/src/app.rs`: drag-and-drop via `egui` (`raw.dropped_files`); ecrã de
   acolhimento inicial (sem ficheiro aberto) com zona de dropzone ampla, atalhos
   `Ctrl+O`/`Ctrl+U` visíveis e lista dos últimos ficheiros de `recents.rs`
   (Sprint_05 — se `recents.rs` ainda não existir nesta sprint, deixar o espaço no
   layout em vez de o omitir e ter de o reintroduzir depois).

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
