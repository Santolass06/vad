# Sprint_17 — Diário de bordo

**Milestone:** M7 — Conformidade legal (licenças, privacidade, distribuição)
**Planning:** ver `Sprint_Planning_17.md`
**Início:** 2026-11-03
**Fim:** 2026-11-03 (evidência reunida; pendências 🧑‍⚖️ por resolver)

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-11-03

> Este diário descreve **evidência reunida**. Não afirma conformidade legal.

- **Sprint_16 (opcional, sem data) saltada** por decisão minha: stretch goals (`vad://`, instância única, OpenSubtitles). Por isso o OpenSubtitles não entra no inventário.
- **Re-medição das dependências** (`cargo metadata --filter-platform x86_64-unknown-linux-gnu`): **455** crates de terceiros (eram 410), **30** expressões de licença distintas (iguais), 0 sem campo `license`. Novidades desde a medição inicial: `keyring` (+ `libdbus-sys`), `ksni`, `mimalloc`, `url`, e `LGPL-2.1` continua só em `libmpv2`/`libmpv2-sys`.
- **Tarefa 1 — licença nos `Cargo.toml`:** campo SPDX `license` **deliberadamente não definido**: escolher `GPL-3.0-only` vs `-or-later` é a decisão 🧑‍⚖️ do utilizador. Usei `license-file = "../../LICENSE"` + `publish = false`. Cabeçalhos SPDX nos `.rs`: não adotados (dependem da mesma decisão).
- **Tarefa 2 — `cargo-deny` 0.20.2** instalado (ferramenta de dev). `deny.toml` com allowlist explícita e decisões escritas (`self_cell`: Apache-2.0 escolhida, GPL-2.0-only deliberadamente fora da allowlist; `Unicode-3.0`, `CDLA-Permissive-2.0`, `OFL-1.1`, `Ubuntu-font-1.0`, `LGPL-2.1` comentados). Saída: `cargo deny check licenses` → **exit 0, «licenses ok»**; `bans`+`sources` → «bans ok, sources ok». Dois avisos `parse-error` porque `LGPL-2.1` é identificador SPDX **obsoleto/ambíguo** (não diz only/or-later) nos dois crates `libmpv2*`. **`cargo deny check advisories` FALHOU** (não consegue obter a base de dados de avisos de segurança sem rede): nada concluído sobre vulnerabilidades.
- **Tarefa 3 — `docs/legal/COMPONENTES.md`:** licenças lidas nos ficheiros `copyright` dos pacotes Debian, em `ffmpeg -version`, no `COPYING` do oniguruma e no `LICENSE` do `whisper.cpp` dentro do crate, com data. Descoberto: a `libmpv` instalada é um build **GPL** (opção `gpl` no binário); o `ffmpeg` do sistema é `--enable-gpl`; o `keyring` puxa a **`libdbus-1` em C** (dependência de build e de execução; o resto do D-Bus do VAD é `zbus` Rust). Itens que **não** verifiquei estão marcados ❓.
- **Tarefa 4 — avisos gerados:** `scripts/gen_third_party_notices.py` gera `THIRD_PARTY_NOTICES.md` (855 KB, 255 textos distintos deduplicados, **inclui as licenças das fontes embebidas pelo egui**: OFL, UFL, Hack, emoji-icon-font). `--check` falha se estiver desatualizado; e um teste (`third_party_notices_header_matches_the_current_cargo_lock`) falha em `cargo test` se o `Cargo.lock` mudar sem regenerar — **apanhou-me a mim** ao adicionar o `sha2` como dev-dep. **16 crates não trazem ficheiro de licença na fonte** (ex. `egui`, `eframe`, `candle-nn`, `whisper-rs-sys`): ficam listados no fim do ficheiro; o texto padrão MIT/Apache terá de ser suprido (🧑‍⚖️).
- **Tarefa 5 — modelos:** `ModelPreset` ganhou `license`, `source_page`, `verified_on`. **`verified_on` é `None` em todos:** `huggingface.co` está bloqueado neste ambiente, por isso as licenças (MIT para os Whisper ggml, Apache-2.0 para Qwen2.5-0.5B-Instruct e tokenizer) são as **declaradas pelo autor, não confirmadas na página**. A UI diz «por verificar» antes da descarga (visto no ecrã, painel Whisper e resumo) e no Sobre. Testes: nenhum preset sem licença/fonte; nenhuma «verificação» sem data válida.
- **Tarefa 6 — privacidade:** `PRIVACY.md` com o inventário (7 linhas, em português simples), a regra «sem telemetria», onde ficam chaves e dados, e uma lista de anfitriões legível por máquina. **Fonte única:** `privacy_tests.rs` falha se o código nomear um anfitrião que não esteja na lista, e se a lista tiver um anfitrião que o código já não usa (mutação: apagar `api.anthropic.com` da lista faz o teste falhar). Teste também que não há crates de telemetria no `Cargo.lock`. **Medição de sessão com `strace -f -e trace=%network`** (todas as chamadas de rede, não só `connect`; binário release, abrir o ficheiro, reproduzir, abrir painéis Whisper/IA e o Sobre): 3807 chamadas, **todas em sockets `AF_UNIX` (X11/D-Bus), zero `AF_INET/AF_INET6`** (uma primeira passagem só com `connect` deu o mesmo, 11 `connect()` AF_UNIX). Essa sessão **não** incluiu transcrição, resumo nem URLs (sem modelos nem rede aqui), por isso não prova esses caminhos. Logs/`tracing` sem chaves: coberto na Sprint_14. **Transcrições nos logs — auditadas e corrigidas (revisão):** o aviso de retry de `http.rs` e os `error!` do `whisper_panel` imprimiam a mensagem do erro, que cita até 200 caracteres do corpo do provider, e um provider pode devolver o prompt (texto da transcrição) nesse corpo. Agora só se regista o **tipo** do erro (`action().title`). Teste novo com uma frase-canário ecoada pelo servidor em respostas 4xx/5xx nos 3 providers; **validado por mutação** (voltar a logar a mensagem faz o teste falhar com «TRANSCRIPT TEXT LEAKED INTO THE LOG»). O texto do erro continua a ir para a UI. Avisos informativos: nota de consentimento no painel de transcrição e nota de termos/direito de autor no diálogo de URL (estáticas, sempre visíveis, não «no primeiro uso»).
- **Tarefa 7 — distribuição (correção após revisão):** o `build.rs` só vigiava `.git/HEAD`, que não muda ao commitar num ramo, e a captura do Sobre mostrava um commit que não podia conter o próprio diálogo. Corrigido: vigia a ref resolvida, `packed-refs` e `index`, e acrescenta `-dirty` se houver alterações em ficheiros seguidos. Diálogo **Sobre** (versão + commit via `build.rs`, licença, ligação ao código-fonte, licenças dos modelos, texto GPL e avisos de terceiros embebidos no binário). `metainfo.xml` **esqueleto**: `<project_license>` fica `TODO-DECISAO-DO-AUTOR` de propósito. O manifesto Flatpak continua por construir e **não fixa** versões/hashes (TODO explícitos): o requisito «fonte correspondente com versões fixas» **não está cumprido**. Codecs com patentes: não analisados.
- **Tarefa 8 — marcas/conteúdo:** o repositório **não contém nenhum ficheiro binário de imagem/ícone/fonte** (`git ls-files` sem png/svg/ico/ttf…), logo não há assets de terceiros. «VLC/VideoLAN» só aparece em comparações no plano/README e numa nota de não afiliação no Sobre. Conflito do nome «VAD» (marcas): fica para ti.

## Desvios face ao Sprint_Planning_17.md

- Sem `vad_screenshot` (não existe aqui): usei capturas Xvfb (`import`).
- Licenças de modelos **não confirmadas na fonte** (rede bloqueada).
- `cargo deny check advisories` não correu (sem rede).
- Teste «uma sessão completa não liga a nada fora da lista»: só sessão parcial (ver acima).
- Avisos informativos estáticos em vez de «primeiro uso».
- Sprint_16 não feita.

## Problemas encontrados

- `LGPL-2.1` (obsoleto/ambíguo) nos bindings da libmpv.
- `keyring` introduz dependência em `libdbus-1` (C).
- 16 crates sem ficheiro de licença na fonte.
