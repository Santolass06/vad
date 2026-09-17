# Sprint_02 — Diário de bordo

**Milestone:** M1 (parte 1) — HUD, CLI, drag-and-drop, probe_dependencies, error.rs
**Planning:** ver `Sprint_Planning_02.md`
**Início:** 2026-09-17  
**Fim:** 2026-09-17  

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-17

- **Arranque da Sprint 02 (M1 parte 1):**
  - Pré-requisito verificado: Gate M0.5 validado (`Sprint_Report_01.md`: SIM).
  - Dívida técnica do M0.5 mapeada: substituição dos `.expect()` em `VadApp::new` por tratamento gracioso de erros através da tabela do `error.rs`.
  - Leitura das especificações: `Sprint_Planning_02.md`, `PLANO_VAD.md` (§4.14, §4.29, §4.34, §6, §7), `design/Main.dc.html`, `design/Dialogs.dc.html`.

- **Implementação de `vad-core/src/error.rs` (Tarefa 2):**
  - Implementado enum `VadError` expandido com variantes para dependências em falta (`FfmpegNotFound`, `YtDlpNotFound`), erros de arranque gráfico (`GlContextUnavailable`), motor mpv (`PlayerInitFailed`), aceleração de hardware (`HwDecUnavailable`), extração (`ExtractionFailed`) e download de modelos (`ModelDownloadFailed`).
  - Implementada a estrutura `ErrorAction` e enum `ErrorSeverity` mapeando a tabela erro→ação→UI do §4.14: cada erro define severidade, título, descrição, comando de instalação sugerido (`sudo apt install ffmpeg`/`yt-dlp`), funcionalidades desativadas e se pode ser ignorado.
  - Teste unitário `test_vad_error_taxonomy` adicionado e validado em `vad-core/src/lib.rs`.

- **Extensão do `Player` em `vad-core/src/player.rs` (preparação para Tarefa 1):**
  - Implementados métodos para velocidade de reprodução (`speed`, `set_speed`), captura de fotograma (`take_screenshot`), ciclo e estado de A-B repeat (`cycle_ab_loop`, `ab_loop_status`, enum `AbLoopStatus`), e inspeção defensiva de faixas de áudio e legendas (`audio_tracks`, `subtitle_tracks`, `set_audio_track`, `set_subtitle_track`, struct `TrackInfo`).
  - Teste unitário `test_player_speed_tracks_and_ab_loop` validado com sucesso.

- **Implementação do CLI via `clap` (Tarefa 3):**
  - Adicionado `clap = { version = "4", features = ["derive"] }` a `crates/vad-app/Cargo.toml`.
  - Definida estrutura `CliArgs` em `crates/vad-app/src/main.rs`: argumento posicional `file: Option<String>` e flag `--fullscreen` (`-f`).
  - Configurado `ViewportBuilder::default().with_fullscreen(args.fullscreen)`.
  - Testado e validado `vad-app --help` e execução com ficheiro e `--fullscreen`.

- **Implementação de `probe_dependencies` (Tarefa 4):**
  - Criado `crates/vad-app/src/probe.rs` com `probe_dependencies() -> Vec<VadError>`.
  - Pesquisa pura de `$PATH` via `std::env::split_paths` e verificação de permissões executáveis (`mode() & 0o111 != 0`) sem recorrer a shell (`sh -c`) para prevenção de injeção (§4.26).
  - Testes unitários `test_is_executable_in_path_common_bins` e `test_probe_dependencies_missing_detection` adicionados e validados.

