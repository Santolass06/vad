# Plano de Implementação: "VAD" — Leitor Multimédia Linux com Camada de IA (Rust + libmpv)

## Histórico

Este documento substitui a v1 do plano, que assumia reescrever o motor de descodificação
do zero (FFmpeg FFI ou Rust puro) para "substituir o VLC a 100%". Essa premissa foi
revista em conversa: o objetivo real não é competir em cobertura de codecs — é ter um
leitor mais leve e bonito, com ferramentas de IA/produtividade (transcrição, resumo,
tradução, edição rápida de áudio) que o VLC não tem. Decisão tomada: **motor = libmpv**,
o VAD constrói-se por cima, não ao lado.

---

## 1. Decisões Tomadas

| Decisão | Escolha | Razão |
| :--- | :--- | :--- |
| Motor de reprodução | **libmpv** (`libmpv2` crate) | Sync A/V, hwdec (VA-API/NVDEC), legendas, DVD/Blu-ray, streaming de rede e filtros de DSP já resolvidos e testados em produção. Reescrever isto do zero era anos de trabalho para reimplementar o que já existe. |
| Licenciamento | **VAD em licença GPL-compatível** | libmpv nesta distro é GPLv2+ (linka libavcodec/libavfilter com componentes GPL). Distribuir um binário que depende dele implica que o VAD também deve ser GPL-compatível. Não há forma de manter "100% permissivo" com este motor — decisão aceite, não é um projeto comercial. |
| Arquitetura | **Core desacoplado da UI** (workspace multi-crate) | Pedido explícito: preparar para eventual porte futuro (outra UI, outro SO) sem acoplar tudo ao egui. |
| Âmbito de features de IA v1 | Transcrição (Whisper), skip-silence, bookmarks, corte/exportação de clips, redução de ruído, resumo automático (LLM), tradução de legendas | Definido em conversa como o diferencial real do projeto. |

### Verificado nesta máquina (não é suposição)

```
libmpv-dev 0.41.0-2ubuntu4 — já instalado
libmpv.so.2 linka: libass, libavcodec/avfilter/avformat, libplacebo,
                   librubberband, libdvdnav, libbluray, libmujs (scripting), libarchive
```

Isto confirma, sem precisar de construir nada: **pitch-corrected speed** (rubberband),
**EQ e outros filtros de áudio** (avfilter/lavfi), **legendas ASS/SSA** (libass),
**DVD/Blu-ray** (libdvdnav/libbluray) e **HDR tone-mapping** (libplacebo) vêm de graça
com o motor. O trabalho do VAD é expor isto na UI, não implementá-lo.

Nota à parte, não relacionada com o plano: o `vlc` instalado nesta máquina é a versão
**snap**, e falhou a inicializar o driver gráfico (`libGL error: failed to load driver:
iris`) no teste acima — sintoma comum de snaps a não conseguirem aceder aos drivers Mesa
do sistema. Se quiseres o VLC a funcionar entretanto, o caminho mais rápido costuma ser
trocar para o pacote `.deb`/apt em vez do snap.

Também confirmado: `arnndn` (RNNoise) está compilado no `libavfilter` desta distro —
**redução de ruído também é um filtro do mpv** (`af=arnndn`), não precisa de crate
própria nem de extração manual de PCM.

---

## 2. Comparação VLC vs VAD

Sem números inventados — o baseline real fica registado em M0 (medir o VLC nesta
máquina) em vez de assumido.

| Critério | VLC (medir em M0) | Alvo VAD | Fonte do ganho |
| :--- | :--- | :--- | :--- |
| Arranque | a medir | menor | Rust nativo + egui vs Qt; não há reescrita de motor a acelerar isto |
| RAM idle/playback | a medir | menor | UI mais leve; motor (mpv) é o mesmo custo base que o VLC paga |
| CPU em pausa | a medir | próximo de 0% | `request_repaint_after` (sleep reativo do egui) |
| Codecs/contentores | referência (FFmpeg) | igual | mesmo motor subjacente (mpv usa FFmpeg) — não há diferença aqui |
| Segurança | CVEs periódicos em C | igual ao VLC nesta camada | ambos usam FFmpeg/libavcodec; a "memory safety" só se aplica ao código Rust que escrevermos, não à descodificação |
| Transcrição/IA | inexistente | Whisper + resumo + tradução + skip-silence | diferencial real do projeto |
| Edição rápida | inexistente | corte/exportação de clips na app | diferencial real do projeto |
| UI | Qt datado | HUD flutuante auto-hide, tema escuro | escolha de design, não depende do motor |

