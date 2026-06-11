use std::{cell::RefCell, rc::Rc};

use pipewire::{
    context::ContextRc,
    core::PW_ID_CORE,
    main_loop::MainLoopRc,
    types::ObjectType,
};

use crate::{
    ApplicationResult,
    backends::{AudioBackend, AudioDevice, StreamConfig},
};

pub struct PipeWireAudioBackend {
    sinks: Vec<AudioDevice>,
    sources: Vec<AudioDevice>,
    cmd_sender: Option<pipewire::channel::Sender<crate::PwStreamCmd>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PipeWireAudioBackend {
    pub const NAME: &'static str = "PipeWire";

    pub fn new() -> ApplicationResult<Self> {
        pipewire::init();

        let main_loop = MainLoopRc::new(None)?;
        let context = ContextRc::new(&main_loop, None)?;
        let core = context.connect_rc(None)?;
        let registry = core.get_registry_rc()?;

        let sinks: Rc<RefCell<Vec<AudioDevice>>> = Rc::new(RefCell::new(vec![]));
        let sources: Rc<RefCell<Vec<AudioDevice>>> = Rc::new(RefCell::new(vec![]));

        let sinks_clone = sinks.clone();
        let sources_clone = sources.clone();

        let _reg_listener = registry
            .add_listener_local()
            .global(move |object| {
                if object.type_ != ObjectType::Node {
                    return;
                }

                let Some(props) = &object.props else { return };
                let Some(class) = props.get("media.class") else { return };

                let name = props
                    .get("node.description")
                    .or_else(|| props.get("node.name"))
                    .unwrap_or("Unknown")
                    .to_string();

                let id = object.id.to_string();

                match class {
                    "Audio/Sink" => sinks_clone.borrow_mut().push(AudioDevice { name, id }),
                    "Audio/Source" => sources_clone.borrow_mut().push(AudioDevice { name, id }),
                    _ => {}
                }
            })
            .register();

        let ml = main_loop.clone();
        let pending = core.sync(0)?;

        let _core_listener = core
            .add_listener_local()
            .done(move |id, seq| {
                if id == PW_ID_CORE && seq == pending {
                    ml.quit();
                }
            })
            .register();

        main_loop.run();

        let sinks = sinks.borrow().clone();
        let sources = sources.borrow().clone();

        Ok(Self {
            sinks,
            sources,
            cmd_sender: None,
            thread: None,
        })
    }
}

fn make_audio_param_bytes(sample_rate: u32, channels: u32) -> Vec<u8> {
    use pipewire::spa;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(sample_rate);
    audio_info.set_channels(channels);
    let mut position = [0u32; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    audio_info.set_position(position);

    pipewire::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pipewire::spa::pod::Value::Object(pipewire::spa::pod::Object {
            type_: spa::utils::SpaTypes::ObjectParamFormat.0,
            id: spa::param::ParamType::EnumFormat.0,
            properties: audio_info.into(),
        }),
    )
    .unwrap()
    .0
    .into_inner()
}

impl AudioBackend for PipeWireAudioBackend {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn sinks(&self) -> Vec<AudioDevice> {
        self.sinks.clone()
    }

    fn sources(&self) -> Vec<AudioDevice> {
        self.sources.clone()
    }

