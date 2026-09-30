//! Native LV2 instance ownership and preallocated port buffers.

use livi::{
    PortIndex,
    event::{LV2AtomEventBuilder, LV2AtomSequence},
};
use std::{
    cell::UnsafeCell,
    collections::BTreeMap,
    ffi::{CStr, CString},
    fmt,
    sync::Arc,
};

use crate::{Error, PluginDescriptor, PortKind, ProcessingContext, UiDescriptor, error::Result};

const MAX_ATOM_EVENT_BYTES: usize = 4096;

struct InstanceBuffers {
    audio_inputs: Vec<Vec<f32>>,
    audio_outputs: Vec<Vec<f32>>,
    cv_inputs: Vec<Vec<f32>>,
    cv_outputs: Vec<Vec<f32>>,
    atom_inputs: Vec<LV2AtomSequence>,
    atom_outputs: Vec<LV2AtomSequence>,
}

#[derive(Clone, Debug)]
pub struct AtomEvent {
    pub time_in_frames: i64,
    pub type_urid: u32,
    pub data: Vec<u8>,
}

pub(crate) struct NativeInstance {
    instance: UnsafeCell<livi::Instance>,
    buffers: UnsafeCell<InstanceBuffers>,
    block_capacity: usize,
    // `livi::Instance` does not retain the Lilv world it was instantiated
    // from. A cloned plugin does, so keep it alive until after `instance` is
    // dropped. Field declaration order is significant here.
    _plugin: livi::Plugin,
}

// `livi::Instance` is Send + Sync. Processing remains exclusively owned by
// `HostedInstance`; `UiInstance` only retains the allocation and exposes its
// native LV2 handle to the UI implementation inside this crate.
unsafe impl Send for NativeInstance {}
unsafe impl Sync for NativeInstance {}

/// An opaque reference used to open a UI for a live plugin instance.
///
/// Clones retain the native plugin instance, so an open UI remains valid if
/// the processing graph retires its [`HostedInstance`].
#[derive(Clone)]
pub struct UiInstance {
    pub(crate) native: Arc<NativeInstance>,
    pub(crate) plugin_uri: String,
    pub(crate) ports: BTreeMap<u32, String>,
    pub(crate) controls: Vec<(u32, f32)>,
    pub(crate) uis: Vec<UiDescriptor>,
}

impl UiInstance {
    pub(crate) fn handle(&self) -> usize {
        unsafe { (&*self.native.instance.get()).raw().instance().handle() as usize }
    }
}

impl PartialEq for UiInstance {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.native, &other.native)
    }
}

impl Eq for UiInstance {}

impl fmt::Debug for UiInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiInstance")
            .field("plugin_uri", &self.plugin_uri)
            .finish_non_exhaustive()
    }
}

pub struct HostedInstance {
    native: Arc<NativeInstance>,
    descriptor: Arc<PluginDescriptor>,
    context: ProcessingContext,
}

impl HostedInstance {
    /// Instantiates a plugin and allocates all non-control port buffers.
    pub(crate) fn instantiate(
        plugin: livi::Plugin,
        descriptor: Arc<PluginDescriptor>,
        context: ProcessingContext,
    ) -> Result<Self> {
        let config = *context.config();
        let features = &context.features;
        // The plugin guard stored below retains the Lilv world for the full
        // native instance lifetime. Calls into third-party plugin code remain
        // contained behind this crate's safe hosting API.
        let instance = unsafe { plugin.instantiate(features.clone(), config.sample_rate) }
            .map_err(|source| Error::Instantiation {
                plugin: plugin.name(),
                source,
            })?;
        let counts = *plugin.port_counts();
        let buffers = |count| {
            (0..count)
                .map(|_| vec![0.; config.block_capacity])
                .collect()
        };
        let atoms = |count| {
            (0..count)
                .map(|_| LV2AtomSequence::new(features, config.atom_sequence_capacity))
                .collect()
        };
        let buffers = InstanceBuffers {
            audio_inputs: buffers(counts.audio_inputs),
            audio_outputs: buffers(counts.audio_outputs),
            cv_inputs: buffers(counts.cv_inputs),
            cv_outputs: buffers(counts.cv_outputs),
            atom_inputs: atoms(counts.atom_sequence_inputs),
            atom_outputs: atoms(counts.atom_sequence_outputs),
        };
        Ok(Self {
            native: Arc::new(NativeInstance {
                instance: UnsafeCell::new(instance),
                buffers: UnsafeCell::new(buffers),
                block_capacity: config.block_capacity,
                _plugin: plugin,
            }),
            descriptor,
            context,
        })
    }

