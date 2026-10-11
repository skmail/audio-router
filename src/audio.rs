use objc2::{AnyThread, rc::Retained};
use objc2_core_audio::*;
use objc2_core_audio_types::*;
use objc2_core_foundation::{CFDictionary, CFString};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSObject, NSString, NSUUID};
use std::{ffi::c_void, ptr::NonNull};

pub type Result<T> = std::result::Result<T, String>;

fn check(status: i32, operation: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        let code = String::from_utf8_lossy(&status.to_be_bytes()).into_owned();
        Err(format!("{operation}: Core Audio error {status} ({code})."))
    }
}

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

pub trait HalValue: Copy {}
impl HalValue for u32 {}
impl HalValue for i32 {}
impl HalValue for f64 {}
impl HalValue for *const CFString {}
impl HalValue for AudioStreamBasicDescription {}

// The private property readers are only used with the exact types documented by HAL.
pub fn scalar<T: HalValue>(id: u32, selector: u32, scope: u32) -> Result<T> {
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>() as u32;
    unsafe {
        check(
            AudioObjectGetPropertyData(
                id,
                NonNull::from(&mut address(selector, scope)),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::new(value.as_mut_ptr()).unwrap().cast(),
            ),
            "Read audio property",
        )?;
    }
    if size as usize != size_of::<T>() {
        return Err("Unexpected audio property size".into());
    }
    Ok(unsafe { value.assume_init() })
}

