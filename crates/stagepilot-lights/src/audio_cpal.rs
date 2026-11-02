//! The real audio input, through cpal (CoreAudio on macOS, WASAPI on Windows, ALSA on Linux).
//! Everything here is safe code. The stream lives on its own thread (some platforms' streams
//! cannot move between threads); closing the handle stops the thread and releases the device.
//! Samples are handed to the sink as mono floats and not kept.

use std::sync::mpsc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, ErrorKind, FromSample, Sample, SampleFormat, SizedSample};

use crate::audio::{AudioError, AudioInput, AudioStream, ErrorSink, SampleSink};
use crate::loudness::downmix;

pub struct CpalInput;

fn map_error(error: &cpal::Error, device: &str) -> AudioError {
    match error.kind() {
        ErrorKind::PermissionDenied => AudioError::PermissionDenied,
        ErrorKind::DeviceNotAvailable => AudioError::DeviceLost(device.to_string()),
        ErrorKind::HostUnavailable => AudioError::NoDevice,
        _ => AudioError::Other(format!("The audio input could not be used: {error}")),
    }
}

fn device_name(device: &Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "audio input".into())
}

fn input_devices() -> Result<Vec<Device>, AudioError> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|error| map_error(&error, "audio input"))?;
    Ok(devices.collect())
}

impl AudioInput for CpalInput {
    fn devices(&self) -> Result<Vec<String>, AudioError> {
        let mut names: Vec<String> = input_devices()?.iter().map(device_name).collect();
        names.sort();
        names.dedup();
        if names.is_empty() {
            return Err(AudioError::NoDevice);
        }
        Ok(names)
    }

    fn open(
        &self,
        device: Option<&str>,
        samples: SampleSink,
        errors: ErrorSink,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let wanted = device.map(str::to_string);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), AudioError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("stagepilot-audio-input".into())
            .spawn(move || {
                let stream = match build(wanted.as_deref(), samples, errors) {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                // Held until the handle is dropped (the sender goes away).
                let _ = stop_rx.recv();
                drop(stream);
            })
            .map_err(|error| {
                AudioError::Other(format!("The audio input could not start: {error}"))
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(Handle {
                stop: Some(stop_tx),
                thread: Some(thread),
            })),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err(AudioError::Other("The audio input could not start.".into())),
        }
    }
}

struct Handle {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl AudioStream for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn build(
    wanted: Option<&str>,
    samples: SampleSink,
    errors: ErrorSink,
) -> Result<cpal::Stream, AudioError> {
    let host = cpal::default_host();
    let device = match wanted {
        Some(name) => input_devices()?
            .into_iter()
            .find(|d| device_name(d) == name)
            .ok_or_else(|| AudioError::DeviceLost(name.to_string()))?,
        None => host.default_input_device().ok_or(AudioError::NoDevice)?,
    };
    let name = device_name(&device);
    let config = device
        .default_input_config()
        .map_err(|error| map_error(&error, &name))?;
    let channels = usize::from(config.channels()).max(1);
    let rate = config.sample_rate();
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let on_error = {
        let name = name.clone();
        move |error: cpal::Error| errors(map_error(&error, &name))
    };
    let stream = match format {
        SampleFormat::F32 => {
            stream::<f32>(&device, stream_config, channels, rate, samples, on_error)
        }
        SampleFormat::I16 => {
            stream::<i16>(&device, stream_config, channels, rate, samples, on_error)
        }
        SampleFormat::U16 => {
            stream::<u16>(&device, stream_config, channels, rate, samples, on_error)
        }
        SampleFormat::I32 => {
            stream::<i32>(&device, stream_config, channels, rate, samples, on_error)
        }
        SampleFormat::I8 => stream::<i8>(&device, stream_config, channels, rate, samples, on_error),
        other => Err(AudioError::Other(format!(
            "The audio input uses an unsupported sample format ({other})."
        ))),
    }?;
    stream.play().map_err(|error| map_error(&error, &name))?;
    Ok(stream)
}

fn stream<T>(
    device: &Device,
    config: cpal::StreamConfig,
    channels: usize,
    rate: u32,
    samples: SampleSink,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + Sample + Send + 'static,
    f32: FromSample<T>,
{
    let name = device_name(device);
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &_| {
                let floats: Vec<f32> = data.iter().map(|s| f32::from_sample(*s)).collect();
                samples(&downmix(&floats, channels), rate);
            },
            on_error,
            None,
        )
        .map_err(|error| map_error(&error, &name))
}
