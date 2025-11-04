use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Text-to-Speech voice gender
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceGender {
    Male,
    Female,
    Neutral,
}

/// Text-to-Speech voice characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub gender: VoiceGender,
    pub language: String, // e.g., "en-US", "es-ES"
    pub sample_rate: u32, // Hz
}

impl Voice {
    /// Create a new voice
    pub fn new(
        id: String,
        name: String,
        gender: VoiceGender,
        language: String,
        sample_rate: u32,
    ) -> Self {
        Self {
            id,
            name,
            gender,
            language,
            sample_rate,
        }
    }
}

/// Speech rate multiplier (0.5x - 2.0x normal speed)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpeechRate(f32);

impl SpeechRate {
    /// Create a new speech rate (clamped to 0.5 - 2.0)
    pub fn new(rate: f32) -> Self {
        Self(rate.clamp(0.5, 2.0))
    }

    /// Get the rate value
    pub fn value(&self) -> f32 {
        self.0
    }

    /// Normal speech rate (1.0x)
    pub fn normal() -> Self {
        Self(1.0)
    }

    /// Slow speech rate (0.75x)
    pub fn slow() -> Self {
        Self(0.75)
    }

    /// Fast speech rate (1.5x)
    pub fn fast() -> Self {
        Self(1.5)
    }
}

impl Default for SpeechRate {
    fn default() -> Self {
        Self::normal()
    }
}

/// Speech priority level (higher priority speaks first)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SpeechPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Urgent = 3,
}

/// SSML (Speech Synthesis Markup Language) element
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsmlElement {
    /// Plain text
    Text(String),
    /// Pause/break
    Break { duration_ms: u32 },
    /// Emphasis
    Emphasis { level: EmphasisLevel, text: String },
    /// Prosody (rate, pitch, volume)
    Prosody {
        rate: Option<String>,
        pitch: Option<String>,
        volume: Option<String>,
        text: String,
    },
    /// Say-as (interpret text in specific way)
    SayAs { interpret_as: String, text: String },
}

/// Emphasis level for SSML
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmphasisLevel {
    None,
    Reduced,
    Moderate,
    Strong,
}

/// Text-to-Speech utterance to be spoken
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utterance {
    pub id: Uuid,
    pub text: String,
    pub ssml: Vec<SsmlElement>,
    pub voice_id: Option<String>,
    pub rate: SpeechRate,
    pub priority: SpeechPriority,
    pub interruptible: bool,
}

impl Utterance {
    /// Create a simple text utterance
    pub fn new(text: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            text,
            ssml: Vec::new(),
            voice_id: None,
            rate: SpeechRate::normal(),
            priority: SpeechPriority::Normal,
            interruptible: true,
        }
    }

    /// Create an utterance with SSML markup
    pub fn with_ssml(text: String, ssml: Vec<SsmlElement>) -> Self {
        Self {
            id: Uuid::new_v4(),
            text,
            ssml,
            voice_id: None,
            rate: SpeechRate::normal(),
            priority: SpeechPriority::Normal,
            interruptible: true,
        }
    }

    /// Set the voice for this utterance
    pub fn with_voice(mut self, voice_id: String) -> Self {
        self.voice_id = Some(voice_id);
        self
    }

    /// Set the speech rate
    pub fn with_rate(mut self, rate: SpeechRate) -> Self {
        self.rate = rate;
        self
    }

    /// Set the priority
    pub fn with_priority(mut self, priority: SpeechPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set whether this utterance can be interrupted
    pub fn interruptible(mut self, interruptible: bool) -> Self {
        self.interruptible = interruptible;
        self
    }

    /// Convert to SSML string
    pub fn to_ssml(&self) -> String {
        if self.ssml.is_empty() {
            return self.text.clone();
        }

        let mut ssml = String::from("<speak>");

        for element in &self.ssml {
            match element {
                SsmlElement::Text(text) => ssml.push_str(text),
                SsmlElement::Break { duration_ms } => {
                    ssml.push_str(&format!("<break time=\"{}ms\"/>", duration_ms));
                }
                SsmlElement::Emphasis { level, text } => {
                    let level_str = match level {
                        EmphasisLevel::None => "none",
                        EmphasisLevel::Reduced => "reduced",
                        EmphasisLevel::Moderate => "moderate",
                        EmphasisLevel::Strong => "strong",
                    };
                    ssml.push_str(&format!(
                        "<emphasis level=\"{}\">{}</emphasis>",
                        level_str, text
                    ));
                }
                SsmlElement::Prosody {
                    rate,
                    pitch,
                    volume,
                    text,
                } => {
                    ssml.push_str("<prosody");
                    if let Some(r) = rate {
                        ssml.push_str(&format!(" rate=\"{}\"", r));
                    }
                    if let Some(p) = pitch {
                        ssml.push_str(&format!(" pitch=\"{}\"", p));
                    }
                    if let Some(v) = volume {
                        ssml.push_str(&format!(" volume=\"{}\"", v));
                    }
                    ssml.push_str(&format!(">{}</prosody>", text));
                }
                SsmlElement::SayAs { interpret_as, text } => {
                    ssml.push_str(&format!(
                        "<say-as interpret-as=\"{}\">{}</say-as>",
                        interpret_as, text
                    ));
                }
            }
        }

        ssml.push_str("</speak>");
        ssml
    }
}

