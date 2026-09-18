# Sprint_04 — Diário de bordo

**Milestone:** M2 (parte 1) — Playlist, faixas, video_panel, equalizador
**Planning:** ver `Sprint_Planning_04.md`
**Início:** 2026-09-18
**Fim:** (em curso)

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-18

- **Investigação empírica de propriedades do libmpv:**
  - Verificação de que `video-aspect-override`, `video-rotate`, `video-crop`, `panscan`, `audio-delay`, `sub-delay` e ajustes de cor (`brightness`, `contrast`, `saturation`, `gamma`) são nativamente aceites pela `libmpv.so.2` instalada.
  - Confirmação de que `audio-device-list/count`, `name` e `description` funcionam defensivamente, permitindo selecionar o dispositivo áudio via propriedade `audio-device`.
  - Confirmação de que filtros de equalizador de 10 bandas (`lavfi=[equalizer=f=...:width_type=o:w=1:g=...]`) e redução de ruído RNNoise (`lavfi=[arnndn]`) combinam numa cadeia `af` separada por vírgulas sem falhas na reprodução.
- **Implementação de `vad-core/src/playlist.rs`:**
  - Criação de `PlaylistItem` (`File` e `Url`), com resolução de streaming diferida para a Sprint 05 conforme planeado.
  - Modos de repetição `RepeatMode` (`Off`, `All`, `Single`).
  - Implementação de `Playlist` com navegação (`next`, `previous`), `has_next`, `has_previous`, ordenação (`move_item`), adição/remoção e shuffle não destrutivo via Fisher-Yates com PRNG leve (XorShift64), sem necessidade de crates externas de números aleatórios (§5).
  - Adicionados testes unitários para a playlist com cobertura de adição, remoção, reordenação, repetição e shuffle com restauração de sequência original.
- **Extensão de `vad-core/src/player.rs`:**
  - Adicionado suporte a `volume-max=200.0` no arranque para permitir volume boost.
  - Adicionados métodos de controlo de vídeo: `video_aspect_override`, `set_video_aspect_override`, `video_rotate`, `set_video_rotate`, `video_crop`, `set_video_crop`, `panscan`, `set_panscan`, `audio_delay`, `set_audio_delay`, `sub_delay`, `set_sub_delay`, `sub_visibility`, `set_sub_visibility`, `load_subtitles`.
  - Adicionados métodos de controlo de cor: `color_adjustments`, `set_color_adjustments`, `reset_color_adjustments`.
  - Adicionados métodos de áudio: `AudioDevice`, `audio_devices`, `audio_device`, `set_audio_device`, `set_audio_filters`.
  - Testes em `vad-core/src/lib.rs` (`test_player_video_and_audio_properties`) validados com 13 testes a passar.

- **Implementação dos painéis da UI em `vad-app`:**
  - `vad-app/src/panels/video_panel.rs`:
    - Implementação estrita do design `design/VideoPanel.dc.html`.
    - Seletores de proporção (Auto, 16:9, 4:3, 21:9) com `video-aspect-override`.
    - Rotação rápida em 4 quadrantes (0°, 90°, 180°, 270°) com `video-rotate`.
    - Recorte e enquadramento (Desativado, Preencher ecrã/PanScan 1.0, 16:9, 4:3) com `video-crop` e `panscan`.
    - Desfasamento / sincronização fina de áudio e legendas (`audio-delay`, `sub-delay`) em milissegundos com botões de passo ±50ms e ±100ms.
    - Controlos de imagem / cor: Brilho, Contraste, Saturação e Gama com sliders (-100 a +100) e botão de reset rápido.
    - Toggle de visibilidade de legendas e atalhos visíveis.
  - `vad-app/src/panels/audio_panel.rs`:
    - Implementação estrita do design `design/Equalizer.dc.html`.
    - Equalizador gráfico de 10 bandas (32Hz, 64Hz, 125Hz, 250Hz, 500Hz, 1kHz, 2kHz, 4kHz, 8kHz, 16kHz) de -12 dB a +12 dB.
    - Linha de referência pontilhada a 0 dB, grelha vertical com marcadores e sliders interativos verticais com drag suave.
    - Renderização de curva espectral contínua com interpolação geométrica e preenchimento poligonal translúcido com brilho neon/accent.
    - Presets de áudio: "Plano", "Voz clara", "Música", "Cinema" e "Personalizado" em tempo real.
    - Secção de Volume Boost (100% a 200%) com slider dedicado e sincronizado com `player.volume()`.
    - Toggle de redução de ruído neuronal RNNoise (`lavfi=[arnndn]`) encadeado com os filtros de equalizador em `af`.
    - Seletor de dispositivo de saída via `egui::ComboBox` consumindo `audio-device-list` com cache defensiva e suporte a escolha manual ou automático (PipeWire).
  - `vad-app/src/panels/playlist_panel.rs`:
    - Implementação estrita do design `design/Playlist.dc.html`.
    - Cabeçalho com contagem de faixas, botões de shuffle (com indicador de estado) e ciclo de repetição (Desligado, Todas, Uma).
    - Lista de faixas com scroll vertical, numeração, título, duração/formato, botão de remover individual e clique para reproduzir.
    - Cartão de faixa atual "A TOCAR AGORA" com badges de estado e destaque visual.
  - `vad-app/src/panels/hud.rs`:
    - Adicionados botões de alternância rápida dos painéis laterais na ROW 2 do HUD: `📜 Playlist`, `🎚 Equalizador` e `🎞 Vídeo`.
    - Seletor de faixas de áudio e legendas enriquecido no HUD com opção "+ Carregar legenda externa..." abrindo modal de seleção de ficheiro (.srt, .vtt, .ass, etc.).
