# Sprint_Planning_05 — M2 (parte 2): URL, resume, config.rs

**Milestone:** M2, parte 2 de 2 (fecha M2)
**Pré-requisito:** Sprint_04 fechada.

## Objetivo

Fechar M2: reprodução por URL isolada da config pessoal do utilizador, resume de
sessão, configuração unificada.

## Tarefas

1. **Reprodução por URL (yt-dlp).** A libmpv, ao contrário do binário standalone, não
   carrega config/scripts por omissão — ativar isso explicitamente. **Isolar
   `config-dir`** numa pasta própria do VAD (nunca `~/.config/mpv`) **e passar
   `--no-config` explicitamente** — isolar o `config-dir` sozinho não chega, o
   `ytdl_hook` e outros scripts podem ainda ler de localizações por omissão fora dessa
   pasta (§3, item marcado como "por confirmar em M2").
2. `vad-core/src/recents.rs`: últimos ~20 ficheiros + timestamp (§4.6). **Não** usar o
   `watch-later` nativo do mpv — diálogo próprio "continuar de onde parou".
3. Diálogo de resume na UI (design `Dialogs.dc.html`, "Continuar de onde parou?").
4. `vad-core/src/config.rs`: `~/.config/vad/config.toml` via `serde`+`toml`,
   consolidando `recents`, atalhos de teclado, e o **schema** (ainda não a UI) para a
   escolha disco/RAM-only por modelo — a feature de Whisper em si só chega em M3.

## Fora de âmbito

Qualquer coisa de Whisper/`model_manager` além do schema do `config.rs` — Sprint_06.

## Critério de saída (fecha M2, §9)

- URL do YouTube reproduz sem herdar `~/.config/mpv` do utilizador — **testar numa
  máquina com config mpv pessoal existente**, para confirmar isolamento real, não só
  assumir pela leitura do código.
- Reabrir a app oferece continuar o último ficheiro.

## Risco conhecido

Este é precisamente o item que o `PLANO_VAD.md` §3 marca como "por confirmar em M2" —
tratar o teste de isolamento de config como parte do critério de saída, não como
verificação opcional.