pub fn ids(id: u32, selector: u32, scope: u32) -> Result<Vec<u32>> {
    let mut addr = address(selector, scope);
    let mut size = 0;
    unsafe {
        check(
            AudioObjectGetPropertyDataSize(
                id,
                NonNull::from(&mut addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
            ),
            "Read audio list size",
        )?;
        if size == 0 {
            return Ok(vec![]);
        }
        let mut result = vec![0u32; size as usize / 4];
        check(
            AudioObjectGetPropertyData(
                id,
                NonNull::from(&mut addr),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::new(result.as_mut_ptr()).unwrap().cast(),
            ),
            "Read audio list",
        )?;
        result.truncate(size as usize / 4);
        Ok(result)
    }
}

pub fn string(id: u32, selector: u32) -> Result<String> {
    let ptr: *const CFString = scalar(id, selector, kAudioObjectPropertyScopeGlobal)?;
    if ptr.is_null() {
        return Err("Missing audio device name".into());
    }
    // CF-valued HAL properties transfer ownership to the caller.
    let value = unsafe {
        objc2_core_foundation::CFRetained::from_raw(NonNull::new(ptr.cast_mut()).unwrap())
    };
    Ok(value.to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: u32,
    pub uid: String,
    pub name: String,
    pub rate: f64,
}

pub fn devices() -> Result<Vec<Device>> {
    let mut result = vec![];
    for id in ids(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )? {
        if ids(
            id,
            kAudioDevicePropertyStreams,
            kAudioObjectPropertyScopeOutput,
        )?
        .is_empty()
        {
            continue;
        }
        let uid = string(id, kAudioDevicePropertyDeviceUID)?;
        if uid.starts_with("audio-router.") {
            continue;
        }
        let alive: u32 = scalar(
            id,
            kAudioDevicePropertyDeviceIsAlive,
            kAudioObjectPropertyScopeGlobal,
        )?;
        if alive == 0 {
            continue;
        }
        result.push(Device {
            id,
            uid,
            name: string(id, kAudioObjectPropertyName)?,
            rate: scalar(
                id,
                kAudioDevicePropertyNominalSampleRate,
                kAudioObjectPropertyScopeGlobal,
            )?,
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

pub fn default_output() -> Result<u32> {
    scalar(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyScopeGlobal,
    )
}

fn dictionary(entries: &[(&str, &NSObject)]) -> Retained<NSDictionary<NSString, NSObject>> {
    let keys: Vec<_> = entries.iter().map(|(k, _)| NSString::from_str(k)).collect();
    NSDictionary::from_slices(
        &keys.iter().map(|k| &**k).collect::<Vec<_>>(),
        &entries.iter().map(|(_, v)| *v).collect::<Vec<_>>(),
    )
}

/// Owns every HAL resource. Dropping a route stops capture before releasing its callback state.
pub struct Route {
    tap: u32,
    aggregate: u32,
    io: AudioDeviceIOProcID,
    context: Box<Context>,
}

struct Context {
    tap_start_channel: usize,
}

impl Route {
    pub fn start(processes: &[u32], device: &Device) -> Result<Self> {
        if processes.is_empty() {
            return Err("The app has no audio processes yet".into());
        }
        let mut route = Self {
            tap: 0,
            aggregate: 0,
            io: None,
            context: Box::new(Context {
                tap_start_channel: 0,
            }),
        };
        unsafe {
            let processes = NSArray::from_retained_slice(
                &processes
                    .iter()
                    .map(|p| NSNumber::new_u32(*p))
                    .collect::<Vec<_>>(),
            );
            let description = CATapDescription::initStereoMixdownOfProcesses(
                CATapDescription::alloc(),
                &processes,
            );
            description.setName(&NSString::from_str("Audio Router"));
            description.setPrivate(true);
            description.setMuteBehavior(CATapMuteBehavior::MutedWhenTapped);
            check(
                AudioHardwareCreateProcessTap(Some(&description), &mut route.tap),
                "Capture app audio",
            )?;

            let uid = NSString::from_str(&device.uid);
            let zero = NSNumber::new_u32(0);
            let one = NSNumber::new_u32(1);
            // Limit the physical subdevice to output. The only input we need is the process tap.
            let subdevice = dictionary(&[("uid", &uid), ("channels-in", &zero)]);
            let subtap = dictionary(&[("uid", &description.UUID().UUIDString()), ("drift", &one)]);
            let subdevices = NSArray::from_retained_slice(&[subdevice]);
            let taps = NSArray::from_retained_slice(&[subtap]);
            let aggregate_uid =
                NSString::from_str(&format!("audio-router.{}", NSUUID::new().UUIDString()));
            let name = NSString::from_str("Audio Router · private route");
            let config = dictionary(&[
                ("uid", &aggregate_uid),
                ("name", &name),
                ("private", &one),
                ("master", &uid),
                ("subdevices", &subdevices),
                ("taps", &taps),
                ("tapautostart", &zero),
            ]);
            // NSDictionary and CFDictionary are toll-free bridged by Apple.
            let cf =
                &*((&*config as *const NSDictionary<NSString, NSObject>).cast::<CFDictionary>());
            check(
                AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut route.aggregate)),
                "Create output route",
            )?;

            let input_channels = validate_streams(route.aggregate, kAudioObjectPropertyScopeInput)?;
            let output_channels =
                validate_streams(route.aggregate, kAudioObjectPropertyScopeOutput)?;
            if input_channels < 2 || output_channels == 0 {
                return Err("Output does not provide a usable audio stream".into());
            }
            // HAL appends tap channels after any physical input channels. Never forward a microphone.
            route.context.tap_start_channel = input_channels - 2;
            check(
                AudioDeviceCreateIOProcID(
                    route.aggregate,
                    Some(render),
                    (&mut *route.context as *mut Context).cast(),
                    NonNull::from(&mut route.io),
                ),
                "Prepare output playback",
            )?;
            check(
                AudioDeviceStart(route.aggregate, route.io),
                "Start routing (allow System Audio Recording in macOS Settings)",
            )?;
        }
        Ok(route)
    }
}

fn validate_streams(device: u32, scope: u32) -> Result<usize> {
    let mut channels = 0;
    for stream in ids(device, kAudioDevicePropertyStreams, scope)? {
        let format: AudioStreamBasicDescription = scalar(
            stream,
            kAudioStreamPropertyVirtualFormat,
            kAudioObjectPropertyScopeGlobal,
        )?;
        if format.mFormatID != kAudioFormatLinearPCM
            || format.mBitsPerChannel != 32
            || format.mFormatFlags & kAudioFormatFlagIsFloat == 0
            || format.mFormatFlags & kAudioFormatFlagIsBigEndian != 0
        {
            return Err("This output does not support native 32-bit float PCM audio".into());
        }
        channels += format.mChannelsPerFrame as usize;
    }
    Ok(channels)
}

impl Drop for Route {
    fn drop(&mut self) {
        unsafe {
            if self.io.is_some() {
                AudioDeviceStop(self.aggregate, self.io);
                AudioDeviceDestroyIOProcID(self.aggregate, self.io);
            }
            if self.aggregate != 0 {
                AudioHardwareDestroyAggregateDevice(self.aggregate);
            }
            if self.tap != 0 {
                AudioHardwareDestroyProcessTap(self.tap);
            }
        }
    }
}

unsafe extern "C-unwind" fn render(
    _device: u32,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    context: *mut c_void,
) -> i32 {
    // HAL owns these variable-length lists for the callback duration. No allocations,
    // locks, logging, Objective-C calls, or disk access are permitted on this thread.
    unsafe {
        let inputs = std::slice::from_raw_parts(
            std::ptr::addr_of!((*input.as_ptr()).mBuffers).cast::<AudioBuffer>(),
            input.as_ref().mNumberBuffers as usize,
        );
        let outputs = std::slice::from_raw_parts_mut(
            std::ptr::addr_of_mut!((*output.as_ptr()).mBuffers).cast::<AudioBuffer>(),
            output.as_ref().mNumberBuffers as usize,
        );
        copy_audio(
            inputs,
            outputs,
            (*context.cast::<Context>()).tap_start_channel,
        );
    }
    0
}

// Handles interleaved and planar stereo, averages stereo for mono outputs, and
// leaves extra surround channels silent. All frame counts are bounded by buffers.
unsafe fn copy_audio(inputs: &[AudioBuffer], outputs: &mut [AudioBuffer], start: usize) {
    unsafe {
        let total_output: usize = outputs.iter().map(|b| b.mNumberChannels as usize).sum();
        let mut output_channel = 0;
        for out in outputs {
            let channels = out.mNumberChannels as usize;
            if out.mData.is_null() || channels == 0 {
                output_channel += channels;
                continue;
            }
            std::ptr::write_bytes(out.mData.cast::<u8>(), 0, out.mDataByteSize as usize);
            let frames = out.mDataByteSize as usize / (4 * channels);
            for channel in 0..channels {
                let destination = output_channel + channel;
                if destination >= 2 {
                    continue;
                }
                for frame in 0..frames {
                    let value = if total_output == 1 {
                        (sample(inputs, start, frame) + sample(inputs, start + 1, frame)) * 0.5
                    } else {
                        sample(inputs, start + destination, frame)
                    };
                    *out.mData.cast::<f32>().add(frame * channels + channel) = value;
                }
            }
            output_channel += channels;
        }
    }
}

unsafe fn sample(buffers: &[AudioBuffer], mut channel: usize, frame: usize) -> f32 {
    for buffer in buffers {
        let channels = buffer.mNumberChannels as usize;
        if channel < channels {
            let offset = frame * channels + channel;
            if buffer.mData.is_null() || offset >= buffer.mDataByteSize as usize / 4 {
                return 0.0;
            }
            return unsafe { *buffer.mData.cast::<f32>().add(offset) };
        }
        channel -= channels;
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn buffer(data: &mut [f32], channels: u32) -> AudioBuffer {
        AudioBuffer {
            mNumberChannels: channels,
            mDataByteSize: std::mem::size_of_val(data) as u32,
            mData: data.as_mut_ptr().cast(),
        }
    }
    #[test]
    fn skips_microphone_and_converts_interleaved_to_planar() {
        let mut mic = [9., 9.];
        let mut tap = [0.1, 0.2, 0.3, 0.4];
        let mut left = [0.; 3];
        let mut right = [0.; 3];
        unsafe {
            copy_audio(
                &[buffer(&mut mic, 1), buffer(&mut tap, 2)],
                &mut [buffer(&mut left, 1), buffer(&mut right, 1)],
                1,
            );
        }
        assert_eq!(left, [0.1, 0.3, 0.]);
        assert_eq!(right, [0.2, 0.4, 0.]);
    }
    #[test]
    fn downmixes_mono_and_silences_surround() {
        let mut left = [0.2, 0.4];
        let mut right = [0.4, 0.8];
        let mut mono = [0.; 2];
        unsafe {
            copy_audio(
                &[buffer(&mut left, 1), buffer(&mut right, 1)],
                &mut [buffer(&mut mono, 1)],
                0,
            );
        }
        assert_eq!(mono, [0.3, 0.6]);
        let mut surround = [9.; 8];
        unsafe {
            copy_audio(
                &[buffer(&mut left, 1), buffer(&mut right, 1)],
                &mut [buffer(&mut surround, 4)],
                0,
            );
        }
        assert_eq!(surround, [0.2, 0.4, 0., 0., 0.4, 0.8, 0., 0.]);
    }
    #[test]
    fn disabled_input_is_silent() {
        let input = AudioBuffer {
            mNumberChannels: 2,
            mDataByteSize: 16,
            mData: std::ptr::null_mut(),
        };
        let mut output = [9.; 4];
        unsafe {
            copy_audio(&[input], &mut [buffer(&mut output, 2)], 0);
        }
        assert_eq!(output, [0.; 4]);
    }
}
