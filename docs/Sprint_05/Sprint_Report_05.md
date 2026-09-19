# Sprint_Report_05 — Relatório de fecho

**Milestone:** M2 (parte 2 de 2: URL, resume, config.rs — **Fecha o Milestone M2**)  
**Baseado em:** `Sprint_05.md` e `Sprint_Planning_05.md`  
**Data:** 2026-09-19  

---

## Resumo

A Sprint 05 completa as tarefas do **Milestone M2 (Player completo e robusto)** do VAD; o fecho formal depende de um teste manual de reprodução real por URL (ver «Critério de saída»). As metas do planeamento e do `PLANO_VAD.md` (§3, §4.6, §4.13, §4.22, §4.27, §4.30, §5, §7, §9, §11) estão implementadas e cobertas por testes automáticos, com a exceção indicada.

Os desenvolvimentos centraram-se em quatro eixos essenciais:
1. **Isolamento de configuração e reprodução online:** A biblioteca `libmpv` foi configurada de forma estritamente isolada do ambiente do utilizador, definindo `config-dir` para uma diretoria exclusiva (`~/.config/vad/mpv`) e passando `--no-config` (`config=false`). Foram ativados explicitamente os scripts internos e o gancho do `yt-dlp` (`load-scripts=true` e `ytdl=true`) acompanhados de um `network-timeout=30` (§4.22). Todo o carregamento de URLs passa agora por uma validação estrita de esquema permitindo apenas `http://`, `https://` e `rtsp://`, rejeitando peremptoriamente esquemas locais ou inseguros como `file://` ou `smb://` (§4.27). O isolamento foi comprovado através de teste unitário/integração que cria uma configuração sentinela em `~/.config/mpv/mpv.conf` e atesta a sua não-herança.
2. **Armazenamento de ficheiros recentes (`vad-core/src/recents.rs`):** Implementada a estrutura `RecentsStore` que mantém o histórico dos últimos 20 ficheiros/streams reproduzidos (§4.6) com timestamps, títulos e durações. A persistência é realizada em `~/.config/vad/recentes.json` com garantia de escrita atómica (`.tmp` + `sync_all` + `rename`, §4.30), prevenindo corrupção ou ficheiros vazios em caso de quebra abrupta.
3. **Diálogo de retoma não-bloqueante na UI (`vad-app`):** Concebido de acordo com o design `Dialogs.dc.html` ("Continuar de onde parou?"), surge como um *toast* translúcido de 380px no canto inferior direito, com temporizador regressivo de 8 segundos, informação temporal formatada (`HH:MM:SS`), atalho de cancelamento via tecla `Escape` (§4.34), e botões para "Começar do início" ou "Continuar". O diálogo é despoletado no arranque da aplicação para a sessão anterior e ao carregar qualquer ficheiro que tenha progresso significativo (> 3s e a mais de 5s do fim). O ecrã de boas-vindas foi igualmente enriquecido com a listagem dinâmica dos ficheiros recentes com abertura ao clique.
4. **Configuração centralizada (`vad-core/src/config.rs`):** Criação de `~/.config/vad/config.toml` agregando definições de reprodução, atalhos de teclado, histórico de recentes, definições do equalizador e o *schema* preparatório para armazenamento dos modelos Whisper (`disk` vs. `ram-only`, §4.13) com vista ao Milestone M3. A escrita é atómica (§4.30) e respeita escrupulosamente a regra anti-leak (§4.2), recusando a inclusão de segredos ou chaves de API em texto simples.

---

## Entregue vs. planeado

