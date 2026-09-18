# Sprint_03 — Diário de bordo

**Milestone:** M1 (parte 2) — MPRIS, inibidor de screensaver, guarda de foco
**Planning:** ver `Sprint_Planning_03.md`
**Início:** 2026-09-18  
**Fim:** 2026-09-18  

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-18

- Leitura completa do `Sprint_Planning_03.md`, `Sprint_Report_02.md`, `workflow.md` e secções pertinentes do `PLANO_VAD.md` (§4.7, §4.25, §7, §9, §10.5).
- Elaborado e aprovado o plano detalhado de implementação para o Sprint 03.
- Tarefa 0 concluída:
  - Criado `vad-core/src/platform.rs` com o trait `PlatformIntegration` (§4.25), contendo métodos padrão para `on_playback_state`, `on_file_loaded`, `on_seek`, `on_volume_changed`, `on_mute_changed`, `update` e `shutdown`.
  - Adicionado método `stop(&self)` ao `Player` em `vad-core/src/player.rs` para suporte a comandos de paragem externa (MPRIS).
  - Re-exportado `PlatformIntegration` em `vad-core/src/lib.rs`.
  - Adicionado teste de encaminhamento do trait (`test_platform_integration_trait_dispatch`) e teste automatizado de concorrência com threads simultâneas disparando comandos no `Player` (`test_concurrent_player_commands` conforme §10.5). Ambos passam com sucesso.
- Tarefas 1 e 2 concluídas:
  - Adicionado `zbus = "5"` condicional a `cfg(target_os = "linux")` em `crates/vad-app/Cargo.toml`.
  - Adicionada variante `VadError::Platform(String)` com tabela de ação e degradação em `crates/vad-core/src/error.rs`.
  - Criado `crates/vad-app/src/mpris.rs` implementando `PlatformIntegration`: expõe `/org/mpris/MediaPlayer2` (`org.mpris.MediaPlayer2` e `org.mpris.MediaPlayer2.Player`). Suporta metadata (`mpris:trackid`, `xesam:title`, `xesam:url`, `mpris:length`), `PlaybackStatus` ("Playing"/"Paused"/"Stopped"), `Volume` normalizado, `Position` em microssegundos, comandos de transporte (`PlayPause`, `Play`, `Pause`, `Stop`, `Seek`, `SetPosition`, `Next`, `Previous`, `OpenUri`), emissão de sinais `PropertiesChanged` e sinal `Seeked`. Suporta instâncias múltiplas requisitando `org.mpris.MediaPlayer2.vad` e fallback transparente para `.vad.instance<PID>` (§9).
  - Criado `crates/vad-app/src/screensaver.rs` implementando `PlatformIntegration`: liga-se a `org.freedesktop.ScreenSaver` via proxy D-Bus. Inibe durante `PlaybackState::Playing` e liberta o cookie durante `Paused` ou `Idle`. Chamada defensiva a `UnInhibit` em `shutdown()` e no `Drop`. Degradação graciosa em ambientes sem D-Bus.
  - Integrado `platform_integrations: Vec<Box<dyn PlatformIntegration>>` em `VadApp` (`crates/vad-app/src/app.rs`): inicialização automática no arranque da app, despacho de todos os `PlayerEvent` no `poll_events`, chamadas de `update()` por frame e `shutdown()` na libertação (`Drop`).
- Tarefa 3 concluída (Guarda de foco de teclado):
  - Auditados e verificados os atalhos de teclado em `crates/vad-app/src/app.rs`.
  - Todos os atalhos globais de transporte (`Space` para play/pause, `ArrowLeft`/`ArrowRight` para seek relativo, `ArrowUp`/`ArrowDown` para volume, `M` para mute, `F`/`F11` para ecrã inteiro) estão condicionados por `!ctx.egui_wants_keyboard_input()`.
  - Adicionado teste unitário `test_keyboard_focus_guard_in_context` e `test_url_scheme_allowlist` no `app.rs`.
- Testes unitários adicionados e validados:
  - `test_platform_integration_trait_dispatch` e `test_concurrent_player_commands` em `vad-core`.
  - `test_mpris_server_lifecycle_and_event_handling` em `vad-app/src/mpris.rs`.
  - `test_screensaver_inhibitor_state_transitions` em `vad-app/src/screensaver.rs`.
  - `test_keyboard_focus_guard_in_context` e `test_url_scheme_allowlist` em `vad-app/src/app.rs`.
  - Todos os 14 testes da workspace a passar.

## Desvios face ao Sprint_Planning_03.md

Nenhum. Todo o âmbito planeado foi implementado conforme as especificações.

## Problemas encontrados

1. **API do zbus 5 para nomes e propriedades:**
   - Em zbus 5, `Connection::request_name` retorna `Result<()>` (e não o enum `RequestNameReply` das versões 2/3), recebendo `&str` e não `&String`. Adaptada a requisição com verificação de sucesso e fallback para `.vad.instance<PID>`.
   - A emissão de sinais gerados por `#[zbus(property)]` (ex.: `playback_status_changed`) requer o guard da interface `iface_ref.get().await` em conjunto com o `signal_emitter`. Adaptada a função interna `notify_player_property_changed` com esta assinatura.
2. **Panics em testes com `egui::Context::run_ui` em egui 0.36.2:**
   - Ao correr passes de teste em `egui::Context`, o `FullOutput::textures_delta` ativa uma asserção de sanidade em modo debug se for descartado sem tratamento. Resolvido invocando `out.textures_delta.clear()` nos testes sintéticos de contexto.
