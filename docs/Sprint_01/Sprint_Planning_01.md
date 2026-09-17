# Sprint_Planning_01 — M0.5 (gate): Protótipo mínimo de renderização

**Milestone:** M0.5 (`PLANO_VAD.md` §9) — **gate**, não avançar para a Sprint_02 sem
este critério validado.
**Pré-requisito:** Sprint_00 fechada (baseline do M0 registado).

## Objetivo

Validar, antes de qualquer feature, que a arquitetura de renderização escolhida
(`mpv_render_context` + OpenGL via `egui_glow::CallbackFn`) funciona em X11 **e**
Wayland. Este é o risco técnico mais alto do projeto — se falhar aqui, é mais barato
descobrir agora do que depois de construir HUD, MPRIS, playlist, etc. por cima.

## Tarefas

1. Criar o workspace Cargo (`Cargo.toml` raiz + `crates/vad-core`, `crates/vad-ai`,
   `crates/vad-audio-tools`, `crates/vad-app` — `PLANO_VAD.md` §7). `vad-ai` e
   `vad-audio-tools` podem ficar como stubs vazios nesta sprint; só `vad-core` e
   `vad-app` têm trabalho real.
2. `vad-core/src/player.rs`: wrapper mínimo sobre `libmpv2`, inicializar o
   `mpv_render_context` com backend OpenGL.
3. `vad-app/src/render.rs`: `egui_glow::CallbackFn` que invoca a render API do mpv na
   thread de UI — restrição do §4.3: o contexto OpenGL tem de estar *current* na
   thread que chama a render API. A renderização do vídeo fica presa a essa thread; só
   os *eventos* do mpv correm à parte.
4. `hwdec=auto-safe` (§4.3/§6) configurado no `player.rs`.
5. HUD mínimo (não o HUD completo do M1) só para mostrar `hwdec-current` real — ler a
   propriedade do mpv, nunca assumir que o valor pedido foi aplicado.
6. `vad-core/src/state.rs`: canal `crossbeam-channel`/`watch` dos eventos do mpv
   (thread C interna) para a UI — versão inicial, mesmo que rudimentar.
7. Play/pause/seek/volume básicos ligados a controlos mínimos, só o suficiente para
   testar manualmente.

## Fora de âmbito

MPRIS, screensaver, CLI, drag-and-drop, playlist, qualquer painel completo — tudo isso
é M1+.

## Critério de saída (gate)

- Play/pause/seek/volume funcionam em **X11 e Wayland** sem flicker, testado com Intel
  e, se possível, NVIDIA.
- HUD mínimo mostra `hwdec-current` real, **incluindo o fallback `SW (CPU)`** quando o
  hwdec é forçado a falhar (testar isto ativamente — forçar `hwdec=no` ou usar um
  cenário em que o driver falhe — não assumir que o caminho feliz é o único testado).

## Risco conhecido

`wid` (janela nativa X11) **não funciona em Wayland** — não é uma opção de recurso se
`mpv_render_context` falhar (§4.3). Não tentar "cair" para lá.

**Gate: registar SIM/NÃO explícito no `Sprint_Report_01.md` antes de iniciar a
Sprint_02.**
