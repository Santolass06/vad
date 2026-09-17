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

**Desvio de âmbito identificado na revisão:** a redação da tarefa 5 do planning
era ambígua sobre se `Ctrl+U` devia ficar apenas **visível** no ecrã de
acolhimento ou já **funcional** (a funcionalidade de URL/yt-dlp em si é
Sprint_05). A implementação optou por o tornar funcional, o que fez o modal
entregar texto arbitrário a `player.load_file()` sem a validação de esquema do
§4.27 — três sprints antes da data em que o plano assume essa validação.
Corrigido na secção "Correções de revisão" abaixo (ponto 3) com uma validação
mínima, sem trazer o resto da Sprint_05 para esta sprint.

Fora isso, nenhum desvio de funcionalidade ou âmbito. Ajustes técnicos decorrentes da versão `egui 0.36.2`:
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

## Correções de revisão (pós-fecho, mesma sprint)

Uma revisão de código encontrou e corrigiu os seguintes problemas antes de a sprint
ser dada como verificada:

1. **Comentário de invariante do §4.3 apagado sem substituição.** O diff desta
   sprint removeu, sem qualquer relação com o trabalho da Sprint_02, o comentário
   em `VideoRenderContext` que documenta que o contexto OpenGL tem de estar
   corrente na thread que chama `render`/`update` (§4.3) — exatamente a invariante
   que a revisão da Sprint_01 tinha deixado registada como risco. Restaurado.
2. **`ErrorAction::disabled_features` era dados mortos — a UI contornava a tabela.**
   `app.rs` decidia se o Whisper estava desativado com `matches!(e,
   VadError::FfmpegNotFound)` em vez de ler `action.disabled_features`, e
   `YtDlpNotFound`'s `disabled_features: &["url_playback"]` não era consultado em
   lado nenhum: com `yt-dlp` em falta, o botão "Abrir URL" continuava totalmente
   funcional. Isto contradiz o próprio critério de saída do planning ("degrada...
   via a tabela do `error.rs`, não via lógica ad-hoc"). Adicionado
   `VadApp::is_feature_disabled(feature)` que lê a tabela, e usado tanto para o
   botão Whisper como para desativar (com tooltip, mesmo padrão) o botão "Abrir
   URL (Ctrl+U)" e o botão "Abrir" dentro do modal de URL quando `yt-dlp` falta.
3. **Gap de segurança do §4.27 introduzido por âmbito adiantado.** O
   `Sprint_Planning_02.md` (tarefa 5) só pedia o atalho `Ctrl+U` **visível**; o
   modal de URL foi implementado como **funcional**, entregando qualquer texto
   diretamente a `player.load_file()` sem validação de esquema — reabrindo a
   ameaça que o §4.27 nomeia explicitamente (`file:///etc/shadow`, `smb://`) três
   sprints antes do previsto (Sprint_05). Corrigido com uma validação mínima de
   esquema (`http://`/`https://`/`rtsp://`) no submit do modal — não foi
   implementada nenhuma outra parte da infraestrutura de URL/yt-dlp da Sprint_05
   (isso continua fora de âmbito).
4. **`VadError::action()` tinha um `_ =>` genérico.** Isto derrota a verificação
   de exaustividade do compilador para um enum que o próprio §4.14 diz que vai
   continuar a crescer — uma variante nova ficaria silenciosamente a cair no
   fallback "Erro de Operação" em vez de alguém ser forçado a decidir a ação
   certa. Substituído por braços explícitos para `Mpv`, `Property`, `Playback` e
   `Io`.
5. **Teste `test_probe_dependencies_missing_detection` mutava `$PATH` global.**
   O teste chamava `std::env::set_var("PATH", ...)`, uma mutação ao nível do
   processo que corre em threads partilhadas com outros testes do mesmo binário
   (`cargo test` corre em paralelo por omissão) — mesma categoria de risco de
   teste não-hermético identificada na revisão da Sprint_01. Refatorado
   `is_executable_in_path_list(binary, paths)` como função pura parametrizada, e
   o teste passou a chamá-la diretamente com um `OsStr` de teste, sem tocar no
   ambiente do processo.
6. **Asserção tautológica em `test_player_speed_tracks_and_ab_loop`.**
   `assert!(audio_tracks.is_empty() || !audio_tracks.is_empty())` é sempre
   verdadeira independentemente do valor — não testava nada além de o `.expect()`
   anterior não ter entrado em pânico. Corrigido para `assert!(audio_tracks.is_empty())`
   (comportamento correto e específico quando não há média carregada).
