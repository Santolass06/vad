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
| Licenciamento | **Binário final em licença GPL-compatível** | libmpv nesta distro é GPLv2+ (linka libavcodec/libavfilter com componentes GPL). Distribuir um binário que depende dele implica que o **binário distribuído** do VAD também deve ser GPL-compatível — decisão aceite, não é um projeto comercial. **Clarificação:** isto é sobre a obra distribuída, não sobre cada ficheiro de código; um crate sem dependências GPL (ex. `llm_provider.rs`, se isolado) pode ser lançado à parte sob MIT se algum dia fizer sentido para reutilização — não bloqueia isso, só o binário `vad` completo é sempre GPL. |
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
| Reprodução direta por URL (YouTube/Twitch via yt-dlp) | Sim, em princípio — confirmado nesta máquina: `libmpv.so` linka `liblua5.2` (o `ytdl_hook` interno do mpv é Lua, não mujs) e `yt-dlp` já está instalado. **Por confirmar em M2:** a libmpv, ao contrário do binário standalone, não carrega config/scripts por omissão — é preciso ativar isso explicitamente em `player.rs`, isolar `config-dir` numa pasta própria do VAD (não `~/.config/mpv`), **e passar `--no-config` explicitamente** — isolar o `config-dir` sozinho não chega, o `ytdl_hook` e outros scripts podem ainda ler de localizações por omissão fora dessa pasta | Aceitar URL na caixa "Abrir"; **`yt-dlp` passa a dependência de runtime**, tal como o `ffmpeg` |

---

## 4. Constrangimentos descobertos (decisões em aberto, não assumidos em silêncio)

1. **Tradução de legendas em tempo real.** O modo `translate` do Whisper só traduz
   **para inglês** — não existe tradução PT→outro idioma nativa no Whisper.
   - **v1:** tradução apenas para inglês (uma chamada Whisper, offline, sem modelo extra).
   - **M5a (local):** reutilizar o mesmo LLM local do resumo (ver ponto 2) para
     traduzir PT↔qualquer idioma via prompt — poupa uma dependência de inferência
     inteira. **Condição de aceitação:** correr 100 segmentos reais do Whisper
     (PT→EN/ES/FR) pelo LLM candidato e validar manualmente que não há alucinação
     nem enchimento de texto — modelos pequenos (0.5B–1B) falham exatamente em
     fragmentos curtos e sem contexto, que é a forma de uma legenda.
   - **M5b (cloud, decisão tomada — ver ponto 2):** os providers cloud de primeira
     parte (OpenAI, Anthropic, Gemini oficiais) não passam pelo gate — a qualidade já
     é validada pelo próprio provider. **Um `OpenAiCompatible` com `base_url`
     não-oficial (Ollama, self-hosted, OpenRouter) passa pelo mesmo gate do
     `LocalQwen`** — o `base_url` é escolhido pelo utilizador em runtime, por isso
     nenhum resultado de gate de um endpoint se transfere para outro; a regra é sobre
     quem controla o endpoint, não sobre o tamanho do modelo.
   - UI mostra um badge por operação — `🔒 local` vs `☁️ sai do PC` — para o
     utilizador escolher conscientemente antes de cada resumo/tradução que sai da
     máquina, o mesmo padrão já usado no tooltip disco/RAM-only do Whisper (§4.13).

2. **Resumo e tradução por LLM: 2 backends, local por omissão + cloud opt-in
   (decisão tomada).** Local continua a ser o caminho por omissão, consistente com a
   filosofia de privacidade do resto do plano — modelo pequeno em GGUF
   (`Qwen2.5-0.5B-Instruct-Q4_K_M` ~350 MB, ou `Llama-3.2-1B-Instruct-Q4_K_M`
   ~700 MB, via `llama-cpp-rs` ou `candle`). **Cloud é opt-in explícito**, com 4
   opções: OpenAI-compatible (cobre também "custom" — é o mesmo código, `base_url`
   configurável aponta para OpenAI oficial, Ollama, OpenRouter ou qualquer endpoint
   compatível; não são duas integrações), Anthropic e Gemini.
   - **Segredos nunca em `config.toml`** (plaintext, `chmod 644`, contradiria
     §4.13/§4.18): chaves de API no keyring do SO (crate `keyring`, Secret Service no
     Linux) com variáveis de ambiente como fallback (`VAD_OPENAI_KEY`,
     `VAD_ANTHROPIC_KEY`, `VAD_GEMINI_KEY`, `VAD_CUSTOM_KEY`). Sem Secret Service
     disponível (comum em setups mínimos/headless) — terceira via, mantida a uma
     linha para não crescer: pedir na UI e guardar só em memória de sessão.
   - Novo erro `VadError::LlmAuthFailed` → UI mostra "chave inválida — abre
     Definições → IA", sem retry infinito nem crash.
   - **Nota de RAM:** o modelo local soma-se aos ~55 MB do Whisper quantizado
     (ver §5); **modo cloud é ~0 MB extra** — é o melhor argumento de eficiência do
     projeto para quem está limitado em RAM, vale a pena o utilizador saber disto.
   - Dependências atrás de feature flags (quem só usa local não paga o custo
     binário): `reqwest` com `rustls` (evita a dor de linkar OpenSSL entre
     distros), `async-openai` para o caminho OpenAI-compatible, clientes finos
     próprios (POST simples) para Anthropic/Gemini em vez de SDKs pesados.
   - **Ordem de execução, não o inverso:** M5a (local, com gate) fica à frente de
     M5b (cloud). M5b sozinho — 4 providers, keyring, painel de definições com teste
     de ligação, chunking, retry/backoff, fallback offline, testes com HTTP mockado —
     é comparável em dimensão a M1–M3 juntos; é um milestone com peso próprio, não um
     refinamento do M5. Se quiseres cloud mais cedo, é uma troca consciente de
     prioridade, não a ordem por omissão.

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
    RAM do §5, não é ignorado. **Limite:** cap de duração em cache (ex. 4h); acima
    disso, truncar com aviso em vez de crescer sem limite — evita o caso patológico
    de uma gravação de 8h+ inflacionar a RAM sem controlo.

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