/// Text-to-Speech engine state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TtsState {
    Idle,
    Speaking,
    Paused,
}

/// Text-to-Speech engine
pub struct TtsEngine {
    voices: Arc<RwLock<HashMap<String, Voice>>>,
    queue: Arc<RwLock<VecDeque<Utterance>>>,
    current: Arc<RwLock<Option<Utterance>>>,
    state: Arc<RwLock<TtsState>>,
    default_voice_id: Arc<RwLock<Option<String>>>,
    enabled: Arc<RwLock<bool>>,
}

impl TtsEngine {
    /// Create a new TTS engine
    pub fn new() -> Self {
        Self {
            voices: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(RwLock::new(VecDeque::new())),
            current: Arc::new(RwLock::new(None)),
            state: Arc::new(RwLock::new(TtsState::Idle)),
            default_voice_id: Arc::new(RwLock::new(None)),
            enabled: Arc::new(RwLock::new(true)),
        }
    }

    /// Register a voice
    pub fn register_voice(&self, voice: Voice) {
        let mut voices = self.voices.write().unwrap();
        voices.insert(voice.id.clone(), voice);
    }

    /// Get available voices
    pub fn get_voices(&self) -> Vec<Voice> {
        let voices = self.voices.read().unwrap();
        voices.values().cloned().collect()
    }

    /// Get voices by gender
    pub fn get_voices_by_gender(&self, gender: &VoiceGender) -> Vec<Voice> {
        let voices = self.voices.read().unwrap();
        voices
            .values()
            .filter(|v| &v.gender == gender)
            .cloned()
            .collect()
    }

    /// Get voices by language
    pub fn get_voices_by_language(&self, language: &str) -> Vec<Voice> {
        let voices = self.voices.read().unwrap();
        voices
            .values()
            .filter(|v| v.language == language)
            .cloned()
            .collect()
    }

    /// Set the default voice
    pub fn set_default_voice(&self, voice_id: String) {
        let mut default = self.default_voice_id.write().unwrap();
        *default = Some(voice_id);
    }

    /// Get the default voice ID
    pub fn get_default_voice(&self) -> Option<String> {
        let default = self.default_voice_id.read().unwrap();
        default.clone()
    }

    /// Enable or disable TTS
    pub fn set_enabled(&self, enabled: bool) {
        let mut e = self.enabled.write().unwrap();
        *e = enabled;
    }

    /// Check if TTS is enabled
    pub fn is_enabled(&self) -> bool {
        let enabled = self.enabled.read().unwrap();
        *enabled
    }

    /// Speak an utterance (adds to queue)
    pub fn speak(&self, utterance: Utterance) {
        if !self.is_enabled() {
            return;
        }

        let mut queue = self.queue.write().unwrap();

        // Insert based on priority
        let insert_pos = queue
            .iter()
            .position(|u| u.priority < utterance.priority)
            .unwrap_or(queue.len());

        queue.insert(insert_pos, utterance);
    }

    /// Speak text with default settings
    pub fn speak_text(&self, text: String) {
        self.speak(Utterance::new(text));
    }