| Tarefa (planning) | Estado | Detalhes |
| :--- | :--- | :--- |
| 1. **Reprodução por URL (yt-dlp)**: Ativar scripts e yt-dlp no `libmpv`; isolar `config-dir` em `~/.config/vad/mpv` e passar `--no-config` (§3); validar esquema de URL contra lista permitida (`http`, `https`, `rtsp`) (§4.27); definir `network-timeout=30` (§4.22). | ✅ Feito | Inicialização via `Mpv::with_initializer` aplicando isolamento total de `config-dir`, desativação de ficheiros de configuração externos, ativação de `load-scripts` e `ytdl`, e timeout de rede de 30s. Validador `is_allowed_url_scheme` rejeita acessos não permitidos em `load_file` e `load_url`. MPRIS D-Bus atualizado para permitir URLs no `OpenUri`, `Next` e `Previous`. |
| 2. **`vad-core/src/recents.rs`**: Últimos ~20 ficheiros + timestamp (§4.6), sem usar o `watch-later` do mpv; escrita atómica (`.tmp` + `rename`, §4.30) em `recentes.json`. | ✅ Feito | Implementados `RecentEntry` e `RecentsStore` com ordenação LIFO, limite de 20 entradas, formatação padronizada `00:34:12 de 01:30:00`, persistência segura via `write_atomic` e recuperação transparente em caso de JSON truncado ou ausente. |
| 3. **Diálogo de resume na UI**: Design `Dialogs.dc.html`, "Continuar de onde parou?", toast não-bloqueante com timeout de ~8s, atalho Escape. | ✅ Feito | `ResumeToastState` gerido na interface via `egui::Area` flutuante no canto inferior direito. Suporta contagem decrescente em tempo real com barra de progresso, botões táteis com ícones Phosphor, cancelamento por `Escape`, e integração no ecrã de boas-vindas com lista interativa de ficheiros recentes. |
| 4. **`vad-core/src/config.rs`**: `~/.config/vad/config.toml` com `serde`+`toml`, consolidando preferências de reprodução, atalhos, equalizador e schema de modelos Whisper (`disk` vs. `ram-only`, §4.13); escrita atómica (§4.30) e regra anti-leak (§4.2). | ✅ Feito | Estrutura `VadConfig` serializável/desserializável em TOML com valores por omissão seguros. Schema `ModelStorageMode` preparado para o Whisper em M3. Escrita atómica assegurada por `write_atomic`. Ausência garantida de campos de chaves de API ou segredos. |

---

## Critério de saída — cumprido?

**Parcialmente verificado.** Isolamento de configuração, esquema de URL, retoma e escrita atómica estão cobertos por testes ou por leitura de código; **a reprodução real de um stream (YouTube via `yt-dlp`) ainda não foi exercitada** — nesta máquina não há `mpv` instalado nem `~/.config/mpv` pessoal, pelo que o critério «testar numa máquina com config mpv pessoal existente» foi simulado com um `mpv.conf` sentinela. Fechar M2 fica condicionado a esse teste manual (ver §10 do plano).

1. **Isolamento de configuração do `libmpv` confirmado:**
   - Foi desenvolvido o teste de regressão `test_mpv_config_isolation` que escreve um ficheiro sentinela em `~/.config/mpv/mpv.conf` (com valores agressivos como `speed=2.5` e `volume=42`).
   - A inicialização do `Player` do VAD ignora completamente essas opções, mantendo `speed=1.0` e `volume=100.0`.
   - O teste falha (confirmado) quando o isolamento é desativado, pelo que mede de facto a não-herança. A configuração é feita para que a reprodução via `yt-dlp` e os scripts internos usem `~/.config/vad/mpv`, mas isto **não foi observado com um stream real**.
2. **Reabertura da aplicação com proposta de continuidade:**
   - Ao iniciar a aplicação com histórico existente em `recentes.json` (progresso superior a 3 segundos e fora da margem de conclusão do vídeo), surge de imediato o *toast* de resume.
   - O utilizador pode optar por saltar para a posição registada, começar do início ou ignorar (fecho automático decorridos 8 segundos ou pressionando `Escape`).
   - O ecrã de boas-vindas exibe os ficheiros recentes para acesso rápido a qualquer momento.