---

## 3. Subsistemas: o que o mpv já resolve vs o que a UI precisa expor

Esta é a lista de problemas reais de um leitor de media (identificados na revisão da v1).
Com libmpv, quase nenhum precisa de ser implementado — mas todos precisam de UI/wiring.

| Subsistema | mpv resolve? | Trabalho do VAD |
| :--- | :--- | :--- |
| Sync A/V, VFR, drift | Sim | Nenhum |
| Legendas SRT/ASS/PGS/DVBSub | Sim (libass) | Seletor de faixa, estilo de exibição |
| Passthrough S/PDIF/HDMI (TrueHD/DTS-HD) | Sim | Toggle na UI de áudio |
| Seek sem índice (AVI/TS), seek em HLS | Sim | Nenhum |
| Desentrelaçamento, HDR tone-mapping | Sim (libplacebo) | Toggle/perfil de cor na UI |
| DVD/Blu-ray com menus | Sim (libdvdnav/libbluray) | UI de navegação de menu |
| Hwdec (VA-API/NVDEC) automático | Sim | Indicador no HUD ("Auto: VA-API") |
| Pitch-corrected speed, EQ | Sim (rubberband/lavfi) | Sliders/controlos na UI |
| MPRIS/D-Bus (teclas de media) | **Não.** MPRIS no mpv standalone é um script Lua externo (`mpv-mpris`), não faz parte da libmpv | **100% trabalho novo** — implementar com `zbus`, expectativa básica no Linux, adicionado a M1 |
| Playlists M3U/PLS/XSPF | Parcial | Parser próprio se quisermos formatos fora do que o mpv já lê |
| Redução de ruído (voz) | Sim (`af=arnndn`, RNNoise compilado no `libavfilter` desta distro) | Toggle na UI de áudio |
| Extração de PCM para o Whisper e para a waveform (16kHz mono) | Não — libmpv não expõe isto de forma simples a partir da reprodução ao vivo | Subprocesso `ffmpeg` separado (ver §4.3), desacoplado da reprodução; cache por ficheiro partilhada entre `waveform_pyramid` e `whisper.rs` (ver §4.12, custo de RAM) |
| Delay de áudio/legendas (`audio-delay`, `sub-delay`) | Sim | Slider/atalhos de teclado na UI (equivalente a j/k/g/h do VLC) |
| Aspect ratio, crop, rotação (`video-aspect-override`, `video-crop`, `video-rotate`) | Sim | Controlos no painel de vídeo |
| Reprodução direta por URL (YouTube/Twitch via yt-dlp) | Sim, em princípio — confirmado nesta máquina: `libmpv.so` linka `liblua5.2` (o `ytdl_hook` interno do mpv é Lua, não mujs) e `yt-dlp` já está instalado. **Por confirmar em M2:** a libmpv, ao contrário do binário standalone, não carrega config/scripts por omissão — é preciso ativar isso explicitamente em `player.rs`, e ao fazê-lo isolar `config-dir` numa pasta própria do VAD, não a do utilizador (`~/.config/mpv`), para não herdar `hwdec`/`vo`/`af` de fora | Aceitar URL na caixa "Abrir"; **`yt-dlp` passa a dependência de runtime**, tal como o `ffmpeg` |

---

## 4. Constrangimentos descobertos (decisões em aberto, não assumidos em silêncio)

1. **Tradução de legendas em tempo real.** O modo `translate` do Whisper só traduz
   **para inglês** — não existe tradução PT→outro idioma nativa no Whisper.
   - **v1:** tradução apenas para inglês (uma chamada Whisper, offline, sem modelo extra).
   - **M5 (revisto):** em vez de um segundo modelo dedicado (NLLB), reutilizar o mesmo
     LLM local do resumo (ver ponto 2) para traduzir PT↔qualquer idioma via prompt —
     poupa uma dependência de inferência inteira. **Condição de aceitação antes de
     adotar isto:** correr 20 segmentos reais do Whisper pelo LLM candidato e validar
     manualmente que não há alucinação nem enchimento de texto — modelos pequenos
     (0.5B–1B) são conhecidos por falhar exatamente em fragmentos curtos e sem
     contexto, que é a forma de uma legenda. Se falhar no teste, volta-se à opção de
     um modelo de tradução dedicado.

