# Sprint_Report_00 — Relatório de fecho

**Milestone:** M0 — Baseline real do VLC
**Baseado em:** `Sprint_00.md`
**Data:** 2026-09-17

---

## Resumo

Baseline do VLC medido nesta máquina com o pacote `.deb` 3.0.23 (snap 3.0.20
removido por falhar o driver gráfico): arranque mediano 1.831 s (inclui 1s de
playback + saída; líquido ≈0.83 s), RAM ~197.4 MiB em pausa / ~197.7 MiB em
playback, CPU ~1% em pausa. Tabela §2 do `PLANO_VAD.md` atualizada com os
números reais + metodologia. Nenhum código do VAD foi tocado (fora de âmbito).

## Entregue vs. planeado

| Tarefa (planning) | Estado |
| :--- | :--- |
| 1. Trocar snap → .deb, confirmar sem `libGL error` | ✅ feito (bloco sudo corrido pelo utilizador no terminal dele, a pedido dele); 0 ocorrências `libGL`/`iris` no log |
| 2. RSS idle + playback | ✅ feito (pausa 202108 KB; playback ~202460 KB) |
| 3. Arranque multi-run, mediana | ✅ feito (hyperfine 10 runs + 2 warmup, mediana 1.831 s calculada do `--export-json`) |
| 4. CPU em pausa ≈0% | ✅ feito (`top` instantâneo ~1%, estado `Paused` via MPRIS) |
| 5. Ficheiro de teste registado | ✅ feito (sintético 1080p30 H.264 + AAC, 90s; comando gerador + `ffprobe` registados; vive em `/tmp/`, reprodutível para M6) |
| 6. Atualizar `PLANO_VAD.md` §2 | ✅ feito |

## Critério de saída — cumprido?

**Sim.** Números reais na tabela §2 com metodologia (comando exato, ficheiro,
nº de repetições). O ramo alternativo (`.deb` também falha) não foi preciso —
o `.deb` reproduz sem `libGL error`.

## Problemas encontrados e resolução

1. `ps %cpu` em pausa mostrava 54–71% (média cumulativa desde o arranque, não
   instantâneo) — resolvido medindo com `top -b -d 1` (~1%). Armadilha
   documentada no diário para quem repetir em M6.
2. `hyperfine` sem `--run-time` num ficheiro de 90s = 15 min de runs (timeout) —
   resolvido com `--run-time 1`; número líquido de arranque derivado por
   subtração e declarado como tal.
3. VA-API indisponível nesta sessão (drivers iHD/i965 ausentes, fallback SW) —
   sem resolução; CPU de playback (73–86%) é limite superior de SW decode de
   padrão sintético, registado como caveat.

## Dívida técnica / riscos para sprints seguintes

- **Sprint 01 (gate M0.5):** o fallback SW observado (`vaInitialize` falha,
  HUD deverá mostrar `SW (CPU)`) é exatamente o caso que o gate manda validar
  no `mpv_render_context` — esta máquina é bom campo de teste para isso.
- **M6 (comparação):** regenerar o ficheiro com o comando registado (o atual
  está em `/tmp/` e é volátil); repetir com `top` (não `ps`) para CPU e
  `--run-time 1` + mediana do JSON para arranque, para os números serem
  comparáveis.
- VA-API indisponível pode afetar também a medição do VAD em M6 (ambos em SW
  nessa altura) — comparação continua válida desde que as condições sejam as
  mesmas; se o driver for corrigido entretanto, re-medir ambas as pontas.
