//! 音频解码器
//!
//! 支持的格式：
//!   - WAV  (.wav)       → symphonia
//!   - OGG  (.ogg)       → symphonia
//!   - MP3  (.mp3)       → symphonia
//!   - FLAC (.flac)      → symphonia

mod symphonia;

pub use symphonia::Decoder as SymphoniaDecoder;