2. **Resumo automático (LLM).** Consistente com a filosofia de privacidade já presente
   no plano (Whisper RAM-only), a recomendação é um **modelo local pequeno em GGUF**
   (`Qwen2.5-0.5B-Instruct-Q4_K_M` ~350 MB, ou `Llama-3.2-1B-Instruct-Q4_K_M` ~700 MB,
   via `llama-cpp-rs` ou `candle`) em vez de API cloud. Isto é uma recomendação, não uma
   decisão tua — API cloud é mais simples de implementar e dá melhores resumos, mas
   contradiz a filosofia "zero-disco/privacidade" do resto do documento. **Nota de
   RAM:** este modelo soma-se aos ~55 MB do Whisper quantizado (ver §5) — o footprint
   total do M5 fica bem acima do resto da app; isto é um custo aceite pela feature, não
   um erro na tabela de otimizações.

3. **Mecanismo de renderização de vídeo.** Incorporar o vídeo por `wid` (janela nativa
   X11) **não funciona em Wayland** — não é uma questão de estabilidade, é uma
   limitação do libmpv nesse protocolo. **Decisão:** usar a `mpv_render_context` API
   do libmpv com backend OpenGL, integrada num `egui_glow::CallbackFn` — o mpv
   renderiza para uma textura/FBO e o egui desenha o HUD por cima na mesma passagem.
   Isto funciona em X11 e Wayland sem código diferente por plataforma. **Restrição a
   respeitar em `player.rs`:** o contexto OpenGL tem de estar current na thread que
   chama a render API — a renderização do vídeo fica presa à thread de UI (dentro do
   callback de pintura do eframe), só os *eventos* do mpv (ponto 8 da arquitetura, via
   canal) correm numa thread separada. As duas coisas não são a mesma decisão.

4. **Notas de reunião com timestamps clicáveis.** A versão barata — exportar
   `[00:04:12]` como texto simples no `.md` — entra em M3 sem custo extra. A versão
   com esquema de URI clicável (`vad://seek?t=252`) para abrir a partir do
   Obsidian/Logseq exige duas peças adicionais: um `.desktop` com
   `x-scheme-handler/vad`, e **instância única do processo** (um segundo lançamento
   de `vad` tem de enviar o comando de seek à janela já aberta em vez de abrir outra).
   A segunda peça é a cara. **Decisão para v1:** timestamps em texto simples em M3; o
   esquema de URI fica como item separado pós-M6, condicionado a implementar
   instância única.

5. **Instância única do processo.** A mesma pergunta aparece em dois sítios: no CLI
   (`vad ficheiro.mkv` — abre janela nova ou reutiliza a existente?) e no ponto 4
   acima. **Decisão para v1:** cada execução de `vad` abre uma janela nova (mais
   simples, sem IPC). Instância única só entra em âmbito se/quando o esquema de URI do
   ponto 4 for implementado — decide-se uma vez, não duas.

6. **Lembrar posição de reprodução (resume).** Isto exige guardar estado entre
   execuções — **não é o mesmo objetivo que o "zero-disk footprint"** do plano original,
   que se referia especificamente aos buffers de descodificação e ao modelo Whisper
   (por privacidade/discos cheios). Guardar os últimos ~20 ficheiros + timestamp num
   ficheiro de configuração pequeno (`~/.config/vad/recentes.json`) é comportamento
   normal de qualquer app e não contradiz essa filosofia. Não delegar isto ao
   `watch-later` nativo do mpv — o VAD precisa de mostrar o próprio diálogo
   "continuar de onde parou" à sua maneira, não a semântica de ficheiro do mpv.

7. **Inibidor de suspensão de ecrã.** Sem isto, o ecrã apaga-se a meio de um vídeo
   longo sem interação do rato/teclado — falha visível logo na primeira demonstração
   de M1 (qualquer teste real de "play e não tocar no rato por 5 minutos" apanha isto
   se ficar para depois). **Movido para M1**, não M2: usa `zbus`, a mesma dependência
   já necessária para o MPRIS no mesmo milestone — é código incremental, não uma
   integração nova. Implementar via `org.freedesktop.ScreenSaver.Inhibit`, ativo
   apenas durante reprodução de vídeo/áudio, liberta ao pausar.