14. **Taxonomia de erros e degradação, num sítio só.** Em vez de tratamento disperso
    por módulo, um único `VadError` (via `thiserror`) em `vad-core/error.rs`, com uma
    tabela erro→ação→UI curta (ex.: `FfmpegNotFound` → desativar waveform/Whisper +
    mostrar o comando de instalação; `HwDecUnavailable` → já coberto por §4.3;
    `ModelDownloadFailed` → manter modelo local se existir, senão desativar a
    feature). **Nunca invocar `sudo` a partir da app** para instalar dependências —
    isso é escalar privilégios silenciosamente; mostrar o comando (`sudo apt install
    ffmpeg`) para o utilizador copiar e correr, nunca executá-lo por ele.

15. **Transcrição é offline (batch), não ao vivo — explícito para não haver
    ambiguidade.** O utilizador abre o ficheiro, pede transcrição, o `extractor.rs`
    corre e o Whisper processa o áudio já extraído. Transcrição em tempo real
    (enquanto o vídeo reproduz) fica fora do v1 — exigiria buffer circular e
    sincronização com o `time-pos` do mpv, sem benefício claro para o caso de uso
    (reuniões já gravadas, não streams ao vivo).

16. **Descarregamento automático de modelos inativos — só onde faz diferença.** Um
    temporizador de inatividade (ex. 5 min sem uso) liberta o Whisper/LLM da RAM.
    **Isto só importa para o caminho RAM-only e para o LLM** — um modelo carregado
    por `mmap` (modo disco, §4.13) já é gerido pelo kernel via page cache; construir
    um unload ativo para esse caminho não ganha nada e compete com o próprio SO.

17. **Extração de PCM: assíncrona, com progresso e cancelamento — falta no plano
    atual.** Correr o `ffmpeg` do `extractor.rs` numa tarefa em background (não
    bloquear a UI), reportar progresso (ex. "Extraindo áudio... 65%") e permitir
    cancelar se o utilizador fechar o ficheiro antes de terminar (mata o
    subprocesso, não o deixa órfão). Sem isto, abrir uma reunião de 1h congela a UI
    por 10-30s.

18. **Cache de PCM permanente em disco: decidido que NÃO entra em v1.** A cache em
    memória por sessão (§4.12) já serve waveform e Whisper; persistir PCM
    descodificado em `~/.cache/vad/pcm/` sobreviveria ao fecho da app — para
    gravações de reuniões, isso significa **áudio potencialmente confidencial
    retido indefinidamente e sem encriptação**, o oposto do motivo de o Whisper ter
    modo RAM-only. Aqui o "zero-disk" é estrutural, não incidental. Se algum dia
    for adicionado (para acelerar reabrir o mesmo ficheiro), tem de ser opt-in e
    comunicado, nunca automático.

19. **Chunking (map-reduce) para transcrições longas — falta independentemente de
    cloud.** Uma transcrição de 90 min de reunião não cabe num único prompt, nem no
    LLM local nem em muitos limites de cloud. `summarizer.rs` resume por blocos e
    funde os resumos parciais; cada provider tem a sua janela de contexto, o limite
    de tokens por chamada é por provider, não uma constante global. Isto já era uma
    lacuna do M5 antes de existir cloud — devia ter sido apanhado antes. **Nota de
    memória:** descartar cada bloco bruto de transcrição assim que o respetivo resumo
    parcial é produzido, em vez de reter tudo até ao fim do map-reduce — o pico de
    heap não deve crescer com o número de blocos já processados.

20. **Progresso do LLM: por bloco, não por token — mesma política do §4.17, não uma
    nova.** Um resumo cloud demora 10-60s; um map-reduce sobre 90 min de transcrição,
    mais. Em vez de streaming SSE (3 formatos diferentes por provider, para um caso
    de uso batch, não chat ao vivo), reportar progresso por chunk do map-reduce
    ("a resumir bloco 3 de 7") — dá feedback real e torna o cancelar significativo,
    sem exigir parsing de streaming específico por provider. Streaming token-a-token
    fica como melhoria posterior, não requisito do M5b.