    fn buffers(&self) -> &InstanceBuffers {
        unsafe { &*self.native.buffers.get() }
    }

    fn buffers_mut(&mut self) -> &mut InstanceBuffers {
        unsafe { &mut *self.native.buffers.get() }
    }

    /// Returns the metadata for this plugin instance.
    pub fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    pub fn context(&self) -> &ProcessingContext {
        &self.context
    }

    /// Writable input storage, indexed by audio-input slot (not LV2 port index).
    /// The slice spans block capacity; fill the prefix passed to `run()`.
    pub fn audio_input_mut(&mut self, slot: usize) -> Option<&mut [f32]> {
        self.buffers_mut()
            .audio_inputs
            .get_mut(slot)
            .map(Vec::as_mut_slice)
    }

    /// Output storage; only the prefix written by the last `run()` is current.
    pub fn audio_output(&self, slot: usize) -> Option<&[f32]> {
        self.buffers().audio_outputs.get(slot).map(Vec::as_slice)
    }

    /// Writable CV storage, spanning the configured block capacity.
    pub fn cv_input_mut(&mut self, slot: usize) -> Option<&mut [f32]> {
        self.buffers_mut()
            .cv_inputs
            .get_mut(slot)
            .map(Vec::as_mut_slice)
    }

    /// CV output storage; only the prefix written by the last `run()` is current.
    pub fn cv_output(&self, slot: usize) -> Option<&[f32]> {
        self.buffers().cv_outputs.get(slot).map(Vec::as_slice)
    }

    pub fn atom_input_count(&self) -> usize {
        self.buffers().atom_inputs.len()
    }

    /// Reads an output sequence without allowing its storage to be replaced.
    pub fn atom_output(&self, slot: usize) -> Option<&LV2AtomSequence> {
        self.buffers().atom_outputs.get(slot)
    }

    /// Returns the type-local buffer slot for an audio input port symbol.
    pub fn audio_input_slot(&self, symbol: &str) -> Option<usize> {
        buffer_slot(&self.descriptor, symbol, PortKind::Audio, true)
    }

    /// Returns the type-local buffer slot for an audio output port symbol.
    pub fn audio_output_slot(&self, symbol: &str) -> Option<usize> {
        buffer_slot(&self.descriptor, symbol, PortKind::Audio, false)
    }

    /// Returns a mutable audio input buffer by port symbol.
    pub fn audio_input_buffer_mut(&mut self, symbol: &str) -> Option<&mut [f32]> {
        let slot = self.audio_input_slot(symbol)?;
        self.audio_input_mut(slot)
    }

    /// Returns an audio output buffer by port symbol.
    pub fn audio_output_buffer(&self, symbol: &str) -> Option<&[f32]> {
        let slot = self.audio_output_slot(symbol)?;
        self.audio_output(slot)
    }

    /// Returns the type-local buffer slot for a CV input port symbol.
    pub fn cv_input_slot(&self, symbol: &str) -> Option<usize> {
        buffer_slot(&self.descriptor, symbol, PortKind::Cv, true)
    }