8. **Bandeja de sistema (system tray).** Útil para "fechar a janela, o áudio da
   reunião continua a tocar". Mesmo padrão de registo de serviço D-Bus do MPRIS/
   screensaver — mas com um passo adicional (protocolo `StatusNotifierItem`, crate
   `ksni`). **Risco a registar:** funciona bem em KDE, mas o GNOME *não mostra* ícones
   de bandeja por omissão sem uma extensão do utilizador (AppIndicator/
   KStatusNotifierItem Support). Tratar como best-effort, não como garantia universal
   — documentar isto na própria app em vez de o utilizador achar que está avariado.

9. **Picture-in-Picture / Always-on-Top.** Já está no mockup do HUD mas falta o
   mecanismo: `window.set_window_level` via `winit`/`eframe`. **Risco a registar:**
   fiável em X11; em Wayland depende do compositor — o GNOME Wayland, por exemplo,
   não expõe always-on-top a aplicações cliente por restrição do protocolo. Mesmo
   tratamento do ponto 8: best-effort, comunicado na UI.

10. **Descarregamento automático de legendas (OpenSubtitles).** Fica como stretch
    goal pós-M6, tal como sugerido. **Nota que não é zero-custo:** a API pública da
    OpenSubtitles hoje exige registo/chave de API e tem limites de taxa — não é uma
    integração livre de fricção como era a extensão VLSub há alguns anos.

11. **Lifetime do buffer do modelo Whisper em RAM-only.** `whisper_init_from_buffer`
    **não copia o modelo** — o whisper.cpp/ggml lê os tensores diretamente do buffer
    do chamador (é por isso que o RAM-only fica em ~55 MB e não ~110 MB). Logo, um
    `Vec<u8>` normal é um risco real de use-after-free em C se algo o realocar
    enquanto o contexto de inferência existir — não é um exagero de segurança, é a
    forma correta de usar esta API. **Decisão:** o buffer do modelo tem de ser um
    `Arc<[u8]>` (slice imutável e fixo), nunca um `Vec<u8>` a que outro código tenha
    acesso mutável. Isto aplica-se só ao buffer do *modelo*; o buffer de *PCM*
    (ponto 12) é outra coisa, sem este constrangimento.

12. **Cache do PCM extraído: partilhado entre waveform e Whisper, mas com custo de
    RAM a registar.** A `waveform_pyramid` (visualização, no abrir do ficheiro) e o
    `whisper.rs` (transcrição, sob pedido do utilizador) usam a mesma extração de
    PCM do `extractor.rs` — mas não ao mesmo tempo, por isso o mecanismo é uma
    **cache por ficheiro**, não uma chamada única com dois consumidores. Custo real:
    PCM 16kHz mono em `f32` é ~230 MB por hora de áudio — uma reunião de 3h fica com
    ~700 MB retidos, mais do que o LLM do M5. **Mitigação:** guardar a cache em
    `i16` em vez de `f32` (metade do custo; o Whisper converte para f32 ao ingerir, e
    o waveform min/max não precisa de mais precisão). Este custo entra no ledger de
    RAM do §5, não é ignorado.

13. **Modo de armazenamento dos modelos Whisper: escolha do utilizador, não
    imposição.** RAM-only é a opção certa para um caso específico (uso pontual,
    privacidade extrema, disco cheio) — não deve ser o único caminho nem o
    predefinido silencioso. **Decisão:** o `whisper_panel.rs` oferece, por modelo,
    duas ações explícitas:
    - **"Guardar no disco"** (predefinição) — persistente em
      `~/.local/share/vad/models/`; não volta a descarregar da próxima vez que for
      ativado, e o whisper.cpp carrega por `mmap` a partir do ficheiro, o que usa
      *menos* RAM residente do que manter o modelo todo em memória (as páginas são
      partilhadas e recuperáveis sob pressão de memória).
    - **"Usar só nesta sessão (RAM-only)"** (opt-in explícito) — nada é escrito no
      disco; tem de ser descarregado outra vez sempre que for usado, e o modelo fica
      inteiro em memória enquanto ativo (ver §4.11).

    Ao passar o rato sobre cada opção, um tooltip explica o tradeoff em vez de
    obrigar o utilizador a adivinhar:
    - *RAM-only:* "Nada fica no disco. Ideal para privacidade extrema ou pouco
      espaço livre. Tens de descarregar de novo (o tamanho do modelo escolhido, ex.
      ~55 MB no `base-q5`) sempre que usares, e ocupa esse espaço em RAM enquanto
      estiver ativo."
    - *Disco:* "Guardado em `~/.local/share/vad/models/`. Mais rápido a ativar da
      próxima vez e usa menos RAM (carregado por mmap), mas ocupa espaço em disco
      até apagares manualmente."

    Arquitetura: `model_manager.rs` (novo, em `vad-ai`) decide o destino do download
    — ficheiro ou `Arc<[u8]>` em memória (ver §4.11) — consoante a escolha; ambos os
    caminhos alimentam `whisper.rs` da mesma forma (a diferença fica isolada nesta
    camada, não se propaga ao resto da app).

