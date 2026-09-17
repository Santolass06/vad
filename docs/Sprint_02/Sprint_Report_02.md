# Sprint_Report_02 — Relatório de fecho

**Milestone:** M1 (parte 1 de 2) — HUD, CLI, drag-and-drop, probe_dependencies, error.rs  
**Baseado em:** `Sprint_02.md`  
**Data:** 2026-09-17  

---

## Resumo

A primeira metade do Milestone M1 foi implementada com sucesso. A aplicação VAD dispõe agora de um HUD flutuante completo com auto-hide de 2 segundos e superfície sem blur a ~92% de opacidade fixa (§4.29), controlos com área de toque alargada de ~20px para ecrãs HiDPI, suporte completo a abertura de ficheiros e URLs por CLI (`clap`) e por arrastar-e-largar (`egui` drag-and-drop), ecrã de acolhimento inicial com zona de dropzone e espaço reservado para `recents.rs` (Sprint 05), e uma arquitetura centralizada de tratamento de erros e degradação graciosa através da tabela erro→ação→UI do `error.rs` ligada ao `probe_dependencies` (§4.14). A política anti-`sudo` foi estritamente respeitada (apresentação do comando e botão de cópia, sem qualquer execução automática).

## Entregue vs. planeado

| Tarefa (planning) | Estado | Detalhes |
| :--- | :--- | :--- |
| 1. `vad-app/src/panels/hud.rs`: HUD flutuante auto-hide (2s), scrubber, transporte, repetição A-B, captura de frame, botão Whisper, velocidade, faixas, volume; opacidade ~92% sem blur (§4.29); toque de ~20px no scrubber e volume | ✅ Feito | Criado `HudPanel` com painter nativo para scrubber e slider de volume com hitbox de 20px, barra visual de 4px (8px em hover, manípulo de 16px em drag). Botão Whisper desativa graciosamente com tooltip se faltar FFmpeg. Temporizador de inatividade oculta o HUD aos 2s de playback. |
| 2. `vad-core/src/error.rs`: `VadError` via `thiserror` + tabela erro→ação→UI (§4.14); diálogos fecham com `Escape` (§4.34) e têm botão de copiar comando de instalação | ✅ Feito | Expandido `VadError` com enum `ErrorSeverity` e struct `ErrorAction`. Mapeadas variantes para FFmpeg, yt-dlp, falhas de arranque GL e mpv. Diálogo fecha com `Escape` e copia comando sugerido para o clipboard via `ctx.copy_text`. |
| 3. `vad-app/src/main.rs`: `clap` para abrir ficheiro por argumento (`vad ficheiro.mkv`) e `--fullscreen` | ✅ Feito | Adicionada crate `clap` (derive), definida struct `CliArgs` e configurado `ViewportBuilder::with_fullscreen`. Validação com `vad-app --help` e execução funcional. |
| 4. `main.rs`: `probe_dependencies` — verificar `ffmpeg`/`yt-dlp` no `$PATH` ao arrancar; se em falta, aplicar tabela do `error.rs` (sem invocar `sudo`, §4.14) | ✅ Feito | Módulo `probe.rs` pesquisa `$PATH` diretamente sem invocar shell (`sh -c`). Se faltarem binários, app entra em modo degradado via `error.rs`, desativa Whisper e exibe diálogo com comando e botão de cópia. |
| 5. `vad-app/src/app.rs`: drag-and-drop via `egui` (`raw.dropped_files`); ecrã de acolhimento inicial com dropzone ampla, atalhos `Ctrl+O`/`Ctrl+U` visíveis e lista de recentes reservada (Sprint 05) | ✅ Feito | Ecrã de acolhimento funcional com atalhos, dropzone e caixa reservada para `recents.rs`. Suporte a arrastar ficheiros para a janela com overlay de realce. Substituição dos `.expect()` por erros geridos. |

## Critério de saída — cumprido?

**Sim.**
1. `vad ficheiro.mkv` e arrastar um ficheiro para a janela abrem e iniciam a reprodução imediata.
2. Sem `ffmpeg`/`yt-dlp` instalados (testado isolando o `$PATH`), a aplicação arranca perfeitamente e entra em modo degradado através da tabela de erros de `error.rs`: os botões correspondentes surgem desativados com tooltip explicativo, o diálogo modal apresenta o comando sugerido (`sudo apt install ffmpeg`/`yt-dlp`) com botão de cópia para a área de transferência, permitindo fechar com `Escape` (§4.34) ou ignorar. **Nenhum comando `sudo` é executado pela aplicação** (§4.14).

## Problemas encontrados e resolução

1. **Ajustes de API para egui 0.36.2:**
   - `Rounding` foi renomeado para `CornerRadius` e o método `Painter::rect`/`rect_stroke` passou a exigir `StrokeKind::Inside`.
   - `ctx.wants_keyboard_input()` passou a chamar-se `ctx.egui_wants_keyboard_input()`.
   - `ui.set_enabled()` foi substituído por `ui.add_enabled(bool, widget)`.
   - `DroppedFile::path()` retorna `&Path` em vez de campo direto.
   - Todos os pontos foram adaptados à API canónica da versão 0.36.2 sem adicionar dependências desnecessárias.
2. **Conflito de borrows em UI:**
   - A leitura direta de eventos em `self.event_rx` impedia a chamada a `self.update_hwdec_label()`. Resolvido drenando os eventos para um vetor local antes da iteração.
   - De igual modo, a iteração sobre referências de `cached_audio_tracks` durante menus suspensos foi resolvida clonando o vetor de faixas antes da renderização do `ComboBox`.

## Dívida técnica / riscos para sprints seguintes

- **Sprint 03 (M1 parte 2):** MPRIS via `zbus`, inibidor de screensaver (mesma base D-Bus), guarda de foco de teclado e decisão de janela nova por execução fecham o Milestone M1 (`Sprint_Planning_03.md`).
- **Layout de Recentes:** O espaço visual e a geometria do ecrã de acolhimento encontram-se definidos; na Sprint 05 será ligado à persistência atómica de `recents.rs` (§4.6/§4.30).
- **Consumo de RAM:** Continua mapeado o acompanhamento do RSS medido no M0.5 (~267 MiB) face ao baseline VLC para avaliação no marco final M6.
