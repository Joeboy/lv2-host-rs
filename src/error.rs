//! Errors produced by LV2 discovery, instantiation, and processing.

/// An error from the core LV2 hosting API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("sample rate must be finite and positive, got {sample_rate}")]
    InvalidSampleRate { sample_rate: f64 },

    #[error("Atom routing requires instances from the same processing context")]
    IncompatibleContexts,
    #[error("plugin is unavailable: {uri}")]
    PluginUnavailable { uri: String },

    #[error("{name} must be greater than zero")]
    InvalidConfiguration { name: &'static str },

    #[error("could not instantiate {plugin}: {source}")]
    Instantiation {
        plugin: String,
        #[source]
        source: livi::error::InstantiateError,
    },

    #[error("port {index} is not a control input")]
    InvalidControlInput { index: u32 },

    #[error("control input port {symbol:?} is unavailable")]
    ControlInputUnavailable { symbol: String },

    #[error("port {index} is not a control output")]
    InvalidControlOutput { index: u32 },

    #[error("control output port {symbol:?} is unavailable")]
    ControlOutputUnavailable { symbol: String },

    #[error("Atom input slot {slot} does not exist")]
    InvalidAtomInput { slot: usize },

    #[error("Atom output slot {slot} does not exist")]
    InvalidAtomOutput { slot: usize },

    #[error("could not write to Atom input slot {slot}: {source}")]
    AtomEvent {
        slot: usize,
        #[source]
        source: livi::error::EventError,
    },

    #[error("invalid LV2 block size {frames}; expected 1..={capacity}")]
    InvalidBlockSize { frames: usize, capacity: usize },

    #[error("LV2 processing failed: {source}")]
    Processing {
        #[source]
        source: livi::error::RunError,
    },

    #[error("{field} contains a NUL byte")]
    InvalidPatchValue { field: &'static str },
}

pub(crate) type Result<T> = std::result::Result<T, Error>;