    /// Returns the type-local buffer slot for a CV output port symbol.
    pub fn cv_output_slot(&self, symbol: &str) -> Option<usize> {
        buffer_slot(&self.descriptor, symbol, PortKind::Cv, false)
    }

    /// Returns a mutable CV input buffer by port symbol.
    pub fn cv_input_buffer_mut(&mut self, symbol: &str) -> Option<&mut [f32]> {
        let slot = self.cv_input_slot(symbol)?;
        self.cv_input_mut(slot)
    }

    /// Returns a CV output buffer by port symbol.
    pub fn cv_output_buffer(&self, symbol: &str) -> Option<&[f32]> {
        let slot = self.cv_output_slot(symbol)?;
        self.cv_output(slot)
    }

    /// Returns the type-local buffer slot for an Atom Sequence input symbol.
    pub fn atom_input_slot(&self, symbol: &str) -> Option<usize> {
        buffer_slot(&self.descriptor, symbol, PortKind::AtomSequence, true)
    }

    /// Returns the type-local buffer slot for an Atom Sequence output symbol.
    pub fn atom_output_slot(&self, symbol: &str) -> Option<usize> {
        buffer_slot(&self.descriptor, symbol, PortKind::AtomSequence, false)
    }

    /// Returns an opaque, owned reference suitable for [`crate::UiHost::open`].
    pub fn ui_instance(&self) -> UiInstance {
        let instance = unsafe { &*self.native.instance.get() };
        UiInstance {
            native: self.native.clone(),
            plugin_uri: self.descriptor.uri.clone(),
            ports: self
                .descriptor
                .ports
                .iter()
                .map(|port| (port.index, port.symbol.clone()))
                .collect(),
            controls: self
                .descriptor
                .ports
                .iter()
                .filter(|port| port.kind == PortKind::Control)
                .filter_map(|port| {
                    let index = PortIndex(port.index as usize);
                    let value = if port.input {
                        instance.control_input(index)
                    } else {
                        instance.control_output(index)
                    };
                    value.map(|value| (port.index, value))
                })
                .collect(),
            uis: self.descriptor.uis.clone(),
        }
    }

    pub fn set_control_input(&mut self, index: u32, value: f32) -> Result<()> {
        self.instance_mut()
            .set_control_input(PortIndex(index as usize), value)
            .map(|_| ())
            .ok_or(Error::InvalidControlInput { index })
    }

    /// Sets a control input by its LV2 port symbol.
    pub fn set_control_input_by_symbol(&mut self, symbol: &str, value: f32) -> Result<()> {
        let index =
            port_index(&self.descriptor, symbol, PortKind::Control, true).ok_or_else(|| {
                Error::ControlInputUnavailable {
                    symbol: symbol.to_owned(),
                }
            })?;
        self.set_control_input(index, value)
    }

    pub fn control_output(&self, index: u32) -> Result<f32> {
        self.instance()
            .control_output(PortIndex(index as usize))
            .ok_or(Error::InvalidControlOutput { index })
    }

    /// Reads a control output by its LV2 port symbol.
    pub fn control_output_by_symbol(&self, symbol: &str) -> Result<f32> {
        let index =
            port_index(&self.descriptor, symbol, PortKind::Control, false).ok_or_else(|| {
                Error::ControlOutputUnavailable {
                    symbol: symbol.to_owned(),
                }
            })?;
        self.control_output(index)
    }

    pub fn clear_inputs(&mut self) {
        let buffers = self.buffers_mut();
        for buffer in &mut buffers.audio_inputs {
            buffer.fill(0.);
        }
        for buffer in &mut buffers.cv_inputs {
            buffer.fill(0.);
        }
        for sequence in &mut buffers.atom_inputs {
            sequence.clear();
        }
    }

    pub fn push_atom_input(&mut self, slot: usize, event: &AtomEvent) -> Result<()> {
        self.push_atom_input_parts(slot, event.time_in_frames, event.type_urid, &event.data)
    }