### Fora de âmbito, por decisão deliberada

Para não serem reintroduzidas mais tarde sem motivo — features do VLC que ficam de
fora, com a razão:

- **Servidor de streaming (RTSP/HTTP broadcast)** — hoje o caminho normal é OBS/nginx-rtmp.
- **Sintonizador de TV analógica/DVB-T** — hardware praticamente extinto.
- **CD de áudio (`cdda://`)** — leitores de CD físico já não existem na generalidade das máquinas.
- **Efeitos de vídeo gimmick** (espelho, puzzle, ondulação) — sem utilidade real.
- **Transcodificador geral** — o corte/exportação via `ffmpeg` (§8) cobre o caso que interessa, sem o menu "Converter/Guardar" cheio de erros do VLC.

---

## 5. Otimizações da Camada de Aplicação

Com o motor partilhado com o VLC (ambos correm sobre FFmpeg via mpv), **"mais leve e
mais rápido" só pode vir da camada Rust que construímos por cima.** É aqui que fica o
esforço real de performance do projeto:

- **`mimalloc`** como alocador global — reduz contenção de locks em heaps com múltiplas
  threads. (Não elimina leaks — apenas reduz fragmentação/contenção.)
- **Perfil de release**: `lto = "fat"`, `codegen-units = 1`, `strip = true`.
  **Sem `panic = "abort"`** — contradiz o critério de M1 "erro tratado sem crash da
  app": um ficheiro corrompido não pode abortar o processo.
- **Whisper quantizado** (`q5_0` / `q5_1`, nomenclatura correta do whisper.cpp/ggml):
  modelo `base` cai de 142 MB para ~55 MB de RAM. É a maior alavanca de RAM do
  playback/transcrição — usar por omissão no caminho RAM-only do `whisper.rs`
  (o caminho disco, predefinido, usa ainda menos RAM residente via `mmap`, ver §4.13).
- **Waveform pyramid**: 3 níveis de resolução pré-computados em background (visão
  global, 5 min, 10s); a UI renderiza no máximo ~1000 pontos visíveis, independente da
  duração do ficheiro.

**Ledger de RAM (não esconder o custo do M5):** Whisper `base-q5` (~55 MB) + LLM local
de resumo/tradução (~350-700 MB, ver §4.2) somam-se quando ambos estão carregados. O
resto da app (mpv + egui) fica bem abaixo disto — o M5 é, de longe, o maior consumidor
de RAM do projeto, não uma otimização.

**Cache de PCM (waveform + Whisper, ver §4.12):** ~115 MB/hora de áudio em `i16`
(~230 MB/hora se fosse `f32`) enquanto o ficheiro está aberto. Transitório — liberta-se
ao fechar o ficheiro — mas para uma reunião de várias horas é um consumo visível, não
zero.

---

## 6. Design da Interface

UI escura, HUD flutuante auto-ocultável (2s), sem os elementos "AVX2" do plano anterior
(não há mais deteção/otimização SIMD própria) — mantém-se `Auto: VA-API`, que agora é
uma leitura real do hwdec ativo no mpv, não uma aspiração. Com `hwdec=auto-safe`
(§4/§7) o indicador tem de ler a propriedade `hwdec-current`, não a pedida — se o
driver falhar (como aconteceu ao VLC snap nesta máquina) o mpv comuta sozinho para
software e o HUD mostra `SW (CPU)` em vez de continuar a exibir "VA-API" a mentir.

### Mockup: Modo Vídeo com HUD Ativo