- **Integração na Janela Principal e Atalhos (`vad-app/src/app.rs`):**
  - Definição de `ActiveSidePanel` (`None`, `Playlist`, `Equalizer`, `Video`).
  - Painel lateral uniforme à direita com largura estrita de **340px fixos** (`exact_size(340.0)`, `resizable(false)`), evitando saltos no viewport OpenGL ao alternar entre Playlist, Equalizador e Vídeo.
  - Botões de alternância direta no topo da barra lateral e na barra de controlo superior.
  - Implementação dos atalhos de sincronização de áudio e legendas: `j`/`k` (atraso de áudio -/+ 50ms) e `g`/`h` (atraso de legendas -/+ 50ms), rigorosamente guardados por `!ctx.egui_wants_keyboard_input()`.
  - Tratamento de evento `PlaybackState::EndOfFile` para progressão automática de faixas na playlist de acordo com o modo de repetição e shuffle ativo.
- **Integração do MPRIS com a Playlist (`vad-app/src/mpris.rs`):**
  - Ligação do `MprisPlayer` ao `Arc<Mutex<Playlist>>`.
  - Suporte completo às propriedades e métodos do `org.mpris.MediaPlayer2.Player`:
    - `Next`: avança na playlist e carrega o ficheiro no player.
    - `Previous`: se a reprodução decorrida for superior a 3 segundos reinicia a faixa atual (seek a 0.0); caso contrário retrocede na playlist.
    - `CanGoNext` e `CanGoPrevious`: refletem dinamicamente a existência de faixa seguinte/anterior de acordo com o modo de repetição/shuffle.
    - `LoopStatus` (`None`, `Track`, `Playlist`) e `Shuffle` (true/false) bidirecionais entre D-Bus e estado interno.
- **Verificação, Linting e Testes:**
  - Adicionados testes unitários cobrindo o ciclo MPRIS com navegação de playlist, transições de estado dos painéis laterais e aplicação de presets do equalizador.
  - Executados `cargo test --workspace` (23 testes a passar) e `cargo clippy --workspace --all-targets -- -D warnings` (0 erros, 0 avisos).

## Desvios face ao Sprint_Planning_04.md

- Nenhum. Todas as tarefas 1 a 5 foram entregues com total fidelidade ao planeamento.

## Problemas encontrados

- **Devolução numérica de `video-aspect-override`:** Ao definir `video-aspect-override` para `"16:9"`, o mpv armazena e devolve o valor em ponto flutuante `"1.777778"` em vez da string da fração original. A API em `Player::video_aspect_override` foi ajustada para retornar `f64`, simplificando a comparação numérica (`ratio <= 0.0` para Auto, `(ratio - 16.0/9.0).abs() < 0.01` para 16:9, etc.).
- **Unificação de Panels no egui 0.36:** Na versão `0.36.2` do egui, `SidePanel::right` foi unificado em `Panel::right(id)`, e o método de largura fixa passou a ser `exact_size(340.0)` em vez de `exact_width`. O código foi corrigido em conformidade sem alterar as dimensões requeridas.
- **Avisos de linter / clippy:** Foram identificadas e corrigidas simplificações idiomáticas de Rust 1.96 (`.next_back()` em `DoubleEndedIterator`, `is_some_and`, `is_none_or`, e anotação `#[allow(clippy::should_implement_trait)]` em `Playlist::next` para evitar ambiguidade com `Iterator::next`).


