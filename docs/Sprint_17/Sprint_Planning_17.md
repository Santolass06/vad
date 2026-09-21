# Sprint_Planning_17 — M7: Conformidade legal (licenças, privacidade, distribuição)

**Milestone:** M7 (`PLANO_VAD.md` §9, §4.38)
**Pré-requisito:** Sprint_14 fechada (o teste anti-leak, §10.7, é reutilizado aqui) e
Sprint_15 fechada (o empacotamento Flatpak cria as obrigações de distribuição). A
Sprint_16 é opcional; se foi feita, o OpenSubtitles entra no inventário da tarefa 5.

## Aviso de âmbito (ler primeiro)

Esta sprint produz **evidência técnica e documentos verificáveis**. **Não é um parecer
jurídico**, e o agente que a executa nunca escreve "em conformidade" no relatório: só
"evidência reunida" e a lista de pendências que exigem revisão humana qualificada
(marcadas 🧑‍⚖️ abaixo). Onde o texto de uma licença ou de uma política externa importa,
cita-se a fonte e a data de verificação — nada é assumido de memória.

## Estado inicial (medido em 2026-09-21, antes das Sprints 10–16)

- `LICENSE` na raiz é o texto da **GPL-3.0**; o `README.md` cita-o (decisão do §1:
  binário GPL-compatível por causa da `libmpv`). **Nenhum `Cargo.toml` tem o campo
  `license`.**
- Grafo de dependências Rust para Linux: **410 crates de terceiros, 30 expressões de
  licença distintas, 0 sem campo `license`** (`cargo metadata --filter-platform
  x86_64-unknown-linux-gnu`). Maioria `MIT OR Apache-2.0`. Casos que exigem decisão
  escrita, não silêncio:
  - `libmpv2` / `libmpv2-sys`: **LGPL-2.1** (os bindings; a `libmpv` em si é outro
    componente, ver tarefa 3);
  - `self_cell`: `Apache-2.0 OR GPL-2.0-only` → escolher explicitamente Apache-2.0;
  - `webpki-roots`: `CDLA-Permissive-2.0` (licença de dados, exige aviso);
  - 18 crates `Unicode-3.0` (exige aviso);
  - `epaint_default_fonts` (fontes **embutidas no binário** pelo egui):
    `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`.
- O `ffmpeg` desta máquina é compilado com `--enable-gpl` (GPL, não LGPL).
- Os números acima serão diferentes no fim do projeto: **re-medir** na tarefa 2.

## Tarefas

1. **Licença do projeto declarada em todo o lado.** Campo `license` (SPDX) em todos os
   `Cargo.toml` do workspace, coerente com o `LICENSE`. 🧑‍⚖️ Decisão do utilizador a
   registar no §4: `GPL-3.0-only` ou `GPL-3.0-or-later` (o `LICENSE` atual não decide
   isto sozinho). Avaliar cabeçalhos SPDX (`// SPDX-License-Identifier: …`) nos `.rs`;
   se adotados, verificar por script, não à mão.
2. **Auditoria das dependências Rust com `cargo-deny`** (ferramenta de desenvolvimento,
   **não** dependência do produto): `deny.toml` versionado com allowlist explícita de
   licenças compatíveis com a GPL-3.0 e regras para fontes/crates banidos. Resolver por
   escrito, no `deny.toml`, cada um dos casos do "Estado inicial". Re-medir a contagem
   (crates, expressões) e registá-la no diário.
3. **Componentes não-Rust e binários externos → `docs/legal/COMPONENTES.md`.** Tabela
   com componente, licença **verificada na fonte** (cabeçalho, repositório ou
   `--version`, com data), forma de ligação (dinâmica, estática, subprocesso) e
   obrigação resultante. No mínimo: `libmpv` (GPLv2+ via libavcodec/libavfilter, §1),
   `ffmpeg` (GPL), `yt-dlp`, `whisper.cpp`/`ggml` (compilados via `whisper-rs-sys`),
   `oniguruma` (via `tokenizers`), `candle`. Nota: um subprocesso (`ffmpeg`/`yt-dlp`
   do sistema) e um binário embutido no Flatpak (Sprint_15) **não têm as mesmas
   obrigações** — a tarefa 7 trata do segundo caso.
4. **Avisos de terceiros gerados, não escritos à mão** — `THIRD_PARTY_NOTICES.md` a
   partir do `Cargo.lock` (avisos de copyright e texto de cada licença, como exigem
   MIT/Apache/Unicode/OFL/Ubuntu Font Licence), **incluindo as fontes embutidas pelo
   egui**. Script de verificação que falha se o ficheiro estiver desatualizado face ao
   `Cargo.lock`. Se o gerador for uma ferramenta externa, não entra como dependência
   do produto.
