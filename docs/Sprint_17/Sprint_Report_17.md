# Sprint_Report_17 — Relatório de fecho

**Milestone:** M7 — Conformidade legal (licenças, privacidade, distribuição)
**Baseado em:** `Sprint_17.md`

---

## Resumo

**Evidência reunida; pendências 🧑‍⚖️ por resolver. Não declaro conformidade.** Há comandos/ecrãs que provam cada ponto técnico (abaixo), mas várias verificações dependem de rede (modelos, advisories, CDNs) ou de decisões jurídicas que não são minhas.

## Entregue vs. planeado

| # | Tarefa | Evidência | Estado |
| :-: | :--- | :--- | :--- |
| 1 | Licença declarada | `license-file`; SPDX por decidir | ⚠️ pendente 🧑‍⚖️ |
| 2 | `cargo-deny` | `cargo deny check licenses bans sources` → exit 0 (`scripts/check_legal.sh`) | ✅ (advisories ❌ sem rede) |
| 3 | `COMPONENTES.md` | `docs/legal/COMPONENTES.md` | ⚠️ com ❓ |
| 4 | Avisos gerados | `scripts/gen_third_party_notices.py --check` + teste | ✅ (16 crates sem texto na fonte) |
| 5 | Licenças dos modelos | testes + UI «por verificar» | ⚠️ não verificadas na fonte |
| 6 | Privacidade | `PRIVACY.md` + `privacy_tests` + `strace` | ⚠️ sessão parcial |
| 7 | Distribuição | Sobre (ecrã), metainfo esqueleto | ❌ Flatpak/fontes fixas por fazer |
| 8 | Marcas/conteúdo | sem assets binários no repo | ⚠️ nome «VAD» por verificar |

## Critério de saída — cumprido?

1. `cargo deny check licenses` exit 0 com allowlist explícita e os casos do «Estado inicial» resolvidos por escrito: **sim** (`deny.toml`).
2. Avisos == `Cargo.lock` e cobrem as fontes do egui: **sim** (script + teste), com a nota dos 16 crates.
3. Todo o `ModelPreset` tem licença e fonte (teste) e a UI mostra-a antes da descarga (**vista em captura**): **sim**; verificação na página do modelo: **não**.
4. Anti-leak com `PRIVACY.md` como fonte única: **sim** (teste); sessão completa medida: **só parcial** (strace sem transcrição/resumo/URL).
5. Sobre em captura: **sim**.
6. Lista 🧑‍⚖️ entregue: **abaixo**.

## Pendências que exigem revisão humana (🧑‍⚖️)

## Problemas encontrados e resolução

## Dívida técnica / riscos

---

## 🧑‍⚖️ Lista de pendências para o utilizador (revisão humana qualificada)

Nada abaixo foi decidido nem concluído por mim.

1. **`GPL-3.0-only` ou `GPL-3.0-or-later`?** Define o campo `license` dos `Cargo.toml`, o `<project_license>` do `metainfo.xml` e o texto do Sobre.
2. **LGPL-2.1 (ligação estática dos bindings `libmpv2*`) dentro de um binário GPL-3.0** e identificador SPDX ambíguo (`LGPL-2.1` obsoleto): confirmar obrigações (relinking).
3. **Compatibilidade GPL-3.0 com a Ubuntu Font Licence 1.0 e a OFL-1.1** (fontes embebidas pelo egui) e com a `libdbus` (GPL-2+ ou AFL-2.1).
4. **Texto das licenças dos 16 crates sem ficheiro de licença** na fonte (listados em `THIRD_PARTY_NOTICES.md`): obter o aviso de copyright correto de cada um.
5. **Verificar as licenças dos modelos nas respetivas páginas** (Whisper ggml; Qwen2.5-0.5B-Instruct GGUF e tokenizer; RNNoise) e preencher `verified_on`. Eu não consegui (rede bloqueada).
6. **Patentes de codecs** (H.264/HEVC/AAC) e a política do canal de distribuição (Flathub, etc.).
7. **Flatpak:** App ID, fixar versões e hashes de `ffmpeg`/`yt-dlp`/`mpv`, código-fonte correspondente por release, `metainfo.xml` válido.
8. **Redação final** dos avisos de consentimento de transcrição e de termos/direito de autor de URLs (`PRIVACY.md`, painel Whisper, diálogo de URL).
9. **Nome «VAD»:** verificação de conflito de marcas.
10. **CDNs intermédios** (para onde o `huggingface.co` redireciona; sites de vídeo via `yt-dlp`): não enumerados no `PRIVACY.md`.
11. **`cargo deny check advisories`** (vulnerabilidades conhecidas): correr com rede.
12. **Texto de `PRIVACY.md` em geral** e a afirmação «sem telemetria»: está sustentada por testes e por uma sessão `strace` parcial, não por auditoria completa.