    fn push_atom_input_parts(
        &mut self,
        slot: usize,
        time_in_frames: i64,
        type_urid: u32,
        data: &[u8],
    ) -> Result<()> {
        let event =
            LV2AtomEventBuilder::<MAX_ATOM_EVENT_BYTES>::new(time_in_frames, type_urid, data)
                .map_err(|source| Error::AtomEvent { slot, source })?;
        self.buffers_mut()
            .atom_inputs
            .get_mut(slot)
            .ok_or(Error::InvalidAtomInput { slot })?
            .push_event(&event)
            .map_err(|source| Error::AtomEvent { slot, source })
    }

    pub fn copy_atom_output_to(
        &self,
        output_slot: usize,
        target: &mut HostedInstance,
        input_slot: usize,
    ) -> Result<()> {
        if !Arc::ptr_eq(&self.context.features, &target.context.features) {
            return Err(Error::IncompatibleContexts);
        }
        if input_slot >= target.atom_input_count() {
            return Err(Error::InvalidAtomInput { slot: input_slot });
        }
        let output = self
            .buffers()
            .atom_outputs
            .get(output_slot)
            .ok_or(Error::InvalidAtomOutput { slot: output_slot })?;
        for event in output.iter() {
            target.push_atom_input_parts(
                input_slot,
                event.event.time_in_frames,
                event.event.body.mytype,
                event.data,
            )?;
        }
        Ok(())
    }

    pub fn run(&mut self, frames: usize) -> Result<()> {
        if frames == 0 || frames > self.native.block_capacity {
            return Err(Error::InvalidBlockSize {
                frames,
                capacity: self.native.block_capacity,
            });
        }
        let instance = self.native.instance.get();
        let buffers = unsafe { &mut *self.native.buffers.get() };
        let ports = livi::EmptyPortConnections::new()
            .with_audio_inputs(buffers.audio_inputs.iter().map(|buffer| &buffer[..frames]))
            .with_audio_outputs(
                buffers
                    .audio_outputs
                    .iter_mut()
                    .map(|buffer| &mut buffer[..frames]),
            )
            .with_cv_inputs(buffers.cv_inputs.iter().map(|buffer| &buffer[..frames]))
            .with_cv_outputs(
                buffers
                    .cv_outputs
                    .iter_mut()
                    .map(|buffer| &mut buffer[..frames]),
            )
            .with_atom_sequence_inputs(buffers.atom_inputs.iter())
            .with_atom_sequence_outputs(buffers.atom_outputs.iter_mut());
        // Buffers are disjoint, preallocated, and valid for the entire call.
        unsafe { (&mut *instance).run(frames, ports) }
            .map_err(|source| Error::Processing { source })
    }

    fn instance(&self) -> &livi::Instance {
        unsafe { &*self.native.instance.get() }
    }

    fn instance_mut(&mut self) -> &mut livi::Instance {
        unsafe { &mut *self.native.instance.get() }
    }
}

fn buffer_slot(
    descriptor: &PluginDescriptor,
    symbol: &str,
    kind: PortKind,
    input: bool,
) -> Option<usize> {
    descriptor
        .ports
        .iter()
        .filter(|port| port.kind == kind && port.input == input)
        .position(|port| port.symbol == symbol)
}

fn port_index(
    descriptor: &PluginDescriptor,
    symbol: &str,
    kind: PortKind,
    input: bool,
) -> Option<u32> {
    descriptor
        .ports
        .iter()
        .find(|port| port.kind == kind && port.input == input && port.symbol == symbol)
        .map(|port| port.index)
}