```text
+---------------------------------------------------------------------------------------------------+
|  [VAD]  reuniao_estrategica.mp4                          [Auto: VA-API]       [—]  [□]  [✕]       |
+---------------------------------------------------------------------------------------------------+
|                                                                                                   |
|                                    [ CANVAS DE VÍDEO 4K / 1080p ]                                 |
|                                                                                                   |
|                            Legenda: "Vamos então analisar o relatório financeiro..."              |
|                                                                                                   |
|  +---------------------------------------------------------------------------------------------+  |
|  | [HUD Flutuante Auto-Hide]                                                                   |  |
|  |  00:14:23 / 01:30:00  [===●===========================================]  -01:15:37          |  |
|  |                                                                                             |  |
|  |  [⏮ 10s] [⏪ 5s]   [ ▶ / ⏸ Play ]   [5s ⏩] [10s ⏭]   [🔁 A-B]   [📸 Frame]   [🎙 Whisper]   |  |
|  |                                                                                             |  |
|  |  Velocidade: [1.25x ▾]   Áudio: [Faixa 1 (PT) ▾]   Legendas: [PT (Auto) ▾]   Vol: [🔊 120%] |  |
|  +---------------------------------------------------------------------------------------------+  |
+---------------------------------------------------------------------------------------------------+
```

### Mockup: Modo Áudio / Reunião & Painel Lateral

```text
+---------------------------------------------------------------------------------------------------+
|  [VAD]  entrevista_direcao.opus                                                      [—]  [□]  [✕]       |
+---------------------------------------------------------------------------------------------------+-----------------+
|  Visualizador de Forma de Onda (Waveform da Reunião com MIP-Mapping)            | [X] Painel      |
|  +---------------------------------------------------------------------------+  | --------------- |
|  |   || |||| | | ||||||||||||||||||||| | | | ||||||||||||||||| | | ||||||||| |  | [🎙 Whisper AI] |
|  |===||=||||=|=|======================[▲]====================================|  | [📑 Playlist]  |
|  +---------------------------------------------------------------------------+  | [🎛 Equalizador]|
|                                                                                 | [🎨 Cores Vídeo]|
|  Controlos Rápidos de Reunião:                                                  | --------------- |
|  [ Saltar Silêncios: ATIVO ]  [ Redução de Ruído: ATIVO ]  [ Speed: 1.5x ]     | Modelo:         |
|                                                                                 | [ base-q5 (~55M)▼
|  Marcadores da Reunião (Bookmarks & Notas):             [ + Adicionar Nota ]    | [x] RAM-Only    |
|  • [00:04:12] Introdução dos objetivos do projeto             [Ir] [Editar]     | (Sem disco)     |
|  • [00:18:45] Discussão do orçamento de TI                    [Ir] [Editar]     |                 |
|  • [00:32:10] Aprovação da transição para Rust                [Ir] [Editar]     | Modelos no PC:  |
|                                                                                 | • tiny [Load][🗑]
|  [ 📄 Exportar Notas e Transcrição para Markdown (.md) ]                        | • base [Ativo][🗑
|                                                                                 |                 |
|                                                                                 | Transcrição:    |
|                                                                                 | [00:04:12]      |
|                                                                                 | "Objetivos..."  |
|                                                                                 | [Exportar .md]  |
+---------------------------------------------------------------------------------------------------+-----------------+
```

---

## 7. Arquitetura do Workspace