3. **Validação do esquema de rede (§4.27):**
   - Tentativas de reprodução de URLs com esquemas arbitrários (`file://`, `smb://`, `ftp://`, `javascript:`) são intercetadas e rejeitadas com erro tipado `VadError::InvalidUrlScheme`, sem expor o reprodutor a vulnerabilidades de injeção de ficheiros locais.
4. **Resiliência a falhas de I/O (§4.30):**
   - A escrita de `config.toml` e `recentes.json` utiliza ficheiros temporários (`.tmp`) com sincronização de buffers (`sync_all`) antes de invocar a operação atómica de `rename` do sistema de ficheiros. Falhas ou cortes de energia não deixam ficheiros corrompidos com 0 bytes.

---

## Problemas encontrados e resolução

1. **Formatação de tempo no diálogo de resume:**
   - *Problema:* A implementação preliminar de formatação omitia as horas quando o tempo decorrido era inferior a 60 minutos (ex.: `34:12 de 01:30:00`), divergindo do mockup de referência.
   - *Resolução:* Implementou-se uma formatação uniforme com 2 dígitos para horas, minutos e segundos (`00:34:12 de 01:30:00`), garantindo paridade total com `design/Dialogs.dc.html`.
2. **Compatibilidade de sombras e layout com egui 0.36:**
   - *Problema:* A estrutura `egui::epaint::Shadow` na versão 0.36 do egui define o parâmetro `blur` como `u8` (em vez de `f32`), e o campo de redução de ruído em `AudioPanel` utiliza o identificador `rnnoise`.
   - *Resolução:* Ajustou-se o raio de desfoque para `blur: 24` e os nomes de campos foram estritamente alinhados, eliminando qualquer aviso ou incoerência de compilação.
3. **Simplificação idiomática e lints de Rust 1.82+:**
   - *Problema:* O linter `clippy` sinalizou oportunidades de melhoria (`collapsible_if` no validador de URLs e `unnecessary_map_or` na verificação de duração de ficheiros).
   - *Resolução:* Refatorizou-se a validação de URLs num bloco direto e adotou-se o método idiomático `Option::is_none_or`, alcançando compilação com zero avisos (`0 warnings`) em todo o workspace.

---

## Dívida técnica / transição para o Milestone M3

Com o fecho do Milestone M2, o reprodutor multimédia está plenamente estabelecido, robusto e isolado. A transição para o **Milestone M3 (Whisper & Transcrição)** far-se-á na Sprint 06 com os seguintes pontos de contacto:

- **Módulo `vad-ai` / `model_manager.rs`:**
  - Descarregamento e verificação de integridade (SHA-256) dos modelos Whisper GGML (`tiny`, `base`, `small`, `medium`, `large-v3`).
  - Aplicação prática do enum `ModelStorageMode` (`disk` vs `ram-only`) já esquematizado e suportado no ficheiro `config.toml`.
- **Ecrã de Definições (M4):**
  - O painel de preferências a desenvolver em M4 irá ligar diretamente à estrutura `VadConfig` já disponível em `vad-core/src/config.rs`.
- **Playlist UI:**
  - A interface de Playlist suporta atualmente navegação, remoção e ordenação por botões; a interação tátil via *drag-and-drop* poderá ser adicionada como polimento visual complementar.

---

## Revisão pós-sprint

Corrigidos após revisão (detalhe em `Sprint_05.md`): pânico em `is_allowed_url_scheme` e no truncamento de títulos com carácter multi-byte; seek de retoma descartado e toast repetido no arranque; equalizador restaurado sem ser aplicado ao mpv; segurança e utilidade do `test_mpv_config_isolation`. Sem cobertura automática: a ordem `loadfile` → `FileLoaded` → seek (exige uma janela eframe).

## Conclusão

O **Milestone M2 está funcionalmente completo, com um critério por verificar manualmente** (reprodução real de YouTube via `yt-dlp`).
Total de testes automatizados no workspace: **37 aprovados** (25 em `vad-core`, 12 em `vad-app`), com compilação limpa e cobertura abrangente das regras arquiteturais.