    fn start_output(
        &mut self,
        device_id: &str,
        config: StreamConfig,
        process: Box<dyn Fn(&mut [f32]) + Send + 'static>,
    ) -> ApplicationResult<()> {
        let (cmd_sender, cmd_receiver) = pipewire::channel::channel::<crate::PwStreamCmd>();

        let node_id: Option<u32> = if device_id.is_empty() {
            None
        } else {
            device_id.parse().ok()
        };

        let channels = config.channels;
        let sample_rate = config.sample_rate as u32;
        let buffer_size = config.buffer_size;

        if let Ok(mut s) = crate::PW_STREAM_SENDER.write() {
            *s = Some(cmd_sender.clone());
        }

        let thread = std::thread::spawn(move || {
            use pipewire as pw;
            use pw::spa;

            pw::init();

            let mainloop = match MainLoopRc::new(None) {
                Ok(ml) => ml,
                Err(e) => { crate::rlog!("[pw stream] mainloop: {e}"); return; }
            };
            let context = match ContextRc::new(&mainloop, None) {
                Ok(c) => c,
                Err(e) => { crate::rlog!("[pw stream] context: {e}"); return; }
            };
            let core = match context.connect_rc(None) {
                Ok(c) => c,
                Err(e) => { crate::rlog!("[pw stream] core: {e}"); return; }
            };

            let props = pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::AUDIO_CHANNELS => channels.to_string(),
                *pw::keys::NODE_LATENCY => format!("{}/{}", buffer_size, sample_rate),
            };

            let stream = match pw::stream::StreamRc::new(core, "rwasio-output", props) {
                Ok(s) => s,
                Err(e) => { crate::rlog!("[pw stream] stream: {e}"); return; }
            };

            let _listener = stream
                .add_local_listener::<()>()
                .process(move |stream, _| {
                    let Some(mut buffer) = stream.dequeue_buffer() else { return };
                    let datas = buffer.datas_mut();
                    let Some(data) = datas.get_mut(0) else { return };

                    let stride = std::mem::size_of::<f32>() * channels as usize;
                    let n_frames = if let Some(slice) = data.data() {
                        let n_frames = (slice.len() / stride).min(buffer_size as usize);
                        let samples = unsafe {
                            std::slice::from_raw_parts_mut(
                                slice.as_mut_ptr() as *mut f32,
                                n_frames * channels as usize,
                            )
                        };
                        process(samples);
                        n_frames
                    } else {
                        0
                    };

                    let chunk = data.chunk_mut();
                    *chunk.offset_mut() = 0;
                    *chunk.stride_mut() = stride as i32;
                    *chunk.size_mut() = (stride * n_frames) as u32;
                })
                .register()
                .unwrap();

            let values = make_audio_param_bytes(sample_rate, channels);
            let mut params = [pw::spa::pod::Pod::from_bytes(&values).unwrap()];

            if let Err(e) = stream.connect(
                spa::utils::Direction::Output,
                node_id,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut params,
            ) {
                crate::rlog!("[pw stream] connect: {e}");
                return;
            }

            let stream_for_cmd = stream.clone();
            let _cmd_attached = cmd_receiver.attach(mainloop.loop_(), {
                let ml = mainloop.clone();
                move |cmd| match cmd {
                    crate::PwStreamCmd::Stop => ml.quit(),
                    crate::PwStreamCmd::SetTarget(new_node_id) => {
                        crate::rlog!("[pw stream] retargeting to {:?}", new_node_id);
                        let _ = stream_for_cmd.disconnect();
                        let values = make_audio_param_bytes(sample_rate, channels);
                        let mut params = [pw::spa::pod::Pod::from_bytes(&values).unwrap()];
                        if let Err(e) = stream_for_cmd.connect(
                            spa::utils::Direction::Output,
                            new_node_id,
                            pw::stream::StreamFlags::AUTOCONNECT
                                | pw::stream::StreamFlags::MAP_BUFFERS
                                | pw::stream::StreamFlags::RT_PROCESS,
                            &mut params,
                        ) {
                            crate::rlog!("[pw stream] retarget connect: {e}");
                        }
                    }
                }
            });

            crate::rlog!("[pw stream] running, node_id={:?}", node_id);
            mainloop.run();
            crate::rlog!("[pw stream] stopped");
        });

        self.cmd_sender = Some(cmd_sender);
        self.thread = Some(thread);
        Ok(())
    }

    fn stop_output(&mut self) -> ApplicationResult<()> {
        if let Some(sender) = self.cmd_sender.take() {
            let _ = sender.send(crate::PwStreamCmd::Stop);
        }
        if let Ok(mut s) = crate::PW_STREAM_SENDER.write() {
            *s = None;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        Ok(())
    }
}