```text
vad/
├── Cargo.toml                    # workspace
├── crates/
│   ├── vad-core/                 # motor: embutir libmpv, estado, playlist — SEM deps de UI
│   │   ├── src/
│   │   │   ├── player.rs         # wrapper sobre libmpv2 (mpv_render_context/OpenGL; hwdec=auto-safe c/ leitura de hwdec-current; play/pause/seek/tracks/filtros/delay/aspect/crop/rotate)
│   │   │   ├── state.rs          # eventos do mpv (thread interna em C) -> crossbeam-channel/watch para a UI
│   │   │   ├── playlist.rs       # shuffle/repeat, URLs (yt-dlp) além de ficheiros locais
│   │   │   ├── recents.rs        # últimos ~20 ficheiros + timestamp p/ "continuar de onde parou" (ver §4.6)
│   │   │   └── bookmarks.rs      # notas de reunião exportáveis (.md, timestamps em texto simples)
│   │   └── Cargo.toml
│   │
│   ├── vad-ai/                   # transcrição, VAD, resumo, tradução — SEM deps de UI
│   │   ├── src/
│   │   │   ├── extractor.rs      # subprocesso ffmpeg -> PCM 16kHz mono i16, cache por ficheiro (waveform + Whisper, ver §4.12)
│   │   │   ├── model_manager.rs  # escolha do utilizador: disco (~/.local/share/vad/models/) ou RAM-only (ver §4.13)
│   │   │   ├── whisper.rs        # transcriber (whisper.cpp bindings); caminho RAM-only em Arc<[u8]> pinado, nunca Vec<u8> (ver §4.11); caminho disco carrega por path
│   │   │   ├── vad_detector.rs   # deteção de silêncio antes do Whisper
│   │   │   ├── summarizer.rs     # LLM local (GGUF) para resumo da transcrição
│   │   │   └── translator.rs     # Whisper translate (EN) + M5: mesmo LLM do summarizer p/ outros idiomas
│   │   └── Cargo.toml           # nota: redução de ruído é af=arnndn no mpv, não crate própria
│   │
│   ├── vad-audio-tools/          # corte/exportação de clips, waveform pyramid
│   │   ├── src/
│   │   │   ├── clip_export.rs    # subprocesso ffmpeg -c copy (ver §8)
│   │   │   └── waveform_pyramid.rs
│   │   └── Cargo.toml
│   │
│   └── vad-app/                  # binário: egui + integração dos crates acima
│       ├── src/
│       │   ├── main.rs           # clap (abrir ficheiro por argumento, --fullscreen); probe_dependencies (ffmpeg/yt-dlp no $PATH)
│       │   ├── app.rs            # loop reativo (request_repaint_after); drag-and-drop (egui raw.dropped_files); atalhos globais só se !ctx.wants_keyboard_input()
│       │   ├── render.rs         # egui_glow::CallbackFn que invoca a mpv_render_context na thread de UI
│       │   ├── mpris.rs          # org.mpris.MediaPlayer2[.Player] via zbus — Metadata: trackid/title/artist/length
│       │   ├── screensaver.rs    # org.freedesktop.ScreenSaver.Inhibit via zbus (ver §4.7)
│       │   ├── tray.rs           # StatusNotifierItem via ksni — best-effort, ver §4.8
│       │   ├── theme.rs
│       │   └── panels/
│       │       ├── hud.rs
│       │       ├── whisper_panel.rs
│       │       ├── playlist_panel.rs
│       │       ├── audio_panel.rs
│       │       └── video_panel.rs    # cor, aspect/crop/rotação, delay A/V — já estava no mockup, faltava no código
│       └── Cargo.toml
```

`vad-core` e `vad-ai` não dependem de `egui` — é o que permite, no futuro, trocar a UI
ou portar para outro SO sem tocar na lógica. Nota: a renderização do vídeo (`render.rs`)
fica presa à thread de UI por exigência da `mpv_render_context` (contexto OpenGL
current); só os eventos de estado do mpv correm em canal — não é a mesma decisão.

---

## 8. Decisão em aberto: mecanismo de corte/exportação de clips (M4)

`libmpv` descodifica; não faz mux/encode de um novo ficheiro. "Cortar/extrair clips"
precisa de um mecanismo próprio — duas opções:

- **Subprocesso `ffmpeg` (CLI)** — simples, stream-copy (`-c copy`) é uma linha,
  dependência de runtime já presente no sistema (o mpv já depende de FFmpeg).
- **`ffmpeg-next` (FFI)** — evita o subprocesso, mas reintroduz a superfície `unsafe`
  que o resto do plano evita ao usar libmpv.

*Recomendação: subprocesso CLI para v1 — mais simples, sem `unsafe` extra, e stream-copy
cobre o caso comum (cortar sem recodificar).*

```
ffmpeg -ss {inicio} -to {fim} -i {input} -c copy -avoid_negative_ts 1 {output}
```

**Tradeoff a mostrar na UI, não descobrir em M4:** com `-ss` antes de `-i` e `-c copy`,
o corte encaixa no keyframe mais próximo — os limites do clip não são exatos ao frame.
Para corte exato seria preciso recodificar (perde a vantagem de velocidade/qualidade
deste mecanismo). Aceitar o corte por keyframe como comportamento do v1.

---

## 9. Fases e Milestones