21. **Fallback automático para local quando offline: só com aviso, nunca silencioso.**
    Se a chamada cloud falhar por falta de rede, cair para o LLM local é razoável —
    mas trocar de um modelo cloud para um modelo de 0.5B muda a qualidade do
    resultado sem o utilizador saber. O badge 🔒/☁️ (ponto 1) existe precisamente
    para isto: um resumo produzido pelo fallback tem de ficar marcado como local
    (🔒), não apresentado como se tivesse vindo do provider pedido. Trocar de
    caminho sem avisar é o mesmo erro que já rejeitámos na ronda da auditoria do
    Mistral — só que na direção inversa (cloud→local em vez de local→cloud).

22. **Resiliência de rede.** Timeouts, retry com backoff+jitter para 429/5xx,
    deteção de offline (aciona o ponto 21). Estende o `VadError` do §4.14:
    `LlmTimeout`, `LlmRateLimited`, `NoNetwork`, `LlmAuthFailed` (já no ponto 2).

23. **Endpoint OpenAI-compatible customizado: validar antes de guardar.** Um
    `base_url` errado sem validação vira bug irreprodutível ("o resumo não
    funciona" sem mais contexto). Botão "Testar ligação" no painel de definições
    (`settings_panel.rs`, novo em `vad-app/src/panels/`) antes de gravar a
    configuração.

24. **Segurança de FFI: panics não podem atravessar a fronteira C→Rust.** Não é toda
    fronteira FFI que precisa disto — chamar *para* C a partir de Rust não exige nada
    de especial. O que precisa de guarda é o **Rust que o C chama de volta**: o
    callback de `mpv_render_context_set_update_callback` (§4.3, Sprint_01) e os
    callbacks de progresso/abort do `whisper.rs` (§4.17, Sprint_06). O Rust moderno
    aborta o processo (não é UB silencioso) quando um panic escapa de um `extern "C"`
    — mas abortar o processo é exatamente o que o §5 já rejeitou ao decidir **não**
    usar `panic = "abort"`, precisamente porque contradiz o critério de M1 "erro
    tratado sem crash da app". Um panic sem guarda nesses dois callbacks mata o
    processo de qualquer forma, anulando essa decisão já tomada. **Decisão:** envolver
    esses callbacks especificamente em `std::panic::catch_unwind`, converter o panic
    apanhado num `VadError` e seguir a tabela erro→ação→UI do §4.14 — não é uma
    prática genérica de "toda fronteira FFI precisa disto", é a consequência direta de
    uma decisão já tomada no plano.

25. **Integrações de sistema operativo isoladas atrás de um trait comum, não
    espalhadas pela app (decisão de arquitetura, não de âmbito).** O v1 continua a
    ser **Linux em primeiro lugar** — não há trabalho de Windows/macOS/mobile
    planeado nem no roteiro (§9/§11). Mas MPRIS, inibidor de screensaver e bandeja de
    sistema (§4.7/§4.8, hoje via `zbus`/`ksni`, só existem no Linux) não devem ser
    chamados diretamente dos painéis da UI — ficam atrás de um trait `PlatformIntegration`
    definido em `vad-core` (ver `platform.rs` no §7), com a única implementação de v1
    marcada `#[cfg(target_os = "linux")]`. Isto não é trabalho extra por antecipação —
    é o mesmo desacoplamento já decidido em §1 ("Core desacoplado da UI... preparar
    para eventual porte futuro") aplicado de forma concreta: se um dia houver um port
    para outro SO, esse trabalho fica confinado a escrever um novo módulo
    `#[cfg(target_os = "...")]` atrás do mesmo trait, sem tocar em `vad-core`/`vad-ai`
    nem reescrever a app base.

26. **Segurança de subprocessos CLI (`ffmpeg`/`yt-dlp`): argv como vetor, nunca
    shell.** Um nome de ficheiro com espaços, aspas ou um prefixo como `-vcodec` pode
    ser interpretado como opção pelo `ffmpeg` se a invocação passar por uma shell.
    **Regra única, aplicada em todos os pontos que chamam estes binários**
    (`extractor.rs` Sprint_06, URL/yt-dlp Sprint_05, `clip_export.rs` Sprint_08):
    `Command::new("ffmpeg").args([...])` — nunca `sh -c`/interpolação de string — e
    terminar as opções com `--` antes de qualquer caminho de ficheiro arbitrário do
    utilizador.
27. **Validação de esquema de URL antes de encaminhar para o mpv/yt-dlp.** A caixa
    "Abrir" aceita URLs (§3); sem validação, um utilizador (ou um link colado de
    outro lado) podia passar `file:///etc/shadow` ou `smb://`. **Decisão:** validar
    contra uma lista permitida (`http://`, `https://`, `rtsp://`) antes de passar ao
    mpv, em `playlist.rs`/URL handling (Sprint_05).
28. **Validação do `base_url` customizado (OpenAI-compatible) contra SSRF.** Um
    endpoint customizado no `settings_panel.rs` (§4.2/§4.23) podia apontar para
    `169.254.169.254` (metadados de cloud) ou uma porta interna da própria máquina.
    **Decisão:** validar o URL com a crate `url::Url` (esquema http/https, sem
    reescrita para endereços de metadados conhecidos) antes de gravar a configuração
    — o mesmo botão "Testar ligação" do §4.23 é o sítio natural para isto
    (Sprint_11/12).
29. **HUD sobre o FBO do mpv não pode usar blur de fundo.** O `egui_glow` não tem
    `backdrop-filter` nativo sobre uma textura externa (o FBO onde o mpv renderiza);
    simular blur por múltiplas passagens de shader sobrecarregaria GPUs integradas
    (Intel Iris/Mesa) sem necessidade. **Decisão:** superfícies do HUD/modais em
    opacidade fixa ~92% (`Color32::from_rgba_premultiplied`) com borda de 1px, em vez
    de tentar reproduzir o blur dos mockups (`Dialogs.dc.html`) literalmente
    (Sprint_02).
30. **Escrita atómica de estado persistente.** `recentes.json` (§4.6) e
    `config.toml` (§5) podem corromper-se (ficar a 0 bytes) se o processo morrer a
    meio da escrita — um corte de energia ou `kill -9` não avisa. **Decisão:** escrever
    sempre primeiro para um ficheiro `.tmp` no mesmo diretório e aplicar
    `std::fs::rename` (atómico no mesmo filesystem) sobre o destino final
    (Sprint_05, onde ambos os ficheiros são implementados).
31. **Morte súbita do subprocesso `ffmpeg` (OOM-killer/sinal).** O `extractor.rs`
    já trata cancelamento pedido pelo utilizador (§4.17); falta o caso em que o
    processo morre sozinho (ex. OOM-killer numa reunião de 4h) — sem isto, a UI fica
    presa em "A extrair áudio... 0%" para sempre. **Decisão:** monitorizar o
    `ExitStatus` do processo filho; código não-zero ou terminação por sinal converte
    de imediato em `VadError::ExtractionFailed` (§4.14) e repõe a UI (Sprint_06).
32. **Introspecção defensiva de propriedades do mpv.** Distros com uma `libmpv` mais
    antiga podem não ter todas as propriedades que o `player.rs` lê (`hwdec-current`,
    etc.). **Decisão:** tratar `MPV_ERROR_PROPERTY_NOT_FOUND` como um caso normal
    (valor por omissão/indisponível), nunca como erro fatal — evita que o VAD falhe
    o arranque numa distro com mpv mais velho (Sprint_01, `player.rs`).
33. **FBO do render em píxeis físicos, não lógicos (fractional scaling/HiDPI).** Em
    Wayland com escala 125%/150%, um FBO criado com as dimensões lógicas da janela
    produz vídeo desfocado. **Decisão:** multiplicar sempre as dimensões lógicas pelo
    `pixels_per_point` real do `egui_glow` ao alocar o framebuffer do
    `mpv_render_context` (Sprint_01, `render.rs`).
34. **Acessibilidade de teclado nos diálogos e controlos.** Sem isto, utilizadores de
    teclado ficam presos num diálogo modal ou perdem a noção de onde está o foco.
    **Decisão:** todo diálogo modal fecha com `Escape`; todo controlo interativo tem
    anel de foco visível (`2px`, cor de acento) — parte da configuração de
    `egui::Visuals` em `theme.rs` (Sprint_15), aplicada a diálogos já existentes
    desde a Sprint_02.

### Fora de âmbito, por decisão deliberada

Para não serem reintroduzidas mais tarde sem motivo — features do VLC que ficam de
fora, com a razão:

- **Servidor de streaming (RTSP/HTTP broadcast)** — hoje o caminho normal é OBS/nginx-rtmp.
- **Sintonizador de TV analógica/DVB-T** — hardware praticamente extinto.
- **CD de áudio (`cdda://`)** — leitores de CD físico já não existem na generalidade das máquinas.
- **Efeitos de vídeo gimmick** (espelho, puzzle, ondulação) — sem utilidade real.
- **Transcodificador geral** — o corte/exportação via `ffmpeg` (§8) cobre o caso que interessa, sem o menu "Converter/Guardar" cheio de erros do VLC.
- **Windows/macOS/Android/iOS como alvo de desenvolvimento do v1** — o projeto é
  Linux em primeiro lugar. O ponto 25 acima garante que a arquitetura não fecha essa
  porta, mas nenhum destes SOs entra no roteiro §9/§11 sem uma decisão explícita
  futura. Para mobile especificamente, a razão é mais forte que "falta de tempo": o
  iOS proíbe `fork()`/`exec()` na sandbox da app, e o Android bloqueia (SELinux
  `W^X`) executar binários próprios a partir da pasta de dados da app desde a API 29
  — o modelo atual de subprocessos (`ffmpeg`, `yt-dlp`) é inviável em mobile por
  construção, não por imaturidade das crates.
- **Migração de `ffmpeg`/`whisper.cpp`/`af=arnndn` para crates Pure Rust
  (`symphonia`/`candle`/`nnnoiseless`) — avaliada e recusada para já.** A motivação
  citada para essa migração (eliminar dependências de subprocesso) já está resolvida
  no desktop Linux: o M6 empacota `ffmpeg`/`yt-dlp` no manifesto Flatpak (§9) e o M1
  já degrada graciosamente na ausência deles via `probe_dependencies`+`error.rs`
  (§4.14). Em troca, `symphonia` não cobre AC3/DTS/TrueHD (áudio comum em rips de
  filmes, ainda que não em gravações de reunião), e não há medição de paridade
  velocidade/qualidade/RAM do `candle` contra o `whisper.cpp` já validado (o número
  de ~55 MB do §5 vem do `whisper.cpp`). Reabrir esta decisão exige, no mínimo, essa
  medição — não é para ser reproposta só com o argumento de "é mais seguro por ser
  Rust".
- **Vetorização SIMD manual da conversão PCM `i16`→`f32`** — avaliada e recusada. A
  cache de PCM já fica em `i16` por decisão (§4.12); a conversão para `f32` que o
  Whisper precisa acontece **dentro do próprio `whisper.cpp`/ggml ao ingerir os
  dados**, não em código do VAD. Não há laço de conversão próprio para vetorizar —
  otimizar isto seria trabalho sem alvo real.
- **`shortcuts.toml` (remapeamento de atalhos de teclado) e `[mpv_options]`
  (passthrough de opções arbitrárias do mpv) no `config.toml`** — adiado para
  pós-M6/stretch, não v1. Ambos são aditivos (não bloqueiam nenhum milestone) e o
  segundo tem uma superfície a considerar com cuidado (opções arbitrárias do mpv
  correm no mesmo processo) — não é para ser adicionado apressadamente só por
  conveniência de utilizadores de i3/Hyprland.

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
- **Waveform pyramid**: só o nível global (visão da reunião inteira) é pré-computado
  ao abrir o ficheiro; os níveis de 5 min e 10s calculam-se **sob demanda** quando o
  utilizador faz zoom, não antecipadamente — evita atraso na abertura de ficheiros
  longos. Armazenamento é min/max por pixel (~KB), não o PCM bruto, por isso o custo
  aqui é de tempo de cálculo, não de RAM. UI renderiza no máximo ~1000 pontos visíveis,
  independente da duração do ficheiro, desenhados num único lote (`egui::Mesh`/
  `rect_filled` em sequência) em vez de uma chamada de desenho por barra — custo real
  de CPU a medir, não assumido.
- **Threads do `whisper.cpp`**: fixar por omissão em `num_cpus::get_physical()` em
  vez do total de threads lógicas — evita contenção de cache entre threads irmãs de
  Hyper-Threading/SMT. Valor por omissão a confirmar por medição em Sprint_06, não
  um ganho percentual assumido.
- **Libertar VRAM em reprodução só-áudio**: desativar o pipeline de textura OpenGL do
  `render.rs` quando o ficheiro aberto não tem faixa de vídeo — sem isto, uma
  reunião em `.opus` continua a reservar memória de GPU para um frame que nunca é
  desenhado.
- **Logging estruturado** via `tracing`, com `--verbose`/`--log-file` no CLI
  (`~/.cache/vad/vad.log`) — sem isto, um bug reportado por um utilizador é
  inreproduzível.
- **Configuração unificada** em `~/.config/vad/config.toml` (serde+toml): consolida o
  que já precisa de persistir — `recents.rs` (§7), escolha disco/RAM-only por modelo
  (§4.13), atalhos de teclado. Um único ficheiro, não vários formatos ad-hoc.

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
|  [⏪5s] [▶/⏸] [5s⏩]   00:14:23 / 01:30:00   1.5x ▾   🔊 100%   (playhead acima  | [🎨 Cores Vídeo]|
|  é clicável/arrastável — não há Modo Reunião sem controlo de transporte)         | --------------- |
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
│   │   │   ├── state.rs          # eventos do mpv (thread interna em C) -> crossbeam-channel/watch para a UI; reconciliação periódica (poll a cada ~5s) como rede de segurança contra eventos perdidos
│   │   │   ├── playlist.rs       # shuffle/repeat, URLs (yt-dlp) além de ficheiros locais
│   │   │   ├── recents.rs        # últimos ~20 ficheiros + timestamp p/ "continuar de onde parou" (ver §4.6)
│   │   │   ├── bookmarks.rs      # notas de reunião exportáveis (.md, timestamps em texto simples)
│   │   │   ├── error.rs          # VadError (thiserror) + tabela erro->ação->UI (ver §4.14)
│   │   │   ├── config.rs         # ~/.config/vad/config.toml, serde+toml (ver §5)
│   │   │   └── platform.rs       # trait PlatformIntegration (media session, inibir screensaver, tray) — só cfg(target_os="linux") implementado no v1 (ver §4.25)
│   │   └── Cargo.toml
│   │
│   ├── vad-ai/                   # transcrição, VAD, resumo, tradução — SEM deps de UI
│   │   ├── src/
│   │   │   ├── extractor.rs      # subprocesso ffmpeg -> PCM 16kHz mono i16, assíncrono c/ progresso e cancelamento (ver §4.17), cache por ficheiro com cap de duração (ver §4.12), SEM persistência em disco (ver §4.18)
│   │   │   ├── model_manager.rs  # escolha do utilizador: disco (~/.local/share/vad/models/) ou RAM-only (ver §4.13)
│   │   │   ├── whisper.rs        # transcriber (whisper.cpp bindings); caminho RAM-only em Arc<[u8]> pinado, nunca Vec<u8> (ver §4.11); caminho disco carrega por path
│   │   │   ├── vad_detector.rs   # deteção de silêncio antes do Whisper
│   │   │   ├── llm_provider.rs   # trait Summarizer + enum LocalQwen/OpenAiCompatible{base_url}/Anthropic/Gemini (ver §4.2)
│   │   │   ├── summarizer.rs     # orquestra chunking/map-reduce (ver §4.19) sobre o llm_provider.rs escolhido
│   │   │   └── translator.rs     # Whisper translate (EN) + M5a/M5b: mesmo llm_provider.rs p/ outros idiomas (ver §4.1)
│   │   └── Cargo.toml           # nota: redução de ruído é af=arnndn no mpv, não crate própria; deps de cloud atrás de feature flags (ver §4.2)
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
│       │   ├── render.rs         # egui_glow::CallbackFn que invoca a mpv_render_context na thread de UI; callback de update envolvido em catch_unwind (ver §4.24)
│       │   ├── mpris.rs          # implementa PlatformIntegration p/ Linux: org.mpris.MediaPlayer2[.Player] via zbus — Metadata: trackid/title/artist/length (ver §4.25)
│       │   ├── screensaver.rs    # implementa PlatformIntegration p/ Linux: org.freedesktop.ScreenSaver.Inhibit via zbus (ver §4.7/§4.25)
│       │   ├── tray.rs           # implementa PlatformIntegration p/ Linux: StatusNotifierItem via ksni — best-effort, ver §4.8/§4.25
│       │   ├── theme.rs
│       │   └── panels/
│       │       ├── hud.rs
│       │       ├── whisper_panel.rs
│       │       ├── playlist_panel.rs
│       │       ├── audio_panel.rs
│       │       ├── video_panel.rs    # cor, aspect/crop/rotação, delay A/V — já estava no mockup, faltava no código
│       │       └── settings_panel.rs # providers de IA, chaves (nunca lidas/escritas em config.toml, ver §4.2), botão "Testar ligação" (§4.23)
│       └── Cargo.toml
```

`vad-core` e `vad-ai` não dependem de `egui` — é o que permite, no futuro, trocar a UI
ou portar para outro SO sem tocar na lógica. Nota: a renderização do vídeo (`render.rs`)
fica presa à thread de UI por exigência da `mpv_render_context` (contexto OpenGL
current); só os eventos de estado do mpv correm em canal — não é a mesma decisão.

O mesmo princípio aplica-se às integrações de sistema operativo: `mpris.rs`,
`screensaver.rs` e `tray.rs` implementam o trait `PlatformIntegration` de
`vad-core/src/platform.rs` em vez de serem chamados diretamente pelos painéis — no v1
só existe a implementação `#[cfg(target_os = "linux")]`, mas um port futuro (§4.25)
troca só esse módulo, nunca a lógica de `vad-core`/`vad-ai` por cima.

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
**Resolução:** checkbox "Corte exato (recodificar)" no painel de exportação — desligado
por omissão (rápido, `-c copy`, keyframe); ligado usa `-c:v libx264 -crf 18 -c:a aac`
(mais lento, preciso ao frame, ficheiro pode crescer). O utilizador escolhe
conscientemente, em vez de o v1 impor silenciosamente o corte por keyframe.

---

## 9. Fases e Milestones

| Fase | Entregável | Critério de aceitação |
| :--- | :--- | :--- |
| **M0** | Medir baseline real do VLC nesta máquina (RSS, arranque, CPU em pausa) | Números registados, substituem os "a medir" da tabela |
| **⭐ M0.5** | Protótipo mínimo: só `vad-core` embutindo libmpv via `mpv_render_context`/OpenGL com `hwdec=auto-safe`, sem HUD nem features. **Gate — não avançar para M1 sem isto validado.** | Play/pause/seek/volume funcionam em X11 **e** Wayland sem flicker (testar com Intel e, se possível, NVIDIA); HUD mínimo mostra `hwdec-current` real, incluindo fallback `SW (CPU)` quando forçado |
| **M1** | HUD completo, MPRIS + inibidor de screensaver (mesma base `zbus`, ver §4.7), CLI (clap) + drag-and-drop, `probe_dependencies` (ffmpeg/yt-dlp, ver §4.14 — nunca sudo automático), guarda de foco de teclado (`wants_keyboard_input`), `VadError`/`error.rs` (§4.14); decisão de janela nova por execução (sem instância única) | Teclas de media do sistema funcionam; ecrã não suspende durante playback; app arranca e degrada graciosamente (botões desativados, comando de instalação mostrado) sem `ffmpeg`/`yt-dlp`; `vad ficheiro.mkv` e arrastar ficheiro abrem reprodução |
| **M2** | Playlist (+ shuffle/repeat), faixas de áudio/legendas, hwdec visível no HUD, `video_panel.rs` (delay A/V, aspect/crop/rotação), reprodução por URL (yt-dlp — confirmar isolamento de `config-dir`, ver §3), resume playback (`recents.rs`), `config.rs` unificado (§5) | Troca de faixa sem reiniciar; indicador de aceleração correto; URL do YouTube reproduz sem herdar `~/.config/mpv` do utilizador; reabrir a app oferece continuar o último ficheiro |
| **M3** | Whisper offline (§4.15, via `extractor.rs` assíncrono com progresso/cancelamento, §4.17) + `model_manager.rs` com escolha disco/RAM-only e tooltips (§4.13) + unload por inatividade (§4.16) + VAD skip-silence + bookmarks exportáveis em .md | Transcrição de um ficheiro de reunião real sem congelar a UI durante a extração nem interromper outra reprodução; utilizador escolhe e vê o tradeoff antes de descarregar um modelo; notas exportadas |
| **M4** | Corte/exportação de clips (via ffmpeg CLI + checkbox "corte exato", ver §8), redução de ruído (af=arnndn) | Selecionar troço na waveform, exportar ficheiro válido; ambos os modos de corte (keyframe e exato) funcionam |
| **M5a** | Resumo + tradução via `llm_provider.rs` (`LocalQwen`), chunking/map-reduce (§4.19), progresso por bloco (§4.20) | **Gate de aceitação:** 100 segmentos reais (PT→EN/ES/FR) revistos manualmente sem alucinação/enchimento; transcrição de 90 min resumida sem exceder a janela de contexto |
| **M5b** | Cloud opt-in: `OpenAiCompatible`/Anthropic/Gemini, `settings_panel.rs` com teste de ligação (§4.23), keyring + fallback env vars (§4.2), fallback offline com disclosure (§4.21), resiliência de rede (§4.22) — **milestone com peso próprio, não refinamento do M5a** | Badge 🔒/☁️ visível antes de cada operação cloud; `base_url` inválido detetado no "Testar ligação", nunca só ao usar; nenhuma chave em `config.toml` nem em `vad.log` |
| **M6** | Polish (tema, animações), bandeja de sistema e PIP/always-on-top (`tray.rs`, best-effort — ver §4.8/4.9), perfil de release, empacotamento (Flatpak com ffmpeg/yt-dlp incluídos no manifesto, removendo a dependência de runtime do sistema na versão empacotada) | Binário instalável, arranque e RAM medidos e comparados ao M0 |
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
5. Teste de concorrência no `player.rs`: disparar comandos (play/pause/seek) a partir
   do handler MPRIS e da UI ao mesmo tempo, em loop; validar que não há deadlock nem
   pânico no `Mutex` do contexto mpv. Sem alvo numérico — o critério é "não trava,
   não crasha", não um tempo específico (targets de performance vêm sempre do
   baseline medido em M0, nunca de números assumidos).
6. `llm_provider.rs` com HTTP mockado (ex. `wiremock`): respostas 401 (auth), 429
   (rate limit) e timeout, validar que cada uma mapeia para o `VadError` certo
   (§4.22) e aciona o fallback com disclosure (§4.21), não um crash nem um retry
   infinito.
7. **Teste anti-leak (específico, não "não deve haver segredos" vago):** depois de
   uma sessão que exercite uma chamada autenticada real, fazer grep dos padrões de
   chave API tanto em `~/.config/vad/config.toml` como em `~/.cache/vad/vad.log` —
   zero ocorrências em ambos.

### Verificação manual
1. Comparar RSS e CPU em pausa contra o baseline medido em M0.
2. Reproduzir um DVD/Blu-ray de teste e confirmar navegação de menu.
3. Transcrever um áudio de reunião com silêncios e validar tempo de transcrição vs
   duração real.
4. Testar teclas de media do teclado/sistema (MPRIS) com a app em segundo plano.
5. No painel de modelos, confirmar que os tooltips de "Guardar no disco" e "RAM-only"
   aparecem ao passar o rato e que a escolha é respeitada (ficheiro criado vs nenhum).

---

## 11. Divisão em Sprints

Isto **não** introduz novo âmbito — é o §9 (Fases e Milestones) reordenado em unidades
de trabalho mais pequenas, respeitando as mesmas dependências. Cada sprint fecha com um
critério verificável; onde o critério é o do milestone completo (§9), a tabela remete
para lá em vez de o repetir. Dois milestones (M1, M2, M3, M5b) foram divididos em mais
do que uma sprint por serem grandes demais para uma unidade só — os pontos de corte
seguem sub-blocos de trabalho já distintos no §7 (ficheiros diferentes) ou no §4 (temas
diferentes), nunca um corte arbitrário a meio de uma peça.

**Duração de cada sprint não é fixada aqui.** Sem velocidade medida nem horas/semana
definidas pelo utilizador, pôr um número (ex. "2 semanas") e somar um total de meses
seria o mesmo tipo de número inventado já rejeitado no resto do plano (§2, "a medir").
Cada linha é uma unidade de âmbito com critério de saída — a cadência é para o
utilizador decidir e medir à medida que a primeira sprint acontece.

| Sprint | Milestone | Foco (ficheiros/temas) | Critério de saída específico desta sprint |
| :--- | :--- | :--- | :--- |
| **0** | M0 | Medir baseline real do VLC. **Nota:** o VLC instalado é o snap e já falhou o driver gráfico (§1) — trocar para o pacote `.deb`/apt antes de medir, ou a medição fica contaminada pelo mesmo bug. | Critério do §9 (M0) |
| **1** | M0.5 (gate) | `player.rs` mínimo: `mpv_render_context`/OpenGL, `hwdec=auto-safe`. | Critério do §9 (M0.5) — gate, não passar à sprint 2 sem isto validado |
| **2** | M1 (parte 1) | `hud.rs` (transporte/scrubber/velocidade/volume/faixas), `main.rs` (clap, `probe_dependencies`), drag-and-drop, `error.rs`/`VadError` (§4.14) | `probe_dependencies` sem `ffmpeg`/`yt-dlp` degrada via a tabela erro→ação→UI do `error.rs` — os dois ficheiros têm de existir juntos, o critério do §9 de "degrada graciosamente" depende do `error.rs`, não só do probe |
| **3** | M1 (parte 2) | `mpris.rs` (zbus), `screensaver.rs` (zbus, §4.7), guarda de foco de teclado (`wants_keyboard_input`) | Critério do §9 (M1) — fecha o milestone |
| **4** | M2 (parte 1) | `playlist.rs` (shuffle/repeat, ficheiros+URLs), seletor de faixas áudio/legendas, `video_panel.rs` (aspect/crop/rotação/delay A-V), `audio_panel.rs`/equalizador (10 bandas + presets, EQ vem de graça do lavfi por §3, só falta a UI) | Troca de faixa sem reiniciar; EQ aplica-se à reprodução em tempo real |
| **5** | M2 (parte 2) | Reprodução por URL (yt-dlp, isolamento `config-dir` + `--no-config`, §3), `recents.rs` (resume), `config.rs` unificado (§5) | Critério do §9 (M2) — fecha o milestone |
| **6** | M3 (parte 1) | `extractor.rs` assíncrono (progresso/cancelamento, §4.17), `waveform_pyramid.rs` (§5 — pertence aqui, não a M4: partilha a cache de PCM do `extractor.rs` por §4.12, e o mockup de reunião do §6 já assume a waveform pronta), `model_manager.rs` + `whisper.rs` (`Arc<[u8]>` pinado, §4.11) | Transcrição de um ficheiro real sem congelar a UI; waveform da reunião inteira visível ao abrir o ficheiro |
| **7** | M3 (parte 2) | `vad_detector.rs` (skip-silence), unload por inatividade (§4.16), `bookmarks.rs` exportável em `.md` | Critério do §9 (M3) — fecha o milestone |
| **8** | M4 | `clip_export.rs` (ffmpeg CLI, corte por keyframe + checkbox "corte exato", §8), toggle `af=arnndn` | Critério do §9 (M4) — a waveform já existe desde a sprint 6, por isso a seleção de troço não é trabalho novo aqui |
| **9** | M5a (parte 1) | `llm_provider.rs` (`LocalQwen`), `summarizer.rs` (chunking/map-reduce, §4.19), `translator.rs`, progresso por bloco (§4.20) | Resumo de transcrição de 90 min sem exceder janela de contexto |
| **10** | M5a (parte 2 — gate) | Correr e rever manualmente 100 segmentos reais (PT→EN/ES/FR) | Critério de aceitação do §9 (M5a) — gate, não avançar para M5b sem isto passar |
| **11** | M5b (parte 1) | Cliente `OpenAiCompatible` (cobre OpenAI oficial e custom/self-hosted), keyring + fallback env vars (§4.2), scaffold do `settings_panel.rs` | Cliente funcional com um provider real testado manualmente |
| **12** | M5b (parte 2) | Clientes `Anthropic`/`Gemini`, botão "Testar ligação" (§4.23) para os 3 backends | `base_url`/chave inválidos detetados no teste de ligação, nunca só ao usar |
| **13** | M5b (parte 3) | Fallback offline com badge 🔒/☁️ (§4.21), retry/backoff 429/5xx, `VadError` estendido (`LlmTimeout`/`LlmRateLimited`/`NoNetwork`, §4.22) | Badge visível antes de cada operação cloud; fallback nunca silencioso |
| **14** | M5b (parte 4) | Testes com HTTP mockado (§10.6), teste anti-leak (§10.7) | Critério do §9 (M5b) — fecha o milestone |
| **15** | M6 | Tema/animações, `tray.rs` (ksni, best-effort, §4.8), PIP/always-on-top (§4.9), perfil de release (§5), Flatpak com `ffmpeg`/`yt-dlp` embutidos | Critério do §9 (M6) |
| **16** (opcional) | Pós-M6 | Esquema de URI `vad://` + instância única (§4.4/4.5), OpenSubtitles (§4.10) | Stretch goals, sem data comprometida |

**Nota sobre M5b ocupar 4 sprints contra as 6 de M1–M3 juntos:** o §4.2 descreve M5b
como "comparável em dimensão" a M1–M3 juntos. Contagem de sprints não é a mesma coisa
que dimensão de esforço — M5b tem mais peças em paralelo (4 backends, keyring, UI de
definições, resiliência, testes) mas cada uma é mais rasa que uma integração como o
`mpv_render_context` (sprint 1) ou o `whisper.rs` (sprint 6). Se a sprint 9 ou a 12
"transbordarem" na prática, é sinal de que a comparação do §4.2 estava certa e a
divisão aqui deve ser ajustada — não o contrário.