5. **Modelos de IA e dados descarregados.** Estender `ModelPreset` com `license`,
   `source_url` e `verified_on`, para cada ficheiro que o VAD descarrega (Whisper ggml,
   Qwen2.5-0.5B-Instruct e tokenizer, e o `.rnnn` do RNNoise se o §4.35 vier a ter
   descarga). **Confirmar cada licença na página do modelo à data da sprint** (não
   assumir). Teste automático: nenhum preset sem licença registada. A UI mostra a
   licença **antes** da descarga (mesmo padrão do §4.13). Regra que se mantém: o VAD
   **nunca redistribui pesos**, só os descarrega do anfitrião original a pedido do
   utilizador.
6. **Privacidade e fluxos de dados.**
   - Inventário de **todas** as ligações de saída (descarga de modelos, URLs do
     utilizador via mpv/yt-dlp, providers cloud das Sprints 11–13, OpenSubtitles se
     existir) em `PRIVACY.md`, em linguagem simples: o que sai do PC, quando, para
     quem, e o que **não** sai. Sem telemetria — provado, não afirmado.
   - **Fonte única com o teste anti-leak (§10.7):** a lista de destinos permitidos do
     teste é a do inventário; uma sessão completa (abrir, transcrever, resumir local,
     exportar) não pode ligar a nada fora dela.
   - Consentimento visível antes do primeiro envio cloud (o badge ☁️ do §4.1/§4.21 já
     existe) com texto do que é enviado. Verificar que **logs e `tracing` nunca
     imprimem chaves de API nem texto de transcrição**.
   - Aviso informativo no primeiro uso da transcrição: gravar/transcrever terceiros
     pode exigir consentimento segundo a lei local; é responsabilidade do utilizador.
     🧑‍⚖️ Redação final por revisão humana (o VAD não presta aconselhamento jurídico).
7. **Distribuição (Flatpak/binário).**
   - Texto da licença, `THIRD_PARTY_NOTICES.md` e aviso de licença acompanham o binário.
   - **Código-fonte correspondente:** o que o binário GPL-3.0 distribuído inclui (VAD,
     e o `ffmpeg`/`libmpv`/`yt-dlp` embutidos no manifesto) tem de ter fonte acessível:
     tag/commit por release e versões **fixas** (com hash) das fontes embutidas no
     manifesto. Verificar que o manifesto as fixa.
   - `metainfo.xml` (AppStream) com `<project_license>` correto.
   - Diálogo **Sobre** na app: licença, versão/commit, ligação ao código-fonte, avisos de
     terceiros, licenças dos modelos.
   - 🧑‍⚖️ Codecs com patentes (H.264/HEVC/AAC): ver a política do canal de distribuição
     escolhido e registar a decisão; o agente não conclui nada sobre patentes.
8. **Marcas, nomes e conteúdo de terceiros.** Não usar nome, logótipo ou cone do VLC/
   VideoLAN; "VLC", OpenAI, Anthropic, Google/Gemini e Qwen só em referência nominativa,
   com nota de não afiliação. Confirmar que nenhum asset do repositório é de terceiros
   sem licença registada. Reprodução por URL: aviso no primeiro uso de que os termos das
   plataformas e o direito de autor são responsabilidade do utilizador; o VAD não
   redistribui conteúdo. 🧑‍⚖️ Verificação de conflito do nome "VAD" (registo de marcas):
   fica para o utilizador.

## Fora de âmbito

Parecer jurídico; análise de liberdade de operação de patentes; registo de marca;
requisitos setoriais (ex.: acessibilidade regulamentar); qualquer alteração de
funcionalidade que não seja um aviso, um registo de licença ou um diálogo informativo.

## Critério de saída

Cada ponto tem um comando ou ecrã que o prova:

1. `cargo deny check licenses` termina com código 0 com allowlist explícita, e os casos
   do "Estado inicial" estão resolvidos por escrito (saída colada no diário).
2. O script de verificação confirma `THIRD_PARTY_NOTICES.md` == `Cargo.lock` atual e o
   ficheiro cobre as fontes embutidas do egui.
3. Teste automático: todo o `ModelPreset` tem licença e fonte; a licença aparece na UI
   antes da descarga (**visto por `vad_screenshot`**).
4. O teste anti-leak passa com a lista do `PRIVACY.md` como única fonte; uma sessão
   completa medida não liga a nada fora dela.
5. O diálogo **Sobre** aparece em screenshot com licença, fonte e avisos.
6. O relatório termina com a **lista de pendências 🧑‍⚖️** entregue ao utilizador. O
   critério **não** é "estar em conformidade": é "evidência reunida e pendências
   explícitas".