/// Encodes an LV2 `patch:Set` event whose value is an `atom:Path`.
pub(crate) fn patch_set_path_event(
    features: &livi::Features,
    property: &str,
    path: &str,
) -> Result<AtomEvent> {
    let urid = |uri: &'static [u8]| features.urid(CStr::from_bytes_with_nul(uri).unwrap());
    let property = CString::new(property).map_err(|_| Error::InvalidPatchValue {
        field: "state property",
    })?;
    let path = CString::new(path).map_err(|_| Error::InvalidPatchValue {
        field: "state value",
    })?;
    let mut object = Vec::with_capacity(64 + path.as_bytes_with_nul().len());
    let push_u32 =
        |buffer: &mut Vec<u8>, value: u32| buffer.extend_from_slice(&value.to_ne_bytes());
    push_u32(&mut object, 0u32);
    push_u32(&mut object, urid(b"http://lv2plug.in/ns/ext/patch#Set\0"));
    let property_atom = features.urid(property.as_c_str());
    let append_property = |buffer: &mut Vec<u8>, key: u32, atom_type: u32, value: &[u8]| {
        push_u32(buffer, key);
        push_u32(buffer, 0u32);
        push_u32(buffer, value.len() as u32);
        push_u32(buffer, atom_type);
        buffer.extend_from_slice(value);
        buffer.resize((buffer.len() + 7) & !7, 0);
    };
    append_property(
        &mut object,
        urid(b"http://lv2plug.in/ns/ext/patch#property\0"),
        urid(b"http://lv2plug.in/ns/ext/atom#URID\0"),
        &property_atom.to_ne_bytes(),
    );
    append_property(
        &mut object,
        urid(b"http://lv2plug.in/ns/ext/patch#value\0"),
        urid(b"http://lv2plug.in/ns/ext/atom#Path\0"),
        path.as_bytes_with_nul(),
    );
    Ok(AtomEvent {
        time_in_frames: 0,
        type_urid: urid(b"http://lv2plug.in/ns/ext/atom#Object\0"),
        data: object,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PortDescriptor;

    fn port(symbol: &str, kind: PortKind, input: bool) -> PortDescriptor {
        PortDescriptor {
            index: 0,
            symbol: symbol.to_owned(),
            name: symbol.to_owned(),
            input,
            kind,
            default: 0.0,
            minimum: 0.0,
            maximum: 1.0,
            logarithmic: false,
            unit: None,
        }
    }

    #[test]
    fn buffer_slots_are_relative_to_port_kind_and_direction() {
        let descriptor = PluginDescriptor {
            uri: "urn:test".to_owned(),
            name: "Test".to_owned(),
            classes: Vec::new(),
            ports: vec![
                port("gain", PortKind::Control, true),
                port("left_in", PortKind::Audio, true),
                port("cv_in", PortKind::Cv, true),
                port("right_in", PortKind::Audio, true),
                port("left_out", PortKind::Audio, false),
                port("events", PortKind::AtomSequence, false),
                port("right_out", PortKind::Audio, false),
            ],
            uis: Vec::new(),
        };

        assert_eq!(
            buffer_slot(&descriptor, "left_in", PortKind::Audio, true),
            Some(0)
        );
        assert_eq!(
            buffer_slot(&descriptor, "right_in", PortKind::Audio, true),
            Some(1)
        );
        assert_eq!(
            buffer_slot(&descriptor, "left_out", PortKind::Audio, false),
            Some(0)
        );
        assert_eq!(
            buffer_slot(&descriptor, "right_out", PortKind::Audio, false),
            Some(1)
        );
        assert_eq!(
            buffer_slot(&descriptor, "cv_in", PortKind::Cv, true),
            Some(0)
        );
        assert_eq!(
            buffer_slot(&descriptor, "events", PortKind::AtomSequence, false),
            Some(0)
        );
        assert_eq!(
            buffer_slot(&descriptor, "missing", PortKind::Audio, false),
            None
        );
        assert_eq!(
            port_index(&descriptor, "gain", PortKind::Control, true),
            Some(0)
        );
        assert_eq!(
            port_index(&descriptor, "gain", PortKind::Control, false),
            None
        );
    }
}
