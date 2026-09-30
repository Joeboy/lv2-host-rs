//! Configuration and shared LV2 features for a group of processing instances.

use std::{ffi::CStr, sync::Arc};

use crate::{AtomEvent, Error, error::Result};

/// Buffer limits and sample rate used by every instance in a context.
#[derive(Clone, Copy, Debug)]
pub struct ProcessingConfig {
    pub sample_rate: f64,
    pub block_capacity: usize,
    pub atom_sequence_capacity: usize,
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            block_capacity: 256,
            atom_sequence_capacity: 4096,
        }
    }
}

/// Shared configuration and URID mapping for instances that exchange events.
///
/// Cloning a context preserves its URID mapping. Create it before starting
/// processing and reuse it when adding plugins to a running graph.
#[derive(Clone)]
pub struct ProcessingContext {
    pub(crate) features: Arc<livi::Features>,
    config: ProcessingConfig,
}

impl ProcessingContext {
    pub(crate) fn new(world: &livi::World, config: ProcessingConfig) -> Result<Self> {
        if !config.sample_rate.is_finite() || config.sample_rate <= 0.0 {
            return Err(Error::InvalidSampleRate {
                sample_rate: config.sample_rate,
            });
        }
        if config.block_capacity == 0 {
            return Err(Error::InvalidConfiguration {
                name: "LV2 block capacity",
            });
        }
        if config.atom_sequence_capacity == 0 {
            return Err(Error::InvalidConfiguration {
                name: "LV2 Atom sequence capacity",
            });
        }
        Ok(Self {
            features: world.build_features(livi::FeaturesBuilder {
                min_block_length: 1,
                max_block_length: config.block_capacity,
            }),
            config,
        })
    }

    pub fn config(&self) -> &ProcessingConfig {
        &self.config
    }

    /// Maps an LV2 URI for constructing Atom events, such as MIDI messages.
    /// Resolve these IDs during setup, outside the audio callback.
    pub fn urid(&self, uri: &CStr) -> u32 {
        self.features.urid(uri)
    }

    /// Encodes a path property using this context's URID mapping.
    pub fn patch_set_path_event(&self, property: &str, path: &str) -> Result<AtomEvent> {
        crate::instance::patch_set_path_event(&self.features, property, path)
    }
}
