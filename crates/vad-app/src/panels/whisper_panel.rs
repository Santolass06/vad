use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Ui};
use tracing::{error, info};
use vad_ai::{
    find_preset, AiPrivacyBadge, DiskModelInfo, LlmTranslator, LocalQwenSummarizer,
    MapReduceSummarizer, MeetingSummary, MockSummarizer, ModelManager, ModelPreset, ModelSource,
    PcmAudio, SummarizeProgress, Summarizer, TargetLanguage, TranscriptionSegment, WhisperEngine,
    DEFAULT_QWEN_CONTEXT_WINDOW, DEFAULT_QWEN_MAX_OUTPUT_TOKENS, DEFAULT_QWEN_MODEL_FILENAME,
    DEFAULT_QWEN_TOKENIZER_FILENAME, DISK_TOOLTIP, PRESET_MODELS, RAM_ONLY_TOOLTIP,
};
use vad_core::{get_process_rss_bytes, ModelStorageMode, VadConfig};

/// Renders a standardized AI privacy badge (🔒 Local / ☁️ Sai do PC) per PLANO_VAD.md §4.1.
pub fn render_privacy_badge(ui: &mut Ui, badge: AiPrivacyBadge) {
    let (bg, text_color) = match badge {
        AiPrivacyBadge::Local => (
            Color32::from_rgba_premultiplied(34, 197, 94, 35),
            Color32::from_rgb(74, 222, 128),
        ),
        AiPrivacyBadge::Cloud => (
            Color32::from_rgba_premultiplied(234, 179, 8, 35),
            Color32::from_rgb(250, 204, 21),
        ),
    };

    egui::Frame::new()
        .fill(bg)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(badge.label()).size(10.5).color(text_color).strong());
        })
        .response
        .on_hover_text(badge.tooltip());
}

/// Idle time after which a RAM-only Whisper model is released (§4.16).
pub const IDLE_UNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// Trigger for Whisper model unloading (§4.16, Sprint_Planning_07 risk): an automatic timeout is
/// never confused with the user asking for the model to go. Replacing a model by another one is
/// not an unload: the old engine is dropped only once its successor has loaded (§4.14).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnloadTrigger {
    /// Inactivity timeout elapsed without use on a RAM-only model (§4.16).
    Inactivity,
    /// The user pressed "Descarregar".
    ExplicitClose,
}

/// Action emitted by the Whisper panel to interact with the media player.
#[derive(Debug, Clone)]
pub enum WhisperAction {
    SeekTo(f64),
    SaveConfig,
}

/// State for the lateral Whisper AI panel per design/Meeting.dc.html and PLANO_VAD.md §4.13.
pub struct WhisperPanel {
    model_manager: ModelManager,
    selected_preset_idx: usize,
    storage_mode: ModelStorageMode,
    /// Shared handle: the transcription thread clones the `Arc` and releases the lock at once,
    /// so the UI thread is never blocked on this mutex while a transcription runs.
    active_engine: Arc<Mutex<Option<Arc<WhisperEngine>>>>,
    active_model_id: Arc<Mutex<Option<String>>>,
    /// Tracks the storage mode under which the active engine was loaded (§4.16).
    active_storage_mode: Arc<Mutex<Option<ModelStorageMode>>>,
    /// Timestamp of last activity with the Whisper model (§4.16).
    last_activity: Arc<Mutex<Instant>>,
    download_progress: Arc<Mutex<Option<f32>>>,
    /// Set from the model-loading threads (download or engine initialisation failures).
    download_error: Arc<Mutex<Option<String>>>,
    transcribing: Arc<AtomicBool>,
    transcribe_progress: Arc<Mutex<Option<i32>>>,
    transcription_segments: Vec<TranscriptionSegment>,
    transcription_error: Option<String>,
    transcription_rx: Option<crossbeam_channel::Receiver<Result<Vec<TranscriptionSegment>, String>>>,
    cached_disk_models: Vec<DiskModelInfo>,
    last_models_refresh: Instant,
    export_notification: Option<(String, Instant)>,

    // --- LLM & Summarization state (M5a) ---
    is_summarizing: Arc<AtomicBool>,
    summarize_progress: Arc<Mutex<Option<SummarizeProgress>>>,
    summarize_error: Option<String>,
    summarize_abort: Arc<AtomicBool>,
    meeting_summary: Option<MeetingSummary>,
    summary_rx: Option<crossbeam_channel::Receiver<Result<MeetingSummary, String>>>,

    // --- Translation state (M5a) ---
    is_translating: Arc<AtomicBool>,
    translation_progress: Arc<Mutex<Option<(usize, usize)>>>,
    translation_error: Option<String>,
    translation_abort: Arc<AtomicBool>,
    selected_target_lang: TargetLanguage,
    translated_segments: Option<Vec<TranscriptionSegment>>,
    translation_rx: Option<crossbeam_channel::Receiver<Result<Vec<TranscriptionSegment>, String>>>,
    show_translated_subtitles: bool,

    // --- LLM Model / Test state ---
    use_mock_llm: bool,
}