| Fase | Entregável | Critério de aceitação |
| :--- | :--- | :--- |
| **M0** | Medir baseline real do VLC nesta máquina (RSS, arranque, CPU em pausa) | Números registados, substituem os "a medir" da tabela |
| **M1** | `vad-core` embutindo libmpv via `mpv_render_context`/OpenGL (obrigatório em Wayland) com `hwdec=auto-safe`, HUD básico, MPRIS + inibidor de screensaver (mesma base `zbus`, ver §4.7), CLI (clap) + drag-and-drop, `probe_dependencies` (ffmpeg/yt-dlp), guarda de foco de teclado (`wants_keyboard_input`); decisão de janela nova por execução (sem instância única) | Play/pause/seek/volume funcionam em X11 **e** Wayland sem flicker; HUD mostra o `hwdec-current` real, incluindo fallback `SW (CPU)`; teclas de media do sistema funcionam; ecrã não suspende durante playback; app arranca e degrada graciosamente (botões desativados) sem `ffmpeg`/`yt-dlp` instalados; `vad ficheiro.mkv` e arrastar ficheiro abrem reprodução |
| **M2** | Playlist (+ shuffle/repeat), faixas de áudio/legendas, hwdec visível no HUD, `video_panel.rs` (delay A/V, aspect/crop/rotação), reprodução por URL (yt-dlp — confirmar isolamento de `config-dir`, ver §3), resume playback (`recents.rs`) | Troca de faixa sem reiniciar; indicador de aceleração correto; URL do YouTube reproduz sem herdar `~/.config/mpv` do utilizador; reabrir a app oferece continuar o último ficheiro |
| **M3** | Whisper (via `extractor.rs`/ffmpeg desacoplado) + `model_manager.rs` com escolha disco/RAM-only e tooltips (ver §4.13) + VAD skip-silence + bookmarks exportáveis em .md (timestamps em texto simples) | Transcrição de um ficheiro de reunião real sem interromper outra reprodução; utilizador escolhe e vê o tradeoff antes de descarregar um modelo; notas exportadas |
| **M4** | Corte/exportação de clips (via ffmpeg CLI, ver §8), redução de ruído (af=arnndn) | Selecionar troço na waveform, exportar ficheiro válido (corte por keyframe aceite) |
| **M5** | Resumo automático (LLM local GGUF) + tradução (EN via Whisper; PT/outros via mesmo LLM) | Resumo gerado a partir de transcrição; **gate de aceitação:** 20 segmentos reais traduzidos pelo LLM revistos manualmente sem alucinação/enchimento antes de expor a feature |
| **M6** | Polish (tema, animações), bandeja de sistema e PIP/always-on-top (`tray.rs`, best-effort — ver §4.8/4.9), perfil de release, empacotamento (Flatpak) | Binário instalável, arranque e RAM medidos e comparados ao M0 |
| **Pós-M6** | Esquema de URI `vad://` + instância única (§4.4/4.5); legendas automáticas via OpenSubtitles (§4.10) | Stretch goals, sem data comprometida |

---

## 10. Plano de Verificação

### Testes automatizados
1. `vad-core`: testes de integração reproduzindo ficheiros de amostra (corpus FATE +
   pelo menos 2 ficheiros deliberadamente corrompidos, para validar que erro é tratado
   sem crash da app).
2. `vad-ai`: teste do VAD detector com áudio sintético (silêncio conhecido) validando
   que não corta início/fim de fala.
3. Validação RAM-only: carregar modelo Whisper para `Arc<[u8]>` (ver §4.11), confirmar
   zero ficheiros novos em `~/.local/share/vad`, `~/.cache` e `/tmp`.
4. Validação modo disco: carregar modelo via `model_manager.rs` com "Guardar no disco",
   confirmar ficheiro criado em `~/.local/share/vad/models/` e reutilizado (sem novo
   download) na ativação seguinte.

### Verificação manual
1. Comparar RSS e CPU em pausa contra o baseline medido em M0.
2. Reproduzir um DVD/Blu-ray de teste e confirmar navegação de menu.
3. Transcrever um áudio de reunião com silêncios e validar tempo de transcrição vs
   duração real.
4. Testar teclas de media do teclado/sistema (MPRIS) com a app em segundo plano.
5. No painel de modelos, confirmar que os tooltips de "Guardar no disco" e "RAM-only"
   aparecem ao passar o rato e que a escolha é respeitada (ficheiro criado vs nenhum).
