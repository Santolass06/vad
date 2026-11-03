//! Sprint_10 gate (M5a): automatic pre-screening of LocalQwen translations.
//!
//! These heuristics only **flag suspects** for the manual review required by
//! `PLANO_VAD.md` §9 (M5a). They never pass the gate on their own: a segment with
//! no flag still needs a human reading it (see `docs/Sprint_10/`).

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flag {
    /// Empty output.
    Empty,
    /// Output identical to the source (not translated).
    Untranslated,
    /// Output far longer than the source: padding / invented content.
    Padding,
    /// Output far shorter than the source: dropped content.
    Truncated,
    /// Digits present in only one of the two texts.
    NumberMismatch,
    /// Meta-commentary ("Aqui está", "Translation:", "Nota:").
    Commentary,
    /// Output still looks Portuguese (source-language leakage).
    SourceLeak,
    /// Source contained a numbering marker like `[3]` that leaked through.
    Marker,
}

impl Flag {
    /// Category used in the failure-rate table: hallucination/padding vs. other.
    pub fn is_hallucination_or_padding(self) -> bool {
        matches!(self, Flag::Padding | Flag::Commentary | Flag::NumberMismatch)
    }
}

const PT_MARKERS: [&str; 14] = [
    " não ", " está ", " são ", " uma ", " também ", " então ", " isso ", " vamos ", " temos ",
    " fica ", " obrigado ", " para o ", " na ", " do ",
];

fn digits(s: &str) -> HashSet<char> {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Returns the flags raised for one `(source, translation)` pair.
pub fn screen(source: &str, out: &str) -> Vec<Flag> {
    let mut flags = Vec::new();
    let (s, o) = (source.trim(), out.trim());
    if o.is_empty() {
        return vec![Flag::Empty];
    }
    if s.eq_ignore_ascii_case(o) {
        flags.push(Flag::Untranslated);
    }
    let (sw, ow) = (words(s) as f64, words(o) as f64);
    // Short fragments get an absolute allowance: "Pois." -> "Right, yes." is fine.
    if ow > sw * 2.2 + 3.0 {
        flags.push(Flag::Padding);
    }
    if sw >= 5.0 && ow < sw * 0.4 {
        flags.push(Flag::Truncated);
    }
    if digits(s) != digits(o) {
        flags.push(Flag::NumberMismatch);
    }
    let lower = format!(" {} ", o.to_lowercase());
    for m in ["aqui está", "here is", "here's the", "translation:", "tradução:", "nota:", "note:", "```"] {
        if lower.contains(m) {
            flags.push(Flag::Commentary);
            break;
        }
    }
    if o.contains('[') && o.contains(']') && !s.contains('[') {
        flags.push(Flag::Marker);
    }
    let pt_hits = PT_MARKERS.iter().filter(|m| lower.contains(*m)).count();
    if pt_hits >= 2 && !flags.contains(&Flag::Untranslated) {
        flags.push(Flag::SourceLeak);
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_provider::tests::real_qwen;
    use crate::translator::{LlmTranslator, TargetLanguage};
    use crate::whisper::TranscriptionSegment;
    use std::fmt::Write as _;
    use std::sync::Arc;

    #[test]
    fn test_screen_flags_each_failure_shape() {
        assert_eq!(screen("Bom dia.", ""), vec![Flag::Empty]);
        assert!(screen("Bom dia a todos.", "Bom dia a todos.").contains(&Flag::Untranslated));
        assert!(screen("Pois.", "Yes, that is exactly the thing that we discussed at length in the meeting last week.")
            .contains(&Flag::Padding));
        assert!(screen("Perdemos três clientes importantes este trimestre.", "Lost.")
            .contains(&Flag::Truncated));
        assert!(screen("Perdemos 3 clientes.", "We lost 5 clients.").contains(&Flag::NumberMismatch));
        assert!(screen("Bom dia.", "Here is the translation: Good morning.").contains(&Flag::Commentary));
        assert!(screen("Vamos começar a reunião.", "Vamos então começar não a reunião.")
            .contains(&Flag::SourceLeak));
    }

    #[test]
    fn test_screen_accepts_clean_translations() {
        assert!(screen("Bom dia a todos.", "Good morning, everyone.").is_empty());
        assert!(screen("Pois.", "Right.").is_empty());
        assert!(screen("Perdemos 3 clientes.", "We lost 3 customers.").is_empty());
    }

    /// Runs the whole 100-segment corpus through the real LocalQwen for each target
    /// language and writes `$VAD_GATE_OUT` (default `gate_results.md`) as the manual
    /// review sheet. Does NOT decide the gate: every row still needs a human verdict.
    #[test]
    #[ignore = "needs VAD_TEST_QWEN_MODEL and VAD_TEST_QWEN_TOKENIZER; ~minutes on CPU"]
    fn gate_sprint10_translate_corpus() {
        let corpus: Vec<&str> = include_str!("../../../docs/Sprint_10/corpus_pt.txt")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        assert_eq!(corpus.len(), 100, "the gate needs exactly 100 segments");
        let qwen = Arc::new(real_qwen(crate::llm_provider::DEFAULT_QWEN_MAX_OUTPUT_TOKENS).expect("env vars not set"));
        let translator = LlmTranslator::new(qwen);
        let segments: Vec<TranscriptionSegment> = corpus
            .iter()
            .enumerate()
            .map(|(i, t)| TranscriptionSegment { start_ms: i as i64 * 4000, end_ms: i as i64 * 4000 + 3500, text: t.to_string() })
            .collect();

        // PLANO §9: 100 segments PT->EN/ES/FR. The same 100 go through each pair.
        let mut md = String::from("# Folha de revisão — Sprint_10\n\n| # | Par | PT | Tradução | Flags auto | Veredicto (OK/ALUC/ENCH/OUTRO) |\n|---|---|---|---|---|---|\n");
        let mut summary = String::new();
        for lang in [TargetLanguage::English, TargetLanguage::Spanish, TargetLanguage::French] {
            let out = translator
                .translate_segments(&segments, "Português", lang, None::<fn(usize, usize)>, None)
                .expect("translation");
            let (mut flagged, mut halluc) = (0, 0);
            for (i, (src, dst)) in segments.iter().zip(&out).enumerate() {
                let flags = screen(&src.text, &dst.text);
                if !flags.is_empty() {
                    flagged += 1;
                }
                if flags.iter().any(|f| f.is_hallucination_or_padding()) {
                    halluc += 1;
                }
                let _ = writeln!(
                    md,
                    "| {} | pt→{} | {} | {} | {:?} | |",
                    i + 1,
                    lang.iso_code(),
                    src.text,
                    dst.text.replace('|', "\\|").replace('\n', " ⏎ "),
                    flags
                );
            }
            let _ = writeln!(summary, "pt→{}: {flagged}/100 com flag automática, {halluc}/100 suspeitos de alucinação/enchimento", lang.iso_code());
        }
        md.push_str("\n## Resumo automático\n\n");
        md.push_str(&summary);
        let path = std::env::var("VAD_GATE_OUT").unwrap_or_else(|_| "gate_results.md".into());
        std::fs::write(&path, &md).expect("write review sheet");
        println!("{summary}\nreview sheet -> {path}");
    }
}