impl Default for WhisperPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl WhisperPanel {
    pub fn new() -> Self {
        let manager = ModelManager::new();
        let disk_models = manager.list_disk_models();

        Self {
            model_manager: manager,
            // Default to base-q5 (~55 MB) per §5
            selected_preset_idx: 1,
            storage_mode: ModelStorageMode::Disk,
            active_engine: Arc::new(Mutex::new(None)),
            active_model_id: Arc::new(Mutex::new(None)),
            active_storage_mode: Arc::new(Mutex::new(None)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
            download_progress: Arc::new(Mutex::new(None)),
            download_error: Arc::new(Mutex::new(None)),
            transcribing: Arc::new(AtomicBool::new(false)),
            transcribe_progress: Arc::new(Mutex::new(None)),
            transcription_segments: Vec::new(),
            transcription_error: None,
            transcription_rx: None,
            cached_disk_models: disk_models,
            last_models_refresh: Instant::now(),
            export_notification: None,

            is_summarizing: Arc::new(AtomicBool::new(false)),
            summarize_progress: Arc::new(Mutex::new(None)),
            summarize_error: None,
            summarize_abort: Arc::new(AtomicBool::new(false)),
            meeting_summary: None,
            summary_rx: None,

            is_translating: Arc::new(AtomicBool::new(false)),
            translation_progress: Arc::new(Mutex::new(None)),
            translation_error: None,
            translation_abort: Arc::new(AtomicBool::new(false)),
            selected_target_lang: TargetLanguage::English,
            translated_segments: None,
            translation_rx: None,
            show_translated_subtitles: false,

            use_mock_llm: false,
        }
    }

    /// Syncs settings from `VadConfig` (§5).
    pub fn init_from_config(&mut self, config: &VadConfig) {
        self.storage_mode = config.whisper.default_storage_mode;
    }

    /// Refreshes list of models stored in `~/.local/share/vad/models/`.
    pub fn refresh_disk_models(&mut self) {
        self.cached_disk_models = self.model_manager.list_disk_models();
        self.last_models_refresh = Instant::now();
    }

    /// Storage mode currently selected in the panel.
    pub fn storage_mode(&self) -> ModelStorageMode {
        self.storage_mode
    }

    /// Storage mode under which the currently active model was loaded (§4.13, §4.16).
    pub fn active_storage_mode(&self) -> Option<ModelStorageMode> {
        *self.active_storage_mode.lock().ok()?
    }

    /// Returns true if a model is currently loaded in memory and ready for inference.
    pub fn has_active_model(&self) -> bool {
        self.active_engine
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    /// Returns the name of the currently active model.
    pub fn active_model_id(&self) -> Option<String> {
        self.active_model_id.lock().ok()?.clone()
    }

    /// Resets the last activity timer to the current moment.
    pub fn touch_activity(&self) {
        if let Ok(mut g) = self.last_activity.lock() {
            *g = Instant::now();
        }
    }

    /// Duration elapsed since the last activity with the Whisper model (§4.16).
    pub fn last_activity_elapsed(&self) -> Duration {
        self.last_activity
            .lock()
            .map(|g| g.elapsed())
            .unwrap_or_default()
    }

    /// Unloads the active Whisper model from memory, recording the trigger and RSS memory delta.
    ///
    /// Per PLANO_VAD.md §4.16 and Sprint_Planning_07 risk:
    /// Differentiates between automatic inactivity unload and explicit actions.
    /// Returns the number of bytes freed from process Resident Set Size (RSS), if measurable.
    pub fn unload_model(&mut self, trigger: UnloadTrigger) -> Option<u64> {
        let before_rss = get_process_rss_bytes();
        let model_id = self.active_model_id.lock().ok().and_then(|mut g| g.take());
        let mode = self.active_storage_mode.lock().ok().and_then(|mut g| g.take());
        let had_engine = self.active_engine.lock().ok().and_then(|mut g| g.take()).is_some();

        if !had_engine && model_id.is_none() {
            return None;
        }

        let after_rss = get_process_rss_bytes();
        let freed_bytes = match (before_rss, after_rss) {
            (Some(b), Some(a)) => Some(b.saturating_sub(a)),
            _ => None,
        };

        info!(
            "Whisper model '{:?}' ({:?}) unloaded via trigger {:?}. RSS before: {:?} bytes, after: {:?} bytes, freed: {:?} bytes (§4.16)",
            model_id, mode, trigger, before_rss, after_rss, freed_bytes
        );

        freed_bytes
    }

    /// Time left before a RAM-only model is unloaded for inactivity (§4.16).
    ///
    /// `None` when there is nothing to schedule: no model, or one loaded in disk mode (mmap),
    /// which the kernel page cache already manages — an active unload there would only compete
    /// with the OS. The caller uses this to wake the UI: egui does not repaint an idle window,
    /// so without a scheduled repaint the timeout would never be checked.
    pub fn idle_unload_due_in(&self, timeout: Duration) -> Option<Duration> {
        if self.active_storage_mode() != Some(ModelStorageMode::RamOnly) {
            return None;
        }
        Some(timeout.saturating_sub(self.last_activity_elapsed()))
    }

    /// Unloads a RAM-only model that has been inactive for `timeout` (§4.16); returns the RSS
    /// freed if it did. Never unloads while a transcription runs.
    pub fn check_inactivity_unload(&mut self, timeout: Duration) -> Option<u64> {
        if self.is_transcribing() {
            self.touch_activity();
            return None;
        }

        if self.idle_unload_due_in(timeout)? > Duration::ZERO {
            return None;
        }

        info!(
            "RAM-only Whisper model inactive for {:.1}s (timeout: {:.1}s). Triggering automatic unload (§4.16)",
            self.last_activity_elapsed().as_secs_f64(),
            timeout.as_secs_f64()
        );
        self.unload_model(UnloadTrigger::Inactivity)
    }

    /// Forgets the transcription of the previous recording: a new file must never show, or
    /// export next to its own notes, another recording's transcript. A transcription still
    /// running for the old file has its result discarded.
    pub fn reset_for_new_media(&mut self) {
        self.transcription_segments.clear();
        self.transcription_error = None;
        self.transcription_rx = None;
        self.meeting_summary = None;
        self.summarize_error = None;
        self.summary_rx = None;
        self.translated_segments = None;
        self.translation_error = None;
        self.translation_rx = None;
    }

    /// Renders the Whisper AI lateral panel.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        current_audio: Option<&PcmAudio>,
    ) -> Option<WhisperAction> {
        let mut action = None;
        let accent = Color32::from_rgb(139, 124, 246);

        // Non-blocking poll for transcription completion
        if let Some(ref rx) = self.transcription_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(segs) => {
                        info!("Transcription finished with {} segments", segs.len());
                        self.transcription_segments = segs;
                        self.transcription_error = None;
                    }
                    Err(err) => {
                        error!("Transcription error: {}", err);
                        self.transcription_error = Some(err);
                    }
                }
                self.transcription_rx = None;
            }
        }

        // Non-blocking poll for meeting summarization completion (M5a)
        if let Some(ref rx) = self.summary_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(summary) => {
                        info!(
                            "Meeting summarization finished successfully ({} chunks, {:.1}s audio)",
                            summary.total_chunks, summary.duration_seconds
                        );
                        self.meeting_summary = Some(summary);
                        self.summarize_error = None;
                    }
                    Err(err) => {
                        error!("Meeting summarization error: {}", err);
                        self.summarize_error = Some(err);
                    }
                }
                self.summary_rx = None;
            }
        }

        // Non-blocking poll for translation completion (M5a)
        if let Some(ref rx) = self.translation_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(segs) => {
                        info!("Translation finished successfully ({} segments)", segs.len());
                        self.translated_segments = Some(segs);
                        self.translation_error = None;
                    }
                    Err(err) => {
                        error!("Translation error: {}", err);
                        self.translation_error = Some(err);
                    }
                }
                self.translation_rx = None;
            }
        }

        // Periodically refresh list of disk models (every 5 seconds)
        if self.last_models_refresh.elapsed().as_secs() > 5 {
            self.refresh_disk_models();
        }

        ui.vertical(|ui| {
            // --- SECTION 1: Model Preset Selection ---
            ui.label(
                RichText::new("MODELO")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(160, 165, 185)),
            );
            ui.add_space(4.0);

            let current_preset = PRESET_MODELS.get(self.selected_preset_idx).copied();
            let selected_name = current_preset
                .map(|p| p.display_name)
                .unwrap_or("Escolher modelo");

            egui::ComboBox::from_id_salt("whisper_preset_combo")
                .selected_text(RichText::new(selected_name).strong())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (idx, preset) in PRESET_MODELS.iter().enumerate() {
                        let is_selected = self.selected_preset_idx == idx;
                        if ui.selectable_label(is_selected, preset.display_name).clicked() {
                            self.selected_preset_idx = idx;
                        }
                    }
                });

            ui.add_space(10.0);

            // --- SECTION 2: Storage Mode Selection & Tooltips (§4.13) ---
            ui.label(
                RichText::new("ARMAZENAMENTO")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(160, 165, 185)),
            );
            ui.add_space(4.0);

            // Radio: Guardar no disco (Predefinição)
            let is_disk = self.storage_mode == ModelStorageMode::Disk;
            let disk_resp = ui.radio(is_disk, "Guardar no disco");
            if disk_resp.clicked() {
                self.storage_mode = ModelStorageMode::Disk;
                action = Some(WhisperAction::SaveConfig);
            }
            disk_resp.on_hover_text(DISK_TOOLTIP);

            // Radio: RAM-only (opt-in)
            let is_ram = self.storage_mode == ModelStorageMode::RamOnly;
            let ram_resp = ui.radio(is_ram, "RAM-only (esta sessão)");
            if ram_resp.clicked() {
                self.storage_mode = ModelStorageMode::RamOnly;
                action = Some(WhisperAction::SaveConfig);
            }
            ram_resp.on_hover_text(RAM_ONLY_TOOLTIP);

            ui.add_space(2.0);

            // Explanatory note under storage options
            let note_text = if is_disk {
                "Guardado em ~/.local/share/vad/models/. Carregado por mmap com baixo consumo de RAM."
            } else {
                "Volátil na memória. Zero ficheiros em disco. Descarrega a cada sessão."
            };
            ui.label(
                RichText::new(note_text)
                    .size(10.5)
                    .color(Color32::from_rgb(140, 145, 165)),
            );

            ui.add_space(10.0);

            // --- SECTION 3: Load / Download Model Action Button ---
            let dl_prog = *self.download_progress.lock().unwrap_or_else(|e| e.into_inner());
            let is_downloading = dl_prog.is_some();
            let active_id = self.active_model_id();

            if is_downloading {
                let pct = dl_prog.unwrap_or(0.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("A descarregar modelo: {:.0}%", pct));
                });
                ui.add(egui::ProgressBar::new(pct / 100.0).animate(true));
            } else {
                let is_current_active = active_id.as_deref() == current_preset.map(|p| p.id);

                if is_current_active {
                    ui.horizontal(|ui| {
                        ui.colored_label(accent, format!("✓ Modelo ativo: {}", active_id.as_deref().unwrap_or_default()));
                        let idle = !self.is_transcribing();
                        if ui
                            .add_enabled(idle, egui::Button::new("Descarregar").small())
                            .on_hover_text("Descarregar modelo da memória RAM")
                            .on_disabled_hover_text("Aguarda o fim da transcrição")
                            .clicked()
                        {
                            self.unload_model(UnloadTrigger::ExplicitClose);
                        }
                    });
                } else if let Some(preset) = current_preset {
                    let btn_text = if self.storage_mode == ModelStorageMode::Disk
                        && self.model_manager.is_model_on_disk(preset.filename)
                    {
                        format!("Ativar {}", preset.id)
                    } else {
                        format!("Descarregar e Ativar {}", preset.id)
                    };

                    let btn = egui::Button::new(RichText::new(btn_text).strong());
                    if ui.add_sized([ui.available_width(), 32.0], btn).clicked() {
                        self.start_model_load(preset, self.storage_mode);
                    }
                }
            }

            let load_error = self.download_error.lock().ok().and_then(|g| g.clone());
            if let Some(err) = load_error {
                ui.colored_label(Color32::RED, format!("Erro: {err}"));
            }

            ui.separator();
            ui.add_space(4.0);

            // --- SECTION 4: Models on Disk List (§4.13, Meeting.dc.html) ---
            ui.label(
                RichText::new("MODELOS NO DISCO")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(160, 165, 185)),
            );
            ui.add_space(4.0);

            if self.cached_disk_models.is_empty() {
                ui.label(
                    RichText::new("Nenhum modelo guardado em disco.")
                        .size(11.0)
                        .italics()
                        .color(Color32::from_rgb(130, 135, 155)),
                );
            } else {
                let mut to_delete = None;
                let mut to_activate = None;

                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(25, 27, 36, 255))
                    .corner_radius(8.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        for model in &self.cached_disk_models {
                            ui.horizontal(|ui| {
                                let is_active = active_id.as_deref() == Some(&model.id);

                                if is_active {
                                    ui.colored_label(accent, RichText::new(&model.id).strong());
                                } else {
                                    ui.label(&model.id);
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("🗑").on_hover_text("Eliminar modelo do disco").clicked() {
                                        to_delete = Some(model.filename.clone());
                                    }

                                    if is_active {
                                        ui.colored_label(accent, "Ativo");
                                    } else if ui.small_button("Ativar").clicked() {
                                        to_activate = Some(model.clone());
                                    }
                                });
                            });
                            ui.separator();
                        }
                    });

                if let Some(fname) = to_delete {
                    let _ = self.model_manager.delete_disk_model(&fname);
                    self.refresh_disk_models();
                }

                if let Some(model_info) = to_activate {
                    if let Some(preset) = find_preset(&model_info.filename) {
                        self.start_model_load(*preset, ModelStorageMode::Disk);
                    } else {
                        // Load custom model path directly
                        self.start_direct_disk_load(model_info.path, model_info.id);
                    }
                }
            }

            ui.separator();
            ui.add_space(4.0);

            // --- SECTION 5: Transcription ---
            ui.label(
                RichText::new("TRANSCRIÇÃO")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(160, 165, 185)),
            );
            ui.add_space(4.0);

            let is_transcribing = self.transcribing.load(Ordering::SeqCst);
            let has_model = self.has_active_model();
            let has_audio = current_audio.is_some();

            if is_transcribing {
                let prog = self.transcribe_progress.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("A transcrever: {}%", prog));
                });
                ui.add(egui::ProgressBar::new(prog as f32 / 100.0).animate(true));
            } else {
                let can_transcribe = has_model && has_audio;
                let transcribe_btn = egui::Button::new(RichText::new("🎙 Transcrever áudio").strong());

                let resp = ui.add_enabled(can_transcribe, transcribe_btn);
                if !can_transcribe {
                    if !has_model {
                        resp.on_disabled_hover_text("Ativa um modelo Whisper primeiro");
                    } else {
                        resp.on_disabled_hover_text("Aguardando extração do áudio do ficheiro");
                    }
                } else if resp.clicked() {
                    if let Some(audio) = current_audio {
                        self.start_transcription(audio.clone());
                    }
                }
            }

            if let Some(ref err) = self.transcription_error {
                ui.colored_label(Color32::RED, format!("Erro de transcrição: {err}"));
            }

            // Results List
            if !self.transcription_segments.is_empty() {
                ui.add_space(6.0);
                let scroll_h = 220.0;

                egui::ScrollArea::vertical()
                    .max_height(scroll_h)
                    .show(ui, |ui| {
                        for seg in &self.transcription_segments {
                            ui.horizontal(|ui| {
                                let time_label = TranscriptionSegment::format_timestamp(seg.start_ms);
                                if ui
                                    .link(RichText::new(time_label).color(accent).monospace())
                                    .on_hover_text("Saltar reprodução para este ponto")
                                    .clicked()
                                {
                                    action = Some(WhisperAction::SeekTo(seg.start_ms as f64 / 1000.0));
                                }

                                ui.label(RichText::new(&seg.text).size(12.0));
                            });
                            ui.add_space(3.0);
                        }
                    });

                ui.add_space(6.0);

                if ui.button("📄 Exportar .md").on_hover_text("Guardar transcrição para Markdown").clicked() {
                    let source = current_audio.map(|a| a.path.as_str());
                    let title = source
                        .and_then(|p| std::path::Path::new(p).file_name())
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Reunião".to_string());
                    let md_content = WhisperEngine::export_to_markdown(&title, &self.transcription_segments);

                    // One file per recording under ~/.local/share/vad/ — a fixed name would
                    // silently overwrite the previous meeting's transcript.
                    let stem = source
                        .and_then(|p| std::path::Path::new(p).file_stem())
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| "reuniao".to_string());
                    let export_dir = self.model_manager.models_dir().parent().unwrap_or(self.model_manager.models_dir());
                    let export_path = export_dir.join(format!("transcricao_{stem}.md"));

                    self.export_notification = Some(match std::fs::write(&export_path, md_content) {
                        Ok(()) => (format!("Guardado em {:?}", export_path), Instant::now()),
                        Err(err) => (format!("Falha ao guardar {:?}: {err}", export_path), Instant::now()),
                    });
                }
            }

            ui.separator();
            ui.add_space(4.0);

            // --- SECTION 6: Meeting Summary (LLM Local, M5a, §4.1, §4.19, §4.20) ---
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("RESUMO DA REUNIÃO")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(160, 165, 185)),
                );
                render_privacy_badge(ui, AiPrivacyBadge::Local);
            });
            ui.add_space(4.0);

            let is_summarizing = self.is_summarizing.load(Ordering::SeqCst);
            let has_segments = !self.transcription_segments.is_empty();

            if is_summarizing {
                let prog_msg = self
                    .summarize_progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .map(|p| p.display_message())
                    .unwrap_or_else(|| "A inicializar resumo...".to_string());

                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(prog_msg).size(12.0));
                });
                ui.add_space(2.0);

                if ui
                    .button(RichText::new("✕ Cancelar").color(Color32::from_rgb(239, 68, 68)))
                    .on_hover_text("Cancelar a geração do resumo")
                    .clicked()
                {
                    self.cancel_summary();
                }
            } else {
                let summarize_btn = egui::Button::new(
                    RichText::new("✨ Gerar Resumo da Reunião").strong(),
                );
                let resp = ui.add_enabled(has_segments, summarize_btn);
                if !has_segments {
                    resp.on_disabled_hover_text("Executa primeiro a transcrição com o Whisper");
                } else if resp.clicked() {
                    self.start_summary();
                }
            }

            if let Some(ref err) = self.summarize_error {
                ui.colored_label(Color32::RED, format!("Erro no resumo: {err}"));
            }

            if let Some(ref summary) = self.meeting_summary {
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(25, 27, 36, 255))
                    .corner_radius(6.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{} blocos processados ({:.1}s)", summary.total_chunks, summary.duration_seconds))
                                    .size(11.0)
                                    .color(Color32::from_rgb(160, 165, 185)),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("📋 Copiar").clicked() {
                                    ui.ctx().copy_text(summary.markdown.clone());
                                }
                            });
                        });
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(160.0)
                            .show(ui, |ui| {
                                ui.label(RichText::new(&summary.markdown).size(11.5));
                            });
                    });
            }

            ui.separator();
            ui.add_space(4.0);

            // --- SECTION 7: Multilingual Translation (M5a, §4.1) ---
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("TRADUÇÃO MULTI-IDIOMA")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(160, 165, 185)),
                );
                render_privacy_badge(ui, AiPrivacyBadge::Local);
            });
            ui.add_space(4.0);

            let is_translating = self.is_translating.load(Ordering::SeqCst);

            ui.horizontal(|ui| {
                ui.label("Destino:");
                egui::ComboBox::from_id_salt("target_lang_select")
                    .selected_text(self.selected_target_lang.display_name())
                    .show_ui(ui, |ui| {
                        for lang in TargetLanguage::ALL {
                            ui.selectable_value(&mut self.selected_target_lang, lang, lang.display_name());
                        }
                    });
            });

            if is_translating {
                let prog = self
                    .translation_progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .unwrap_or((0, self.transcription_segments.len().max(1)));

                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("A traduzir: {}/{} segmentos...", prog.0, prog.1));
                });
                ui.add(egui::ProgressBar::new(prog.0 as f32 / prog.1 as f32).animate(true));

                if ui
                    .button(RichText::new("✕ Cancelar").color(Color32::from_rgb(239, 68, 68)))
                    .on_hover_text("Cancelar tradução")
                    .clicked()
                {
                    self.cancel_translation();
                }
            } else {
                let trans_btn = egui::Button::new(
                    RichText::new(format!("🌐 Traduzir para {}", self.selected_target_lang.display_name())).strong(),
                );
                let resp = ui.add_enabled(has_segments, trans_btn);
                if !has_segments {
                    resp.on_disabled_hover_text("Transcreve o áudio primeiro");
                } else if resp.clicked() {
                    let target = self.selected_target_lang;
                    self.start_translation(target);
                }
            }

            if let Some(ref err) = self.translation_error {
                ui.colored_label(Color32::RED, format!("Erro de tradução: {err}"));
            }

            if let Some(ref trans_segs) = self.translated_segments {
                ui.add_space(2.0);
                ui.checkbox(
                    &mut self.show_translated_subtitles,
                    "Exibir tradução nas legendas / reprodução",
                );
                ui.label(
                    RichText::new(format!("{} segmentos traduzidos disponíveis", trans_segs.len()))
                        .size(11.0)
                        .color(Color32::from_rgb(160, 165, 185)),
                );
            }

            if let Some((ref msg, instant)) = self.export_notification {
                if instant.elapsed().as_secs() < 6 {
                    let color = if msg.starts_with("Falha") { Color32::RED } else { Color32::GREEN };
                    ui.colored_label(color, msg);
                }
            }
        });

        action
    }

    /// Spawns model download & initialization in a background thread.
    fn start_model_load(&mut self, preset: ModelPreset, mode: ModelStorageMode) {
        self.touch_activity();
        *self.download_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let progress_state = Arc::clone(&self.download_progress);
        let error_state = Arc::clone(&self.download_error);
        let active_engine_state = Arc::clone(&self.active_engine);
        let active_model_id_state = Arc::clone(&self.active_model_id);
        let active_storage_mode_state = Arc::clone(&self.active_storage_mode);
        let last_activity_state = Arc::clone(&self.last_activity);
        let manager = ModelManager::new();

        *progress_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(0.0);

        thread::spawn(move || {
            let prog_clone = Arc::clone(&progress_state);
            let result = manager.load_or_download_model(&preset, mode, move |pct| {
                if let Ok(mut guard) = prog_clone.lock() {
                    *guard = Some(pct);
                }
            });

            let loaded = result.and_then(|source| WhisperEngine::load(&source));
            match loaded {
                Ok(engine) => {
                    *active_engine_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(engine));
                    *active_model_id_state.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(preset.id.to_string());
                    *active_storage_mode_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(mode);
                    *last_activity_state.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                    info!(
                        "Whisper model '{}' loaded successfully ({:?}) (§4.13)",
                        preset.id, mode
                    );
                }
                Err(err) => {
                    // ModelDownloadFailed / engine init: tell the user (§4.14), keep any active model
                    error!("Failed to load model {}: {:?}", preset.id, err);
                    *error_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(err.to_string());
                }
            }

            *progress_state.lock().unwrap_or_else(|e| e.into_inner()) = None;
        });
    }

    /// Loads an existing disk model file directly.
    fn start_direct_disk_load(&mut self, path: std::path::PathBuf, id: String) {
        self.touch_activity();
        *self.download_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let error_state = Arc::clone(&self.download_error);
        let active_engine_state = Arc::clone(&self.active_engine);
        let active_model_id_state = Arc::clone(&self.active_model_id);
        let active_storage_mode_state = Arc::clone(&self.active_storage_mode);
        let last_activity_state = Arc::clone(&self.last_activity);

        thread::spawn(move || match WhisperEngine::load(&ModelSource::Disk(path)) {
            Ok(engine) => {
                *active_engine_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(engine));
                *active_model_id_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
                *active_storage_mode_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(ModelStorageMode::Disk);
                *last_activity_state.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                info!("Direct disk model '{}' loaded successfully into memory (§4.13)", id);
            }
            Err(err) => {
                error!("Failed to initialize direct disk model: {:?}", err);
                *error_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(err.to_string());
            }
        });
    }

    /// Spawns Whisper transcription on a background worker thread.
    fn start_transcription(&mut self, audio: PcmAudio) {
        self.touch_activity();
        self.transcription_error = None;
        self.transcription_segments.clear();
        self.transcribing.store(true, Ordering::SeqCst);

        let active_engine = Arc::clone(&self.active_engine);
        let transcribing = Arc::clone(&self.transcribing);
        let transcribe_progress = Arc::clone(&self.transcribe_progress);
        let last_activity = Arc::clone(&self.last_activity);

        let (tx, rx) = crossbeam_channel::bounded::<Result<Vec<TranscriptionSegment>, String>>(1);

        thread::spawn(move || {
            // Clone the handle and release the lock immediately: holding it for the whole
            // transcription would block the UI thread (`has_active_model` runs every frame).
            let engine = active_engine.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let Some(engine) = engine else {
                let _ = tx.send(Err("Nenhum modelo Whisper ativo".to_string()));
                transcribing.store(false, Ordering::SeqCst);
                return;
            };

            let prog_clone = Arc::clone(&transcribe_progress);
            let last_act_clone = Arc::clone(&last_activity);
            let progress_cb = move |pct: i32| {
                if let Ok(mut g) = prog_clone.lock() {
                    *g = Some(pct);
                }
                if let Ok(mut g) = last_act_clone.lock() {
                    *g = Instant::now();
                }
            };

            let result = engine.transcribe(&audio, None, Some(progress_cb), None::<fn() -> bool>);

            let res_mapped = result.map_err(|e| e.to_string());
            let _ = tx.send(res_mapped);

            transcribing.store(false, Ordering::SeqCst);
            *transcribe_progress.lock().unwrap_or_else(|e| e.into_inner()) = None;
            if let Ok(mut g) = last_activity.lock() {
                *g = Instant::now();
            }
        });

        self.transcription_rx = Some(rx);
    }

    /// Stores newly delivered transcription segments into state.
    #[allow(dead_code)]
    pub fn set_segments(&mut self, segments: Vec<TranscriptionSegment>) {
        self.transcription_segments = segments;
    }

    /// Returns the currently loaded transcription segments.
    pub fn transcription_segments(&self) -> &[TranscriptionSegment] {
        &self.transcription_segments
    }

    /// Returns whether transcription is currently executing.
    pub fn is_transcribing(&self) -> bool {
        self.transcribing.load(Ordering::SeqCst)
    }

    /// Returns current transcription progress percentage if active.
    pub fn transcribe_progress(&self) -> Option<i32> {
        self.transcribe_progress.lock().ok().and_then(|g| *g)
    }

    /// Returns the currently generated meeting summary, if any.
    pub fn meeting_summary(&self) -> Option<&MeetingSummary> {
        self.meeting_summary.as_ref()
    }

    /// Sets or clears the current meeting summary.
    #[allow(dead_code)]
    pub fn set_meeting_summary(&mut self, summary: Option<MeetingSummary>) {
        self.meeting_summary = summary;
    }

    /// Returns whether meeting summarization is currently in progress.
    #[allow(dead_code)]
    pub fn is_summarizing(&self) -> bool {
        self.is_summarizing.load(Ordering::SeqCst)
    }

    /// Returns whether multilingual translation is currently in progress.
    #[allow(dead_code)]
    pub fn is_translating(&self) -> bool {
        self.is_translating.load(Ordering::SeqCst)
    }

    /// Returns translated segments if available.
    pub fn translated_segments(&self) -> Option<&[TranscriptionSegment]> {
        self.translated_segments.as_deref()
    }

    /// Returns whether translated subtitles should be displayed.
    pub fn show_translated_subtitles(&self) -> bool {
        self.show_translated_subtitles
    }

    /// Sets whether translated subtitles should be displayed.
    #[allow(dead_code)]
    pub fn set_show_translated_subtitles(&mut self, show: bool) {
        self.show_translated_subtitles = show;
    }

    /// Overrides LLM execution to use the deterministic MockSummarizer (useful for tests/demos).
    #[allow(dead_code)]
    pub fn set_use_mock_llm(&mut self, use_mock: bool) {
        self.use_mock_llm = use_mock;
    }

    /// Initiates asynchronous meeting summarization with Map-Reduce in a background thread (§4.19, §4.20).
    pub fn start_summary(&mut self) {
        if self.is_summarizing.load(Ordering::SeqCst) || self.transcription_segments.is_empty() {
            return;
        }

        self.touch_activity();
        self.is_summarizing.store(true, Ordering::SeqCst);
        self.summarize_abort.store(false, Ordering::SeqCst);
        self.summarize_error = None;
        *self.summarize_progress.lock().unwrap_or_else(|e| e.into_inner()) = None;

        let segments = self.transcription_segments.clone();
        let is_summarizing = Arc::clone(&self.is_summarizing);
        let progress_state = Arc::clone(&self.summarize_progress);
        let abort_flag = Arc::clone(&self.summarize_abort);

        let manager = ModelManager::new();
        let ready = manager.is_llm_model_ready(DEFAULT_QWEN_MODEL_FILENAME, DEFAULT_QWEN_TOKENIZER_FILENAME);
        let use_mock = self.use_mock_llm || !ready;

        let summarizer: Arc<dyn Summarizer> = if !use_mock {
            if let Some((model_p, tok_p)) = manager.get_llm_model_paths(DEFAULT_QWEN_MODEL_FILENAME, DEFAULT_QWEN_TOKENIZER_FILENAME) {
                match LocalQwenSummarizer::load_from_paths(&model_p, &tok_p, DEFAULT_QWEN_CONTEXT_WINDOW, DEFAULT_QWEN_MAX_OUTPUT_TOKENS) {
                    Ok(s) => Arc::new(s),
                    Err(e) => {
                        error!("Falha ao carregar modelo Qwen local: {:?}", e);
                        Arc::new(MockSummarizer::default())
                    }
                }
            } else {
                Arc::new(MockSummarizer::default())
            }
        } else {
            Arc::new(MockSummarizer::default())
        };

        let (tx, rx) = crossbeam_channel::bounded::<Result<MeetingSummary, String>>(1);

        thread::spawn(move || {
            let map_reduce = MapReduceSummarizer::new(summarizer);
            let prog_clone = Arc::clone(&progress_state);
            let progress_cb = move |p: SummarizeProgress| {
                if let Ok(mut g) = prog_clone.lock() {
                    *g = Some(p);
                }
            };

            let res = map_reduce.summarize(&segments, Some(progress_cb), Some(abort_flag));
            let mapped = res.map_err(|e| e.to_string());
            let _ = tx.send(mapped);

            is_summarizing.store(false, Ordering::SeqCst);
            *progress_state.lock().unwrap_or_else(|e| e.into_inner()) = None;
        });

        self.summary_rx = Some(rx);
    }

    /// Signals cancellation of the running summarization operation (§4.20).
    pub fn cancel_summary(&mut self) {
        self.summarize_abort.store(true, Ordering::SeqCst);
    }

    /// Initiates multilingual translation of current transcription segments in a background thread (§4.1).
    pub fn start_translation(&mut self, target_lang: TargetLanguage) {
        if self.is_translating.load(Ordering::SeqCst) || self.transcription_segments.is_empty() {
            return;
        }

        self.touch_activity();
        self.is_translating.store(true, Ordering::SeqCst);
        self.translation_abort.store(false, Ordering::SeqCst);
        self.translation_error = None;
        *self.translation_progress.lock().unwrap_or_else(|e| e.into_inner()) = None;

        let segments = self.transcription_segments.clone();
        let is_translating = Arc::clone(&self.is_translating);
        let progress_state = Arc::clone(&self.translation_progress);
        let abort_flag = Arc::clone(&self.translation_abort);

        let manager = ModelManager::new();
        let ready = manager.is_llm_model_ready(DEFAULT_QWEN_MODEL_FILENAME, DEFAULT_QWEN_TOKENIZER_FILENAME);
        let use_mock = self.use_mock_llm || !ready;

        let summarizer: Arc<dyn Summarizer> = if !use_mock {
            if let Some((model_p, tok_p)) = manager.get_llm_model_paths(DEFAULT_QWEN_MODEL_FILENAME, DEFAULT_QWEN_TOKENIZER_FILENAME) {
                match LocalQwenSummarizer::load_from_paths(&model_p, &tok_p, DEFAULT_QWEN_CONTEXT_WINDOW, DEFAULT_QWEN_MAX_OUTPUT_TOKENS) {
                    Ok(s) => Arc::new(s),
                    Err(_) => Arc::new(MockSummarizer::default()),
                }
            } else {
                Arc::new(MockSummarizer::default())
            }
        } else {
            Arc::new(MockSummarizer::default())
        };

        let (tx, rx) = crossbeam_channel::bounded::<Result<Vec<TranscriptionSegment>, String>>(1);

        thread::spawn(move || {
            let translator = LlmTranslator::new(summarizer);
            let prog_clone = Arc::clone(&progress_state);
            let progress_cb = move |curr: usize, total: usize| {
                if let Ok(mut g) = prog_clone.lock() {
                    *g = Some((curr, total));
                }
            };

            let res = translator.translate_segments(
                &segments,
                "Português",
                target_lang,
                Some(progress_cb),
                Some(abort_flag),
            );

            let mapped = res.map_err(|e| e.to_string());
            let _ = tx.send(mapped);

            is_translating.store(false, Ordering::SeqCst);
            *progress_state.lock().unwrap_or_else(|e| e.into_inner()) = None;
        });

        self.translation_rx = Some(rx);
    }

    /// Signals cancellation of the running translation operation.
    pub fn cancel_translation(&mut self) {
        self.translation_abort.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inactivity_unload_respects_storage_mode_section_4_16() {
        let mut panel = WhisperPanel::new();

        // 1. When no model is loaded, check_inactivity_unload returns None
        assert_eq!(panel.check_inactivity_unload(Duration::from_millis(10)), None);

        // 2. Simulate model loaded in Disk mode
        *panel.active_model_id.lock().unwrap() = Some("tiny".to_string());
        *panel.active_storage_mode.lock().unwrap() = Some(ModelStorageMode::Disk);
        *panel.last_activity.lock().unwrap() = Instant::now() - Duration::from_secs(400);

        // Per §4.16: Disk mode must NOT be unloaded on inactivity!
        let freed_disk = panel.check_inactivity_unload(Duration::from_secs(300));
        assert_eq!(freed_disk, None, "Disk mode model must NOT be unloaded on inactivity (§4.16)");
        assert_eq!(panel.active_model_id().as_deref(), Some("tiny"));

        // 3. Simulate model loaded in RAM-only mode
        *panel.active_storage_mode.lock().unwrap() = Some(ModelStorageMode::RamOnly);
        // Reset activity to now: should NOT unload yet
        *panel.last_activity.lock().unwrap() = Instant::now();
        assert_eq!(panel.check_inactivity_unload(Duration::from_secs(300)), None);
        assert_eq!(panel.active_model_id().as_deref(), Some("tiny"));

        // Advance simulated activity to past 300s
        *panel.last_activity.lock().unwrap() = Instant::now() - Duration::from_secs(301);
        let _freed_ram = panel.check_inactivity_unload(Duration::from_secs(300));
        assert_eq!(
            panel.active_model_id(),
            None,
            "RAM-only model must be unloaded after inactivity timeout (§4.16)"
        );
        assert_eq!(panel.active_storage_mode(), None);
    }

    #[test]
    fn test_explicit_unload_vs_inactivity_triggers() {
        let mut panel = WhisperPanel::new();
        *panel.active_model_id.lock().unwrap() = Some("base-q5".to_string());
        *panel.active_storage_mode.lock().unwrap() = Some(ModelStorageMode::RamOnly);

        // Explicit close
        let _ = panel.unload_model(UnloadTrigger::ExplicitClose);
        assert_eq!(panel.active_model_id(), None);
    }

    /// Idle timers are scheduled only for RAM-only models (§4.16); disk models have none.
    #[test]
    fn test_idle_unload_due_in_only_for_ram_only() {
        let panel = WhisperPanel::new();
        assert_eq!(panel.idle_unload_due_in(IDLE_UNLOAD_TIMEOUT), None);

        *panel.active_storage_mode.lock().unwrap() = Some(ModelStorageMode::Disk);
        assert_eq!(panel.idle_unload_due_in(IDLE_UNLOAD_TIMEOUT), None);

        *panel.active_storage_mode.lock().unwrap() = Some(ModelStorageMode::RamOnly);
        *panel.last_activity.lock().unwrap() = Instant::now() - Duration::from_secs(100);
        let due = panel.idle_unload_due_in(IDLE_UNLOAD_TIMEOUT).unwrap();
        assert!(due > Duration::from_secs(190) && due <= Duration::from_secs(200), "due in {due:?}");

        *panel.last_activity.lock().unwrap() = Instant::now() - Duration::from_secs(900);
        assert_eq!(panel.idle_unload_due_in(IDLE_UNLOAD_TIMEOUT), Some(Duration::ZERO));
    }

    /// The elapsed-clock path with no back-dated `Instant`: the timeout really expires.
    #[test]
    fn test_inactivity_unload_fires_after_real_elapsed_time() {
        let mut panel = WhisperPanel::new();
        *panel.active_model_id.lock().unwrap() = Some("base-q5".to_string());
        *panel.active_storage_mode.lock().unwrap() = Some(ModelStorageMode::RamOnly);
        panel.touch_activity();

        let timeout = Duration::from_millis(600);
        assert_eq!(panel.check_inactivity_unload(timeout), None, "must not fire right after use");
        assert!(panel.active_model_id().is_some());

        thread::sleep(timeout + Duration::from_millis(200));
        let _ = panel.check_inactivity_unload(timeout);
        assert_eq!(panel.active_model_id(), None, "must fire once the timeout really elapsed");
    }

    #[test]
    fn test_reset_for_new_media_drops_previous_transcript() {
        let mut panel = WhisperPanel::new();
        panel.set_segments(vec![TranscriptionSegment { start_ms: 0, end_ms: 1000, text: "old".into() }]);
        panel.transcription_error = Some("old error".into());
        panel.reset_for_new_media();
        assert!(panel.transcription_segments().is_empty());
        assert!(panel.transcription_error.is_none());
    }

    /// §4.16 exit criterion with a REAL engine: load `base-q5` RAM-only, let the idle timeout
    /// expire, and compare the process RSS before/after the unload the panel performs itself.
    /// Needs network (downloads ~55 MB into /tmp); run with
    /// `cargo test -p vad-app -- --ignored --nocapture unload_frees`.
    #[test]
    #[ignore = "needs network access (downloads the base-q5 model)"]
    fn test_inactivity_unload_frees_real_engine_ram() {
        let dir = std::path::PathBuf::from(format!("/tmp/vad_test_unload_{}", std::process::id()));
        let manager = ModelManager::with_dir(dir.join("models"));
        let preset = find_preset("base-q5").unwrap();

        let rss_start = get_process_rss_bytes().unwrap();
        let source = manager
            .load_or_download_model(preset, ModelStorageMode::RamOnly, |_| {})
            .expect("model download");
        let engine = WhisperEngine::load(&source).expect("engine load");
        drop(source); // only the engine may keep the model alive from here on
        let rss_loaded = get_process_rss_bytes().unwrap();

        let mut panel = WhisperPanel::new();
        *panel.active_engine.lock().unwrap() = Some(Arc::new(engine));
        *panel.active_model_id.lock().unwrap() = Some(preset.id.to_string());
        *panel.active_storage_mode.lock().unwrap() = Some(ModelStorageMode::RamOnly);

        // Not yet idle: nothing may happen
        assert_eq!(panel.check_inactivity_unload(IDLE_UNLOAD_TIMEOUT), None);
        assert!(panel.has_active_model());

        *panel.last_activity.lock().unwrap() = Instant::now() - IDLE_UNLOAD_TIMEOUT - Duration::from_secs(1);
        let freed_reported = panel.check_inactivity_unload(IDLE_UNLOAD_TIMEOUT);
        let rss_after = get_process_rss_bytes().unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        println!(
            "UNLOAD RSS: start={rss_start} B, model loaded={rss_loaded} B (+{} B), after inactivity unload={rss_after} B, freed={} B, panel reported {freed_reported:?}",
            rss_loaded.saturating_sub(rss_start),
            rss_loaded.saturating_sub(rss_after),
        );
        assert!(!panel.has_active_model(), "engine must be dropped by the inactivity unload");
        let grew = rss_loaded.saturating_sub(rss_start);
        let freed = rss_loaded.saturating_sub(rss_after);
        assert!(grew > 0 && freed * 2 >= grew, "unload must return at least half of what loading took");
    }

    #[test]
    fn test_whisper_panel_start_summary_async() {
        let mut panel = WhisperPanel::new();
        panel.set_use_mock_llm(true);
        panel.set_segments(vec![
            TranscriptionSegment {
                start_ms: 0,
                end_ms: 3000,
                text: "Apresentação dos resultados trimestrais.".to_string(),
            },
            TranscriptionSegment {
                start_ms: 3000,
                end_ms: 6000,
                text: "O plano de migração foi aprovado por unanimidade.".to_string(),
            },
        ]);

        assert!(!panel.is_summarizing());
        panel.start_summary();
        assert!(panel.is_summarizing());

        // Wait for background worker to deliver result
        let start = Instant::now();
        while panel.is_summarizing() && start.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(10));
        }

        assert!(!panel.is_summarizing());
        let summary_rx = panel.summary_rx.take().expect("summary_rx must be present");
        let res = summary_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        assert!(res.is_ok());
        let summary = res.unwrap();
        assert_eq!(summary.privacy_badge, AiPrivacyBadge::Local);
        assert!(summary.markdown.contains("Resumo da Reunião"));
    }

    #[test]
    fn test_whisper_panel_start_translation_async() {
        let mut panel = WhisperPanel::new();
        panel.set_use_mock_llm(true);
        panel.set_segments(vec![TranscriptionSegment {
            start_ms: 0,
            end_ms: 2500,
            text: "Bom dia a todos.".to_string(),
        }]);

        assert!(!panel.is_translating());
        panel.start_translation(TargetLanguage::English);
        assert!(panel.is_translating());

        let start = Instant::now();
        while panel.is_translating() && start.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(10));
        }

        assert!(!panel.is_translating());
        let translation_rx = panel.translation_rx.take().expect("translation_rx must be present");
        let res = translation_rx.recv_timeout(Duration::from_millis(500)).unwrap();
        assert!(res.is_ok());
        let translated = res.unwrap();
        assert_eq!(translated.len(), 1);
        assert_eq!(translated[0].start_ms, 0);
        assert_eq!(translated[0].end_ms, 2500);
    }
}
