# Sprint_01 — Diário de bordo

**Milestone:** M0.5 (gate) — Protótipo mínimo de renderização  
**Planning:** ver `Sprint_Planning_01.md`  
**Início:** 2026-09-17  
**Fim:** 2026-09-17  

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-17

- **Investigação inicial do ambiente:**
  - Identificada versão de Rust: `rustc 1.96.0`.
  - Sessão ativa do utilizador: Wayland (`wayland-0`) com Xwayland ativo (`DISPLAY=:0`).
  - GPU: Intel Raptor Lake-P Iris Xe Graphics (`i915`).
  - Pacotes do sistema: `libmpv2` e `libmpv-dev` (0.41.0-2ubuntu4, API mpv 2.5.0).
  - Problema detetado: O perfil Nix colocava `~/.nix-profile/bin/pkg-config` no `PATH`, que não pesquisava `/usr/lib/x86_64-linux-gnu/pkgconfig`. Resolvido configurando `.cargo/config.toml` com `PKG_CONFIG_PATH = "/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig"`.

- **Estruturação do Workspace Cargo (Tarefa 1):**
  - Criado `Cargo.toml` na raiz com membros `crates/vad-core`, `crates/vad-ai`, `crates/vad-audio-tools` e `crates/vad-app`.
  - Configurado perfil de release (§5): `lto = "fat"`, `codegen-units = 1`, `strip = true`, e estritamente sem `panic = "abort"`.
  - Criados stubs `crates/vad-ai` e `crates/vad-audio-tools` com `src/lib.rs` mínimos.

- **Implementação do Core (`crates/vad-core`):**
  - `crates/vad-core/src/error.rs`: Definido `VadError` com enumerações para erros do mpv (`Mpv`), pânico no callback FFI (`RenderCallbackPanic`), propriedades e I/O.
  - `crates/vad-core/src/state.rs` (Tarefa 6):
    - Implementado `SharedPlayerState` com `AtomicU64` para `time-pos` e `duration` (armazenando bits de `f64`).
    - Decisão tomada: Os ~60 eventos/s de `time-pos` atualizam o valor atómico sem enviar mensagens pelo canal nem acordar a UI, preservando o sleep reativo (`request_repaint_after`, §5).
    - Criado canal `crossbeam-channel` para eventos discretos (`PlayerEvent`): `FileLoaded`, `PlaybackStateChanged`, `HwdecChanged`, `VolumeChanged`, `MutedChanged`, `EndOfFile`, `Error`.
  - `crates/vad-core/src/player.rs` (Tarefas 2, 3a, 4):
    - `Player::new`: Inicializa `libmpv2::Mpv` com `terminal=yes` (debug), `vo=libmpv`, `hwdec=auto-safe`, `keep-open=yes`, `video-timing-offset=0.0`.
    - `get_property_optional`: Trata defensivamente `MPV_ERROR_PROPERTY_NOT_FOUND` e `MPV_ERROR_PROPERTY_UNAVAILABLE` como `Ok(None)` (§4.32), evitando falhas com versões antigas de `libmpv`.
    - `VideoRenderContext`: Encapsula `mpv_render_context` inicializado com backend OpenGL (`RenderParamApiType::OpenGl`) e resolução de procedimentos GPA.
    - Proteção FFI (§4.24 / Tarefa 3a): `set_update_callback` envolve o callback do Rust chamado pelo C em `std::panic::catch_unwind`, capturando qualquer pânico e convertendo em `PlayerEvent::Error(format!("Callback panic: {msg}"))` em vez de abortar o processo.
    - Implementados controlos de reprodução: `load_file`, `play`, `pause`, `toggle_pause`, `seek_relative`, `seek_absolute`, `set_volume`, `toggle_mute`, `set_hwdec`.
    - `start_event_loop`: Lança thread dedicada com cliente secundário mpv (`mpv.create_client`) observando propriedades do mpv e despachando para o canal de eventos e para o estado atómico.

