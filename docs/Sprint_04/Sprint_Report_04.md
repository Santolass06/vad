# Sprint_Report_04 — Relatório de fecho

**Milestone:** M2 (parte 1 de 2: Playlist, faixas, vídeo, equalizador)  
**Baseado em:** `Sprint_04.md`  
**Data:** 2026-09-18  

---

## Resumo

A Sprint 04 (primeira metade do Milestone M2) foi concluída com êxito. O foco residiu na produtividade de reprodução multimédia e no controlo avançado de vídeo e áudio através das capacidades nativas do `libmpv` (§3), dotando a aplicação de uma interface gráfica refinada, moderna e consistente com as especificações visuais de referência (`design/Playlist.dc.html`, `design/Equalizer.dc.html` e `design/VideoPanel.dc.html`).

Foi introduzida a estrutura de dados `Playlist` em `vad-core/src/playlist.rs`, suportando ficheiros locais e URLs, modos de repetição cíclica (`Off`, `All`, `Single`), e baralhamento (*shuffle*) não destrutivo com algoritmo Fisher-Yates e PRNG determinístico leve (XorShift64), sem recurso a dependências externas supérfluas (§5). No subsistema de vídeo, o novo painel `video_panel.rs` expõe o controlo em tempo real de proporção de ecrã (*aspect ratio*), rotação em 4 quadrantes, recorte/enquadramento (*panscan* e *crop*), desfasamento de sincronização de áudio e legendas (com atalhos rápidos de teclado `j`/`k`/`g`/`h`), e ajuste fino de imagem (brilho, contraste, saturação e gama).

No subsistema de áudio, o painel `audio_panel.rs` implementa um equalizador gráfico de 10 bandas (32 Hz a 16 kHz) com linha de referência pontilhada nos 0 dB, renderização vetorial de curva espectral contínua com preenchimento poligonal translúcido, presets de áudio instantâneos ("Plano", "Voz clara", "Música", "Cinema"), amplificação de volume (*Volume Boost*) até 200%, redução de ruído por redes neuronais recorrentes RNNoise (`lavfi=[arnndn]`), e seleção explícita de dispositivo de saída via `audio-device-list`.

Adicionalmente, os três painéis laterais foram integrados numa barra lateral uniforme de **340px fixos**, eliminando qualquer salto de layout na superfície de renderização OpenGL, e o servidor MPRIS v2 foi totalmente sincronizado com a nova `Playlist`.

## Entregue vs. planeado

| Tarefa (planning) | Estado | Detalhes |
| :--- | :--- | :--- |
| 1. `vad-core/src/playlist.rs`: shuffle/repeat, itens locais e por URL. Aceitação de ambos os tipos na estrutura de dados (resolução de URLs diferida para a Sprint 05). | ✅ Feito | Implementados `PlaylistItem` (`File` e `Url`), `RepeatMode` (`Off`, `All`, `Single`), e a estrutura `Playlist`. Baralhamento não destrutivo via Fisher-Yates com XorShift64, permitindo restaurar a ordem original ao desativar o shuffle. Suporte completo a navegação (`next`, `previous`), reordenação (`move_item`), adição e remoção. Coberto por testes unitários exaustivos. |
| 2. Seletor de faixas de áudio/legendas na UI (exposto pelo mpv, §3). | ✅ Feito | HUD atualizado com seletores suspensos (`ComboBox`) de faixas de áudio e legendas sem interromper nem recarregar o ficheiro. Adicionado suporte no seletor de legendas para "+ Carregar legenda externa..." com diálogo de seleção de ficheiro (`.srt`, `.vtt`, `.ass`, `.sub`) injetado via `sub-add`. |
| 3. `vad-app/src/panels/video_panel.rs`: aspect ratio, crop, rotação (`video-aspect-override`/`video-crop`/`video-rotate`), delay de áudio/legendas (`audio-delay`/`sub-delay`) — painel do design `VideoPanel.dc.html`. | ✅ Feito | Painel lateral de vídeo implementado fielmente ao mockup. Suporta rácios Auto, 16:9, 4:3, 21:9; rotação a 0°, 90°, 180°, 270°; modos de enquadramento (PanScan e Crop 16:9/4:3); sincronização de áudio e legendas com botões de passo e atalhos rápidos (`j`/`k` para áudio, `g`/`h` para legendas) com guarda estrita de foco de teclado; e controlos de cor (-100 a +100) com reposição rápida a 0. |
| 4. `vad-app/src/panels/audio_panel.rs`: equalizador de 10 bandas + presets + volume boost com filtros `lavfi` — painel do design `Equalizer.dc.html`, com linha de referência a 0 dB, curva espectral contínua e lista `audio-device-list`. | ✅ Feito | Painel do equalizador gráfico implementado. Contém 10 bandas verticais interativas com marcadores de ganho (±12 dB, passo 0.5 dB), curva contínua geométrica com preenchimento sombreado neon, presets em tempo real, Volume Boost até 200% (`volume-max=200.0`), ativação de RNNoise, e ComboBox com lista dinâmica de placas/interfaces de áudio (`audio-device-list`) e seleção manual ou automática (PipeWire). |
| 5. Painéis laterais com largura uniforme de **340px fixos** — evita saltos de layout na área de vídeo ao trocar de painel. | ✅ Feito | Estruturado `egui::Panel::right` com `exact_size(340.0)` e `resizable(false)`. A alternância entre Playlist, Equalizador e Vídeo ocorre no mesmo espaço lateral com abas de topo, mantendo a geometria da viewport de vídeo OpenGL estável. |
| 6. *(Integração MPRIS)*: Ligar comandos de playlist no D-Bus (`Next`, `Previous`, `CanGoNext`, `CanGoPrevious`, `LoopStatus`, `Shuffle`). | ✅ Feito | `MprisPlayer` ligado à `Playlist` via `Arc<Mutex<Playlist>>`. `Next` avança a lista e carrega a faixa; `Previous` reinicia o ficheiro se decorridos mais de 3s ou recua na lista; `LoopStatus` mapeia bidirecionalmente `None`, `Track` e `Playlist`; e `Shuffle` sincronizado com D-Bus. Validado com testes unitários dedicados. |

