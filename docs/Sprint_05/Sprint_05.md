# Sprint_05 — Diário de bordo

**Milestone:** M2 (parte 2) — Reprodução por URL, resume, config.rs
**Planning:** ver `Sprint_Planning_05.md`
**Início:** 2026-09-19
**Fim:** 2026-09-19

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-19

- Início da implementação da Sprint 05 (Milestone M2, parte 2).
- Plano aprovado: isolamento de libmpv contra `~/.config/mpv`, suporte a URLs via `yt-dlp` com validação de esquema (§4.27) e timeout de rede (§4.22), `recents.rs` com limite de 20 ficheiros (§4.6), diálogo toast não-bloqueante de resume (~8s, `design/Dialogs.dc.html`), e configuração unificada `config.rs` em `~/.config/vad/config.toml` (§5) com escrita atómica (`.tmp` + `rename`, §4.30).
- Adicionadas dependências `serde` (derive), `serde_json` e `toml` em `crates/vad-core/Cargo.toml`.
- Criado `crates/vad-core/src/util.rs`:
  - `write_atomic`: escrita atómica via ficheiro temporário (`.tmp`) no mesmo diretório pai, `sync_all()`, e `std::fs::rename` (§4.30).
  - Resolução segura de caminhos: `vad_config_dir()`, `vad_mpv_config_dir()`, `vad_recentes_path()`, `vad_config_path()`.
  - `is_allowed_url_scheme`: validação contra lista permitida (`http://`, `https://`, `rtsp://`) rejeitando esquemas perigosos como `file://` e `smb://` (§4.27).
- Criado `crates/vad-core/src/recents.rs`:
  - `RecentEntry` e `RecentsStore` limitados a 20 ficheiros com timestamp (§4.6).
  - Persistência em `recentes.json` com `write_atomic` e fallback resiliente a JSON corrompido sem falha de arranque.
  - Formatação padronizada `00:34:12 de 01:30:00` coincidente com `Dialogs.dc.html`.
- Criado `crates/vad-core/src/config.rs`:
  - `VadConfig` estruturado com `player`, `recents`, `shortcuts`, `whisper` e `equalizer`.
  - Schema de modelos Whisper com `ModelStorageMode` (`disk` vs. `ram-only`) pronto para M3 (§4.13).
  - Regra anti-leak respeitada (§4.2): nenhum segredo/chave de API é suportado no ficheiro TOML.
  - Persistência em `config.toml` com `write_atomic` e fallback resiliente a sintaxe inválida.
- Atualizado `crates/vad-core/src/player.rs`:
  - Inicialização com `Mpv::with_initializer` configurando `config-dir` isolado em `~/.config/vad/mpv`, `config=false` (`--no-config`), `load-scripts=true`, `ytdl=true`, e `network-timeout=30` (§3, §4.22).
  - Validação estrita de esquemas em `load_file` e novo método `load_url` (§4.27).
- Adicionado teste de isolamento `test_mpv_config_isolation` criando configuração sentinela em `~/.config/mpv/mpv.conf` e confirmando que o VAD a ignora integralmente (critério de saída de M2). Teste passou com 24/24 testes aprovados em `vad-core`.
- Atualizado `crates/vad-app/src/mpris.rs`:
  - Removida a limitação de reprodução de URLs nos métodos `next()` e `previous()`, permitindo reprodução uniforme de URLs via MPRIS D-Bus.
- Atualizado `crates/vad-app/src/app.rs`:
  - Integradas as estruturas `RecentsStore` e `VadConfig` carregadas no arranque da aplicação.
  - Implementado o diálogo toast de retoma (`ResumeToastState` e `render_resume_toast`) desenhado fielmente segundo `design/Dialogs.dc.html` com timeout de 8s, estilo não-bloqueante via `egui::Area` (canto inferior direito), contador de tempo, botões "Começar do início" e "Continuar", e fecho por tecla Escape (§4.34).
  - Oferta de retoma ativa tanto no arranque da aplicação para a última sessão quanto ao abrir ficheiros que possuam progresso prévio registado (>3s).
  - Atualizado ecrã inicial (`render_welcome_screen`) com visualização dinâmica dos últimos ficheiros recentes e respetivo resumo de tempo para abertura imediata.
  - Ativado suporte a itens de URL no `EndOfFile` e `PlaylistAction::PlayItem`, retirando a mensagem temporária de adiamento.
  - Implementadas as rotinas de persistência periódica (`save_progress_periodically` a cada 5s) e de saída segura (`save_state` em `on_exit` e `Drop`).
  - Adicionados testes de integração em `vad-app` para timeout do toast, regras de threshold de retoma e oferta de resume no arranque.
- Bateria de testes do workspace completa: 37/37 testes aprovados.

## Desvios face ao Sprint_Planning_05.md

Nenhum. Todas as 4 tarefas planeadas foram integralmente implementadas de acordo com as especificações técnicas de `PLANO_VAD.md`.

## Problemas encontrados

1. **Formatação de tempo com zero à esquerda:**
   - *Problema:* A implementação inicial de `format_seconds` devolvia `MM:SS` quando a hora era zero (ex.: `34:12 de 01:30:00`).
   - *Resolução:* Padronizou-se a formatação para `HH:MM:SS` fixos com dois dígitos em cada campo (`00:34:12 de 01:30:00`), garantindo paridade com a especificação visual de `Dialogs.dc.html`.
2. **Nome de campo em `AudioPanel`:**
   - *Problema:* O campo booleano de redução de ruído chamava-se `rnnoise`, enquanto a primeira passagem usou `rnnoise_enabled`.
   - *Resolução:* Ajustado para o nome real do campo (`rnnoise`).
3. **Tipo de raio de desfoque em `Shadow` no egui 0.36:**
   - *Problema:* A sombra do toast requeria `blur: u8` e não `f32`.
   - *Resolução:* Alterado de `24.0` para `24`.
