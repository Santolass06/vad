# Sprint_Planning_00 — M0: Baseline real do VLC

**Milestone:** M0 (`PLANO_VAD.md` §9)
**Pré-requisito:** nenhum — primeira sprint do projeto.

## Objetivo

Medir o VLC real nesta máquina para substituir os valores "a medir" da tabela
comparativa (§2). Sem isto, qualquer alvo de performance do VAD é inventado.

## Contexto / risco conhecido

O VLC instalado é a versão **snap** e já falhou a inicializar o driver gráfico
(`libGL error: failed to load driver: iris`) num teste anterior — ver `PLANO_VAD.md`
§1, "Verificado nesta máquina". Medir o snap avariado contaminaria o baseline com um
número que não representa um VLC funcional.

## Tarefas

1. Trocar o VLC snap pelo pacote `.deb`/apt (`sudo snap remove vlc` +
   `sudo apt install vlc`). Confirmar que reproduz vídeo sem `libGL error` antes de
   medir qualquer coisa.
2. Medir RSS (RES) em dois estados: idle após abrir um ficheiro e pausar; durante
   playback normal.
3. Medir tempo de arranque com várias repetições (ex. `hyperfine`), reportar a
   mediana, não uma amostra única.
4. Medir CPU em pausa (`top`/`ps` durante alguns segundos), confirmar que estabiliza
   perto de 0%.
5. Registar o ficheiro de teste usado (codec, resolução, duração) — sem isto o número
   não é reprodutível nem comparável ao medir de novo em M6.
6. Atualizar `PLANO_VAD.md` §2, substituindo os "a medir" pelos números reais e a
   metodologia usada.

## Fora de âmbito

Qualquer código do VAD — isso começa na Sprint_01.

## Critério de saída

Números reais na tabela §2, com metodologia registada (comando exato, ficheiro de
teste, número de repetições). Se o `.deb` também falhar a renderizar (sinal de
problema mais profundo no driver Mesa do sistema, não só do snap), documentar isso
como achado em vez de forçar um número sob condições quebradas.