- **Implementação do HUD Flutuante em `crates/vad-app/src/panels/hud.rs` (Tarefa 1):**
  - Superfície flutuante ancorada ao fundo do ecrã com opacidade fixa ~92% (`Color32::from_rgba_premultiplied(26, 28, 40, 235)`), borda de 1px e cantos arredondados (16px), **sem shaders de blur** (§4.29).
  - Temporizador de inatividade com auto-hide de 2.0s durante reprodução; movimento do cursor ou estado de pausa reativa e mantém a visibilidade do HUD.
  - Scrubber customizado com hitbox de ~20px de altura (`Sense::click_and_drag()`), barra visual de 4px expandindo para 8px em hover, manípulo de 16px durante o arrasto, formatação de tempo duplo (`00:14:23 / 01:30:00` e negativo `-01:15:37`).
  - Barra de transporte com saltos `⏮ 10s`, `⏪ 5s`, botão central de Play/Pause, `5s ⏩`, `10s ⏭`, repetição A-B interativa com ciclo de estados, captura de fotograma (`📸 Frame`) e botão Whisper com badge dourado.
  - Integração da política de degradação: se faltar `ffmpeg`, o botão Whisper fica desativado com tooltip informativo indicando `sudo apt install ffmpeg`.
  - Linha secundária com seletores de velocidade (`0.25x` a `2.0x`), faixas de áudio e legendas (via mpv `track-list`), e slider de volume com hitbox de 20px e botão mute.

- **Implementação de Drag-and-Drop, Acolhimento e Diálogo de Degradação em `crates/vad-app/src/app.rs` (Tarefa 5):**
  - Resolução da dívida técnica do M0.5: eliminados os `.expect()` em `VadApp::new` para `Player::new` e contexto OpenGL, apresentando um ecrã de recuperação com a taxonomia do `error.rs` sem abortar o processo.
  - Drag-and-drop: interceção de ficheiros arrastados via `ctx.input(|i| i.raw.dropped_files)` com carregamento automático no leitor, e overlay de realce quando ficheiros estão a pairar sobre a janela.
  - Ecrã de acolhimento inicial quando não há ficheiro aberto: dropzone ampla com atalhos `Ctrl+O` e `Ctrl+U`, e secção reservada de layout para `recents.rs` (Sprint 05).
  - Diálogo modal de degradação quando faltam dependências (`ffmpeg`/`yt-dlp`), conforme `design/Dialogs.dc.html`: exibe explicação, bloco com o comando `sudo apt install ...`, botão para copiar para o clipboard via `ctx.copy_text`, botão de fecho/ignorar, e botão de nova verificação.
  - Acessibilidade de teclado: diálogo de dependências e modal de abertura fecham com a tecla `Escape` (§4.34).

## Desvios face ao Sprint_Planning_02.md

Nenhum desvio de funcionalidade ou âmbito. Ajustes técnicos decorrentes da versão `egui 0.36.2`:
- `Rounding` foi renomeado para `CornerRadius` no egui 0.36 e os métodos de desenho do `Painter` agora requerem o parâmetro explícito `StrokeKind::Inside`.
- `Frame::new()` foi configurado com `.corner_radius(16)` e margem inteira `.inner_margin(20)`.

## Problemas encontrados

1. **`file.path()` em egui 0.36:**
   - *Problema:* Aceder a `file.path` como campo falhou na compilação porque `DroppedFile::path()` é um método que retorna `&Path`.
   - *Resolução:* Chamado `file.path()` e verificado se a string resultante não está vazia.

2. **Deteção de foco de teclado (`wants_keyboard_input`):**
   - *Problema:* `ctx.wants_keyboard_input()` foi renomeado no egui 0.36 para `ctx.egui_wants_keyboard_input()`.
   - *Resolução:* Atualizado para a nova nomenclatura canónica da crate.

3. **Substituição de `ui.set_enabled()`:**
   - *Problema:* `ui.set_enabled(false)` foi descontinuado no egui 0.36.
   - *Resolução:* Utilizado `ui.add_enabled(false, widget)`.

4. **Conflito de borrows no loop de eventos e na listagem de faixas do HUD:**
   - *Problema:* Em `poll_events`, ler diretamente de `self.event_rx` impedia a chamada a `self.update_hwdec_label()`; de forma análoga, iterar sobre `&self.cached_audio_tracks` impedia a mutação de `self.poke()` dentro do closure do `ComboBox`.
   - *Resolução:* Coletar eventos para um `Vec` local antes do processamento, e clonar a lista de faixas (vetor de pequenas structs) antes de passar ao menu.