    /// Speak urgent message (interrupts current if interruptible)
    pub fn speak_urgent(&self, text: String) {
        if !self.is_enabled() {
            return;
        }

        let utterance = Utterance::new(text)
            .with_priority(SpeechPriority::Urgent)
            .interruptible(false);

        // Check if we should interrupt current
        let current = self.current.read().unwrap();
        if let Some(ref curr) = *current {
            if curr.interruptible {
                drop(current);
                self.stop();
            }
        }

        self.speak(utterance);
    }

    /// Get the current utterance being spoken
    pub fn get_current(&self) -> Option<Utterance> {
        let current = self.current.read().unwrap();
        current.clone()
    }

    /// Get the queue size
    pub fn queue_size(&self) -> usize {
        let queue = self.queue.read().unwrap();
        queue.len()
    }

    /// Get the engine state
    pub fn get_state(&self) -> TtsState {
        let state = self.state.read().unwrap();
        state.clone()
    }

    /// Start speaking the next utterance in queue
    pub fn start_next(&self) -> bool {
        if !self.is_enabled() {
            return false;
        }

        let mut queue = self.queue.write().unwrap();
        if let Some(utterance) = queue.pop_front() {
            let mut current = self.current.write().unwrap();
            *current = Some(utterance);

            let mut state = self.state.write().unwrap();
            *state = TtsState::Speaking;
            true
        } else {
            false
        }
    }

    /// Pause speaking
    pub fn pause(&self) {
        let mut state = self.state.write().unwrap();
        if *state == TtsState::Speaking {
            *state = TtsState::Paused;
        }
    }

    /// Resume speaking
    pub fn resume(&self) {
        let mut state = self.state.write().unwrap();
        if *state == TtsState::Paused {
            *state = TtsState::Speaking;
        }
    }

    /// Stop speaking and clear current
    pub fn stop(&self) {
        let mut current = self.current.write().unwrap();
        *current = None;

        let mut state = self.state.write().unwrap();
        *state = TtsState::Idle;
    }

    /// Clear the queue
    pub fn clear_queue(&self) {
        let mut queue = self.queue.write().unwrap();
        queue.clear();
    }

    /// Mark current utterance as finished
    pub fn finish_current(&self) {
        let mut current = self.current.write().unwrap();
        *current = None;

        let mut state = self.state.write().unwrap();
        *state = TtsState::Idle;

        // Auto-start next if available
        drop(current);
        drop(state);
        self.start_next();
    }
}

impl Default for TtsEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_creation() {
        let voice = Voice::new(
            "en-us-male-1".to_string(),
            "English Male".to_string(),
            VoiceGender::Male,
            "en-US".to_string(),
            22050,
        );