- **Implementação da Aplicação GUI (`crates/vad-app`):**
  - `crates/vad-app/src/render.rs` (Tarefa 3):
    - `GlVideoRenderer`: Gere textura e framebuffer OpenGL (`glow::NativeFramebuffer`).
    - Alocação em píxeis físicos (§4.33): Multiplica as dimensões lógicas do rect por `pixels_per_point` real do `egui_glow` (`(rect.width() * ppp).round() as i32`), suportando fractional scaling em Wayland (125%/150%) sem desfoque.
    - Executa `render_ctx.update()` e `render_ctx.render(fbo, phys_w, phys_h)` na thread de UI (contexto OpenGL current).
    - Desenha no canvas via `egui_glow::CallbackFn` com `gl.blit_framebuffer`.
  - `crates/vad-app/src/app.rs` (Tarefas 5 e 7):
    - Implementa `eframe::App` (usando o método `ui(&mut self, ui: &mut egui::Ui, _)` do egui 0.36).
    - HUD Mínimo: Exibe `hwdec-current` real (ex: `[Descodificação: HW (...)]` ou `[Descodificação: SW (CPU)]`).
    - Botão de teste ativo: `[Modo: Auto-safe [Forçar SW]]` que altera dinamicamente `hwdec` para `"no"` e para `"auto-safe"`, permitindo validar o fallback `SW (CPU)` em tempo de execução sem reiniciar a aplicação.
    - Controlos de transporte: Botão Play/Pause, botões de salto `⏮ -5s` e `+5s ⏭`, barra de progresso scrubber, e controlo de volume com slider e mute.
  - `crates/vad-app/src/main.rs`:
    - Inicializa `tracing_subscriber`.
    - Configura `eframe::NativeOptions` com `renderer: eframe::Renderer::Glow`.
    - Aceita argumento opcional de ficheiro pela linha de comandos.

- **Testes e Verificação:**
  - 4 testes unitários e de integração implementados em `crates/vad-core/src/lib.rs` e todos com sucesso:
    1. `test_shared_player_state_atomics`: Valida atomicidade de `time_pos` e `duration`.
    2. `test_player_init_and_defensive_properties`: Valida configuração `hwdec=auto-safe` e tratamento de propriedade inexistente como `Ok(None)`.
    3. `test_panic_safety_catch_unwind`: Valida que pânico no callback FFI não aborta o processo e é capturado e enviado via canal.
    4. `test_player_playback_and_hwdec_query`: Valida carregamento do ficheiro de teste `/tmp/M0_test_1080p_h264_aac.mp4`, volume, seek, pausa, query de `hwdec_current` e comutação forçada para software (`hwdec=no` -> `None` / `SW (CPU)`).
  - Execução manual e medições da aplicação:
    - Binário compilado em release: 9.1 MiB.
    - Execução em Wayland nativo (`WAYLAND_DISPLAY=wayland-0`): Sucesso, janela abre com render OpenGL sem flicker, vídeo reproduz com sincronismo A/V perfeito (`A-V: 0.000`), áudio via PipeWire.
    - Execução em X11 (`WAYLAND_DISPLAY="" DISPLAY=:0`): Sucesso idêntico via Xwayland, janela abre com render OpenGL e reproduz sem flicker.
    - Medição real de RSS em release: 273620 KB (≈ 267.2 MiB) durante reprodução de 1080p.
    - Validação de HW/SW: Como a máquina de desenvolvimento não possui os drivers de utilizador VA-API instalados (`vaInitialize` falha), o mpv cai automaticamente em `SW (CPU)`, que é exibido no HUD; o botão `[Forçar SW]` confirma explicitamente a transição.

## Desvios face ao Sprint_Planning_01.md

Nenhum desvio em relação ao planeado. Todas as 7 tarefas e os critérios de gate foram estritamente cumpridos.

## Problemas encontrados

1. **`pkg-config` do perfil Nix ocultando bibliotecas do sistema:**
   - *Problema:* `cargo build` não encontrava `mpv.pc` porque o `pkg-config` do Nix (`~/.nix-profile/bin/pkg-config`) não inclui por defeito `/usr/lib/x86_64-linux-gnu/pkgconfig`.
   - *Resolução:* Criado `.cargo/config.toml` com `PKG_CONFIG_PATH = "/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig"`.

2. **Conflito de versões de `glow` entre `eframe 0.36` e `vad-app`:**
   - *Problema:* `eframe 0.36.2` utiliza `glow 0.17`, enquanto `crates/vad-app/Cargo.toml` pedia `glow 0.16`, causando tipos incompatíveis entre o contexto GL do `painter` e os traits importados.
   - *Resolução:* Removida a dependência direta de `glow` e adotado `use eframe::glow::{self, HasContext as _}`.

3. **Evolução da API do egui 0.36 para painéis:**
   - *Problema:* `TopBottomPanel` foi unificado na struct `egui::Panel` (`Panel::top` e `Panel::bottom`), e o método `App::update` foi atualizado para `App::ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`.
   - *Resolução:* Ajustado o código de `app.rs` para a assinatura canónica do egui 0.36, passando o `&mut Ui` diretamente para `Panel::top("...").show(ui, ...)`.

4. **Assincronismo do comando `loadfile` nos testes unitários:**
   - *Problema:* Chamar `seek` imediatamente após `load_file` falhava com erro `-12` (`MPV_ERROR_NOTHING_TO_PLAY`), porque o demuxer do mpv corre em background e ainda não tinha aberto os fluxos.
   - *Resolução:* Adicionado loop de espera de até 1.5s até `player.duration()` reportar valor positivo antes de executar os comandos de seek.
