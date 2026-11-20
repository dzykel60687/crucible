//! Adapter: lets vst3-host's playback path output through our exclusive-mode stream.
//!
//! Portable code. The exclusive stream itself lives behind the audio seam in
//! `daw-audio-io`, which returns an error on systems where exclusive mode does not apply.

use daw_diagnostics::DawError;
use std::sync::{Arc, Mutex};
use vst3_host::{AudioBackend, AudioConfig, AudioStream};

#[derive(Debug)]
pub struct BackendError(pub DawError);

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for BackendError {}

/// Shared slot so the program can read the stream's details and timing while it runs.
pub type StreamSlot = Arc<Mutex<Option<daw_audio_io::ExclusiveOutput>>>;

pub struct ExclusiveBackend {
    pub device: Option<String>,
    pub period_frames: Option<u32>,
    pub slot: StreamSlot,
    /// The coded reason the stream could not open. vst3-host wraps backend errors in its own
    /// type, so the code is kept here for the caller to report.
    pub open_error: Arc<Mutex<Option<DawError>>>,
}

pub struct ExclusiveStream {
    slot: StreamSlot,
}

impl AudioStream for ExclusiveStream {
    // The stream starts as soon as it is opened; play and pause are not needed by the spike.
    fn play(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn pause(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

impl Drop for ExclusiveStream {
    fn drop(&mut self) {
        // Stops the audio thread before the plugin is torn down.
        let taken = self.slot.lock().unwrap_or_else(|p| p.into_inner()).take();
        drop(taken);
    }
}

impl AudioBackend for ExclusiveBackend {
    type Stream = ExclusiveStream;
    type Device = ();
    type Error = BackendError;

    fn enumerate_output_devices(&self) -> Result<Vec<()>, BackendError> {
        Ok(vec![()])
    }
    fn enumerate_input_devices(&self) -> Result<Vec<()>, BackendError> {
        Ok(vec![])
    }
    fn default_output_device(&self) -> Option<()> {
        Some(())
    }
    fn default_input_device(&self) -> Option<()> {
        None
    }

    fn create_output_stream(
        &self,
        _device: &(),
        config: AudioConfig,
        data_callback: Box<dyn FnMut(&mut [f32]) + Send>,
        _error_callback: Box<dyn FnMut(BackendError) + Send>,
    ) -> Result<ExclusiveStream, BackendError> {
        let out = daw_audio_io::open_exclusive_output(
            self.device.clone(),
            config.sample_rate as u32,
            self.period_frames,
            config.output_channels,
            data_callback,
        )
        .map_err(|e| {
            *self.open_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.clone());
            BackendError(e)
        })?;
        *self.slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(out);
        Ok(ExclusiveStream { slot: self.slot.clone() })
    }

    fn create_input_stream(
        &self,
        _device: &(),
        _config: AudioConfig,
        _data_callback: Box<dyn FnMut(&[f32]) + Send>,
        _error_callback: Box<dyn FnMut(BackendError) + Send>,
    ) -> Result<ExclusiveStream, BackendError> {
        Err(BackendError(DawError::new(daw_diagnostics::Code::StreamStartFailed, "audio input is not part of the M0 spike")))
    }
}