        assert_eq!(voice.id, "en-us-male-1");
        assert_eq!(voice.gender, VoiceGender::Male);
        assert_eq!(voice.language, "en-US");
    }

    #[test]
    fn test_speech_rate() {
        let normal = SpeechRate::normal();
        assert_eq!(normal.value(), 1.0);

        let fast = SpeechRate::fast();
        assert_eq!(fast.value(), 1.5);

        // Test clamping
        let too_fast = SpeechRate::new(5.0);
        assert_eq!(too_fast.value(), 2.0);

        let too_slow = SpeechRate::new(0.1);
        assert_eq!(too_slow.value(), 0.5);
    }

    #[test]
    fn test_utterance_creation() {
        let utterance = Utterance::new("Hello world".to_string());
        assert_eq!(utterance.text, "Hello world");
        assert_eq!(utterance.priority, SpeechPriority::Normal);
        assert!(utterance.interruptible);
    }

    #[test]
    fn test_utterance_builder() {
        let utterance = Utterance::new("Test".to_string())
            .with_voice("voice-1".to_string())
            .with_rate(SpeechRate::fast())
            .with_priority(SpeechPriority::High)
            .interruptible(false);

        assert_eq!(utterance.voice_id, Some("voice-1".to_string()));
        assert_eq!(utterance.rate.value(), 1.5);
        assert_eq!(utterance.priority, SpeechPriority::High);
        assert!(!utterance.interruptible);
    }

    #[test]
    fn test_ssml_generation() {
        let ssml = vec![
            SsmlElement::Text("Hello".to_string()),
            SsmlElement::Break { duration_ms: 500 },
            SsmlElement::Emphasis {
                level: EmphasisLevel::Strong,
                text: "world".to_string(),
            },
        ];

        let utterance = Utterance::with_ssml("Hello world".to_string(), ssml);
        let ssml_string = utterance.to_ssml();

        assert!(ssml_string.contains("<speak>"));
        assert!(ssml_string.contains("Hello"));
        assert!(ssml_string.contains("<break time=\"500ms\"/>"));
        assert!(ssml_string.contains("<emphasis level=\"strong\">world</emphasis>"));
        assert!(ssml_string.contains("</speak>"));
    }

    #[test]
    fn test_tts_engine_register_voice() {
        let engine = TtsEngine::new();
        let voice = Voice::new(
            "test-voice".to_string(),
            "Test".to_string(),
            VoiceGender::Neutral,
            "en-US".to_string(),
            16000,
        );

        engine.register_voice(voice);

        let voices = engine.get_voices();
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].id, "test-voice");
    }

    #[test]
    fn test_get_voices_by_gender() {
        let engine = TtsEngine::new();

        engine.register_voice(Voice::new(
            "male-1".to_string(),
            "Male 1".to_string(),
            VoiceGender::Male,
            "en-US".to_string(),
            22050,
        ));

        engine.register_voice(Voice::new(
            "female-1".to_string(),
            "Female 1".to_string(),
            VoiceGender::Female,
            "en-US".to_string(),
            22050,
        ));

        let male_voices = engine.get_voices_by_gender(&VoiceGender::Male);
        assert_eq!(male_voices.len(), 1);
        assert_eq!(male_voices[0].id, "male-1");
    }

    #[test]
    fn test_speak_queuing() {
        let engine = TtsEngine::new();

        engine.speak_text("First".to_string());
        engine.speak_text("Second".to_string());

        assert_eq!(engine.queue_size(), 2);
    }

    #[test]
    fn test_priority_queuing() {
        let engine = TtsEngine::new();

        engine.speak(Utterance::new("Low".to_string()).with_priority(SpeechPriority::Low));
        engine.speak(Utterance::new("High".to_string()).with_priority(SpeechPriority::High));
        engine.speak(Utterance::new("Normal".to_string()).with_priority(SpeechPriority::Normal));

        // Start first (should be High priority)
        engine.start_next();
        let current = engine.get_current().unwrap();
        assert_eq!(current.text, "High");

        engine.finish_current();
        let current = engine.get_current().unwrap();
        assert_eq!(current.text, "Normal");
    }

    #[test]
    fn test_urgent_interrupt() {
        let engine = TtsEngine::new();

        // Start speaking something interruptible
        engine.speak(Utterance::new("Regular message".to_string()));
        engine.start_next();

        assert_eq!(engine.get_state(), TtsState::Speaking);

        // Speak urgent (should interrupt)
        engine.speak_urgent("URGENT!".to_string());

        // Current should be cleared
        assert_eq!(engine.get_state(), TtsState::Idle);

        // Queue should have urgent message
        assert_eq!(engine.queue_size(), 1);
    }

    #[test]
    fn test_pause_resume() {
        let engine = TtsEngine::new();

        engine.speak_text("Test".to_string());
        engine.start_next();

        assert_eq!(engine.get_state(), TtsState::Speaking);

        engine.pause();
        assert_eq!(engine.get_state(), TtsState::Paused);

        engine.resume();
        assert_eq!(engine.get_state(), TtsState::Speaking);
    }

    #[test]
    fn test_stop() {
        let engine = TtsEngine::new();

        engine.speak_text("Test".to_string());
        engine.start_next();

        engine.stop();

        assert_eq!(engine.get_state(), TtsState::Idle);
        assert!(engine.get_current().is_none());
    }

    #[test]
    fn test_enable_disable() {
        let engine = TtsEngine::new();

        assert!(engine.is_enabled());

        engine.set_enabled(false);
        assert!(!engine.is_enabled());

        // Speaking when disabled should do nothing
        engine.speak_text("Test".to_string());
        assert_eq!(engine.queue_size(), 0);
    }

    #[test]
    fn test_default_voice() {
        let engine = TtsEngine::new();

        assert!(engine.get_default_voice().is_none());

        engine.set_default_voice("voice-1".to_string());
        assert_eq!(engine.get_default_voice(), Some("voice-1".to_string()));
    }
}
