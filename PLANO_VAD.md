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
| Extração de PCM para o Whisper (16kHz mono) | Não — libmpv não expõe isto de forma simples a partir da reprodução ao vivo | Subprocesso `ffmpeg` separado (ver §4.3), desacoplado da reprodução |

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
  playback/transcrição — usar por omissão no `whisper.rs` / RAM-only loader.
- **Waveform pyramid**: 3 níveis de resolução pré-computados em background (visão
  global, 5 min, 10s); a UI renderiza no máximo ~1000 pontos visíveis, independente da
  duração do ficheiro.

**Ledger de RAM (não esconder o custo do M5):** Whisper `base-q5` (~55 MB) + LLM local
de resumo/tradução (~350-700 MB, ver §4.2) somam-se quando ambos estão carregados. O
resto da app (mpv + egui) fica bem abaixo disto — o M5 é, de longe, o maior consumidor
de RAM do projeto, não uma otimização.

---

## 6. Design da Interface

UI escura, HUD flutuante auto-ocultável (2s), sem os elementos "AVX2" do plano anterior
(não há mais deteção/otimização SIMD própria) — mantém-se `Auto: VA-API`, que agora é
uma leitura real do hwdec ativo no mpv, não uma aspiração.

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
│   │   │   ├── player.rs         # wrapper sobre libmpv2 (mpv_render_context/OpenGL; play/pause/seek/tracks/filtros)
│   │   │   ├── state.rs          # eventos do mpv (thread interna em C) -> crossbeam-channel/watch para a UI
│   │   │   ├── playlist.rs
│   │   │   └── bookmarks.rs      # notas de reunião exportáveis (.md, timestamps em texto simples)
│   │   └── Cargo.toml
│   │
│   ├── vad-ai/                   # transcrição, VAD, resumo, tradução — SEM deps de UI
│   │   ├── src/
│   │   │   ├── extractor.rs      # subprocesso ffmpeg -> PCM f32 16kHz mono, desacoplado da reprodução
│   │   │   ├── whisper.rs        # transcriber (whisper.cpp bindings), RAM-only loader
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
│       │   ├── main.rs           # clap (abrir ficheiro por argumento, --fullscreen)
│       │   ├── app.rs            # loop reativo (request_repaint_after); drag-and-drop (egui raw.dropped_files)
│       │   ├── render.rs         # egui_glow::CallbackFn que invoca a mpv_render_context na thread de UI
│       │   ├── mpris.rs          # org.mpris.MediaPlayer2[.Player] via zbus — Metadata: trackid/title/artist/length
│       │   ├── theme.rs
│       │   └── panels/
│       │       ├── hud.rs
│       │       ├── whisper_panel.rs
│       │       ├── playlist_panel.rs
│       │       └── audio_panel.rs
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
| **M1** | `vad-core` embutindo libmpv via `mpv_render_context`/OpenGL (obrigatório em Wayland), HUD básico, MPRIS completo (zbus), CLI (clap) + drag-and-drop; decisão de janela nova por execução (sem instância única) | Play/pause/seek/volume funcionam em X11 **e** Wayland sem flicker; teclas de media do sistema funcionam; `vad ficheiro.mkv` e arrastar ficheiro abrem reprodução |
| **M2** | Playlist, faixas de áudio/legendas, hwdec visível no HUD | Troca de faixa sem reiniciar; indicador de aceleração correto |
| **M3** | Whisper (via `extractor.rs`/ffmpeg desacoplado) + VAD skip-silence + bookmarks exportáveis em .md (timestamps em texto simples) | Transcrição de um ficheiro de reunião real sem interromper outra reprodução; notas exportadas |
| **M4** | Corte/exportação de clips (via ffmpeg CLI, ver §8), redução de ruído (af=arnndn) | Selecionar troço na waveform, exportar ficheiro válido (corte por keyframe aceite) |
| **M5** | Resumo automático (LLM local GGUF) + tradução (EN via Whisper; PT/outros via mesmo LLM) | Resumo gerado a partir de transcrição; **gate de aceitação:** 20 segmentos reais traduzidos pelo LLM revistos manualmente sem alucinação/enchimento antes de expor a feature |
| **M6** | Polish (tema, animações), perfil de release, empacotamento (Flatpak) | Binário instalável, arranque e RAM medidos e comparados ao M0 |

---

## 10. Plano de Verificação

### Testes automatizados
1. `vad-core`: testes de integração reproduzindo ficheiros de amostra (corpus FATE +
   pelo menos 2 ficheiros deliberadamente corrompidos, para validar que erro é tratado
   sem crash da app).
2. `vad-ai`: teste do VAD detector com áudio sintético (silêncio conhecido) validando
   que não corta início/fim de fala.
3. Validação RAM-only: carregar modelo Whisper para `Vec<u8>`, confirmar zero ficheiros
   novos em `~/.cache` e `/tmp`.

### Verificação manual
1. Comparar RSS e CPU em pausa contra o baseline medido em M0.
2. Reproduzir um DVD/Blu-ray de teste e confirmar navegação de menu.
3. Transcrever um áudio de reunião com silêncios e validar tempo de transcrição vs
   duração real.
4. Testar teclas de media do teclado/sistema (MPRIS) com a app em segundo plano.
