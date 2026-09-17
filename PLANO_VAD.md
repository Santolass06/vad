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

---

## 4. Constrangimentos descobertos (decisões em aberto, não assumidos em silêncio)

1. **Tradução de legendas em tempo real.** O modo `translate` do Whisper só traduz
   **para inglês** — não existe tradução PT→outro idioma nativa no Whisper. Duas opções:
   - **v1:** tradução apenas para inglês (uma chamada Whisper, offline, sem modelo extra).
   - **Stretch goal:** adicionar um modelo de tradução local separado (ex. NLLB pequeno)
     para PT↔qualquer idioma. Mais RAM, mais complexidade, mais tempo de dev.

   *Por decidir: aceitas EN-only no v1, ou a tradução multi-idioma é suficientemente
   importante para justificar um segundo modelo já no v1?*

2. **Resumo automático (LLM).** Consistente com a filosofia de privacidade já presente
   no plano (Whisper RAM-only), a recomendação é um **modelo local pequeno** (via
   `candle` ou `llama.cpp`/GGUF) em vez de API cloud. Isto é uma recomendação, não uma
   decisão tua — API cloud é mais simples de implementar e dá melhores resumos, mas
   contradiz a filosofia "zero-disco/privacidade" do resto do documento.

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
  modelo `base` cai de 142 MB para ~55 MB de RAM. É a maior alavanca de RAM do projeto
  inteiro — usar por omissão no `whisper.rs` / RAM-only loader.
- **Waveform pyramid**: 3 níveis de resolução pré-computados em background (visão
  global, 5 min, 10s); a UI renderiza no máximo ~1000 pontos visíveis, independente da
  duração do ficheiro.

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
│   │   │   ├── player.rs         # wrapper sobre libmpv2 (play/pause/seek/tracks/filtros)
│   │   │   ├── state.rs          # estado de reprodução observável (canais/eventos)
│   │   │   ├── playlist.rs
│   │   │   └── bookmarks.rs      # notas de reunião exportáveis (.md)
│   │   └── Cargo.toml
│   │
│   ├── vad-ai/                   # transcrição, VAD, resumo, tradução — SEM deps de UI
│   │   ├── src/
│   │   │   ├── whisper.rs        # transcriber (whisper.cpp bindings), RAM-only loader
│   │   │   ├── vad_detector.rs   # deteção de silêncio antes do Whisper
│   │   │   ├── summarizer.rs     # LLM local para resumo da transcrição
│   │   │   └── translator.rs     # Whisper translate (EN) + stretch goal multi-idioma
│   │   └── Cargo.toml           # nota: redução de ruído é af=arnndn no mpv, não crate própria
│   │
│   ├── vad-audio-tools/          # corte/exportação de clips, waveform pyramid
│   │   ├── src/
│   │   │   ├── clip_export.rs
│   │   │   └── waveform_pyramid.rs
│   │   └── Cargo.toml
│   │
│   └── vad-app/                  # binário: egui + integração dos crates acima
│       ├── src/
│       │   ├── main.rs
│       │   ├── app.rs            # loop reativo (request_repaint_after)
│       │   ├── mpris.rs          # integração D-Bus/MPRIS (teclas de media)
│       │   ├── theme.rs
│       │   └── panels/
│       │       ├── hud.rs
│       │       ├── whisper_panel.rs
│       │       ├── playlist_panel.rs
│       │       └── audio_panel.rs
│       └── Cargo.toml
```

`vad-core` e `vad-ai` não dependem de `egui` — é o que permite, no futuro, trocar a UI
ou portar para outro SO sem tocar na lógica.

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

---

## 9. Fases e Milestones

| Fase | Entregável | Critério de aceitação |
| :--- | :--- | :--- |
| **M0** | Medir baseline real do VLC nesta máquina (RSS, arranque, CPU em pausa) | Números registados, substituem os "a medir" da tabela |
| **M1** | `vad-core` embutindo libmpv, reproduz ficheiro local, HUD básico, MPRIS | Play/pause/seek/volume funcionam; teclas de media do teclado funcionam |
| **M2** | Playlist, faixas de áudio/legendas, hwdec visível no HUD | Troca de faixa sem reiniciar; indicador de aceleração correto |
| **M3** | Whisper + VAD skip-silence + bookmarks exportáveis em .md | Transcrição de um ficheiro de reunião real, notas exportadas |
| **M4** | Corte/exportação de clips (via ffmpeg CLI), redução de ruído (af=arnndn) | Selecionar troço na waveform, exportar ficheiro válido |
| **M5** | Resumo automático (LLM local) + tradução (EN via Whisper) | Resumo gerado a partir de transcrição; legendas EN geradas |
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