## Critério de saída — cumprido?

**Sim. O critério de saída da Sprint 04 foi integralmente cumprido:**

1. **Troca de faixas de áudio/legenda sem reiniciar o ficheiro:** Os seletores do HUD leem as faixas ativas e efetuam a alternância imediata através das propriedades `aid` e `sid` do `libmpv`, mantendo a posição temporal e a reprodução ininterrupta.
2. **Controlos de vídeo funcionais:** Modificações de `video-aspect-override`, `video-rotate`, `video-crop`, `panscan`, `audio-delay` e `sub-delay` atuam em tempo real sobre a renderização do vídeo.
3. **Equalizador e filtros em tempo real com presets na sessão:** O encadeamento dos filtros `lavfi=[equalizer=...]` e `lavfi=[arnndn]` através da propriedade `af` aplica-se instantaneamente ao fluxo de áudio sem quebras (*glitches*). Os valores dos 10 canais e o preset selecionado persistem durante toda a sessão na estrutura `AudioPanel` (persistência em disco reservada para o `config.rs` na Sprint 05).
4. **Largura lateral fixa e estável:** Largura garantida a 340px exatos, sem variações que forcem redimensionamento ou repintura errática do contexto OpenGL.

## Problemas encontrados e resolução

1. **Formato numérico retornado por `video-aspect-override`:**
   - *Problema:* Ao definir o formato fracionário (ex.: `"16:9"`), o `libmpv` normaliza internamente a propriedade e devolve a string formatada em vírgula flutuante (ex.: `"1.777778"`).
   - *Resolução:* A API `Player::video_aspect_override` passou a retornar `f64` (`<= 0.0` para automático), permitindo uma comparação robusta por tolerância numérica (`(aspect - 16.0/9.0).abs() < 0.05`).
2. **Atualização da API de Panels no egui 0.36.2:**
   - *Problema:* O egui 0.36 unificou `SidePanel` na estrutura genérica `egui::Panel`, substituindo `exact_width(w)` por `exact_size(w)`.
   - *Resolução:* Adaptou-se a declaração do painel lateral em `app.rs` para `egui::Panel::right("vad_uniform_side_panel").exact_size(340.0).resizable(false)`, preservando o comportamento visual pretendido.
3. **Encadeamento de filtros áudio no mpv:**
   - *Problema:* A aplicação de filtros individuais podia sobrepor-se se definida incorretamente.
   - *Resolução:* A função `set_audio_filters` centraliza todas as bandas do equalizador e a redução RNNoise numa única cadeia de filtros separada por vírgulas para a propriedade `af`, limpando-a quando todos os ganhos estão em 0 dB e o RNNoise está desativado.
4. **Garantia de foco nos atalhos de sincronização:**
   - *Problema:* Os novos atalhos `j`/`k` (áudio) e `g`/`h` (legendas) poderiam colidir com a escrita de texto em caixas de entrada futuras.
   - *Resolução:* Todos os novos atalhos foram estritamente protegidos pela verificação `!ctx.egui_wants_keyboard_input()`.

## Dívida técnica / riscos para sprints seguintes

- **Sprint 05 (M2 parte 2):**
  - **Resolução de reprodução de URLs:** Implementar o suporte efetivo a streaming via `yt-dlp` para itens da playlist do tipo `PlaylistItem::Url`, com isolamento seguro de `config-dir` e verificação de esquemas permitidos.
  - **Histórico e Resume:** Criar o módulo `recents.rs` para retomar a reprodução na última posição e manter histórico de ficheiros recentes.
  - **Persistência unificada:** Criar `config.rs` (`~/.config/vad/config.toml`) para persistir entre sessões os ganhos do equalizador, preset ativo, dispositivo de áudio preferido, volume, e proporções de vídeo.
- **Suporte a drag-and-drop na Playlist:** Na Sprint 04, a reordenação é assegurada pelas APIs da playlist (`move_item`), podendo no futuro ser adicionado suporte gráfico para arrastar e largar itens diretamente na lista da interface.
