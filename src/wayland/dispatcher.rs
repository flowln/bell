pub mod dispatcher {
    use wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle, protocol::wl_registry,
    };

    use wayland_client::backend::ObjectId;
    use wayland_client::protocol::wl_buffer::WlBuffer;
    use wayland_client::protocol::wl_compositor::WlCompositor;
    use wayland_client::protocol::wl_output::WlOutput;
    use wayland_client::protocol::wl_shm::WlShm;
    use wayland_client::protocol::wl_surface::WlSurface;

    use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
    use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
    use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;

    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

    use crate::wayland::wayland::GlobalData;

    pub struct UserData;
    #[derive(Debug)]
    pub struct BufferUserData {
        pub parent_id: Option<ObjectId>,
    }
    pub struct XdgUserData;
    pub struct WlrUserData {
        pub parent_id: ObjectId,
    }
    pub struct SurfaceUserData {
        pub parent_id: ObjectId,
    }
    pub struct WlSurfaceUserData;
    pub struct WlOutputUserData;


    fn binds_on<I: Proxy + 'static>(interface: &String) -> bool {
        interface == I::interface().name
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for GlobalData {
        fn event(
            state: &mut Self,
            registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            queue_handle: &QueueHandle<GlobalData>,
        ) {
            match event {
                wl_registry::Event::Global {
                    name,
                    interface,
                    version,
                } => {
                    if binds_on::<WlCompositor>(&interface) {
                        state.compositor =
                            Some(registry.bind(name, version, queue_handle, UserData {}));
                    } else if binds_on::<WlOutput>(&interface) {
                        let _: WlOutput =
                            registry.bind(name, version, queue_handle, WlOutputUserData {});
                    } else if binds_on::<WlShm>(&interface) {
                        state.shared_memory =
                            Some(registry.bind(name, version, queue_handle, UserData {}));
                    } else if binds_on::<XdgWmBase>(&interface) {
                        state.xdg_wm_base =
                            Some(registry.bind(name, version, queue_handle, XdgUserData {}));
                    } else if binds_on::<ZwlrLayerShellV1>(&interface) {
                        state.wlr_layer_shell =
                            Some(registry.bind(name, version, queue_handle, UserData {}));
                    }

                    println!("Name: {name}, Interface: {interface}");
                }
                wl_registry::Event::GlobalRemove { name: _ } => {}
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl<I: Proxy> Dispatch<I, UserData> for GlobalData {
        fn event(
            _state: &mut GlobalData,
            _proxy: &I,
            _event: <I as Proxy>::Event,
            _data: &UserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<GlobalData>,
        ) {
            println!("{_proxy:?}");
        }
    }

    impl Dispatch<XdgWmBase, XdgUserData> for GlobalData {
        fn event(
            _state: &mut GlobalData,
            proxy: &XdgWmBase,
            event: <XdgWmBase as Proxy>::Event,
            _data: &XdgUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<GlobalData>,
        ) {
            type EventType = <XdgWmBase as Proxy>::Event;
            match event {
                EventType::Ping { serial } => {
                    proxy.pong(serial);
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl Dispatch<XdgSurface, SurfaceUserData> for GlobalData {
        fn event(
            _state: &mut GlobalData,
            proxy: &XdgSurface,
            event: <XdgSurface as Proxy>::Event,
            _data: &SurfaceUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<GlobalData>,
        ) {
            type EventType = <XdgSurface as Proxy>::Event;
            match event {
                EventType::Configure { serial } => {
                    proxy.ack_configure(serial);
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl Dispatch<XdgToplevel, SurfaceUserData> for GlobalData {
        fn event(
            state: &mut GlobalData,
            _proxy: &XdgToplevel,
            event: <XdgToplevel as Proxy>::Event,
            data: &SurfaceUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<GlobalData>,
        ) {
            type EventType = <XdgToplevel as Proxy>::Event;
            match event {
                EventType::Close => {
                    state.destroy_surface(&data.parent_id);
                }
                EventType::Configure {
                    width,
                    height,
                    states,
                } => {
                    println!("XdgToplevel: configure {} {}", width, height);
                }
                EventType::ConfigureBounds { width, height } => {
                    println!("XdgToplevel: configure_bounds {} {}", width, height);
                }
                EventType::WmCapabilities { capabilities } => {
                    println!("XdgToplevel: wm_capabilities")
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl Dispatch<ZwlrLayerSurfaceV1, WlrUserData> for GlobalData {
        fn event(
            state: &mut GlobalData,
            proxy: &ZwlrLayerSurfaceV1,
            event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
            data: &WlrUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<GlobalData>,
        ) {
            type EventType = <ZwlrLayerSurfaceV1 as Proxy>::Event;
            match event {
                EventType::Configure {
                    serial,
                    width,
                    height,
                } => {
                    proxy.ack_configure(serial);

                    if let Some(surface) = state.get_surface(&data.parent_id) {
                        surface.set_configured();
                    } else {
                        eprintln!("No surface associated with id {:?}.", data.parent_id);
                    }
                }
                EventType::Closed => {
                    state.destroy_surface(&data.parent_id);
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl Dispatch<WlBuffer, BufferUserData> for GlobalData {
        fn event(
            state: &mut GlobalData,
            _proxy: &WlBuffer,
            event: <WlBuffer as Proxy>::Event,
            data: &BufferUserData,
            _conn: &Connection,
            queue_handle: &QueueHandle<GlobalData>,
        ) {
            type EventType = <WlBuffer as Proxy>::Event;
            match event {
                EventType::Release => {
                    if let Some(surface_id) = data.parent_id.as_ref() {
                        if let Some(surface) = state.get_surface(surface_id) {
                            surface.set_ready_to_draw(queue_handle);
                        } else {
                            eprintln!("No surface associated with id {surface_id:?}.");
                        }
                    } else {
                        eprintln!("No surface id associated with buffer.");
                    }
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl Dispatch<WlSurface, WlSurfaceUserData> for GlobalData {
        fn event(
            state: &mut GlobalData,
            proxy: &WlSurface,
            event: <WlSurface as Proxy>::Event,
            _data: &WlSurfaceUserData,
            _conn: &Connection,
            queue_handle: &QueueHandle<GlobalData>,
        ) {
            type EventType = <WlSurface as Proxy>::Event;
            match event {
                EventType::PreferredBufferScale { factor } => {
                    if let Some(surface) = state.surfaces.get_mut(&proxy.id()) {
                        surface.set_buffer_scale(
                            factor,
                            state.shared_memory.as_mut().unwrap(),
                            queue_handle,
                        );
                    }
                }
                _ => {
                    println!("WlSurface: {:?}", event);
                }
            }
        }
    }

    impl Dispatch<WlOutput, WlOutputUserData> for GlobalData {
        fn event(
            state: &mut GlobalData,
            proxy: &WlOutput,
            event: <WlOutput as Proxy>::Event,
            _data: &WlOutputUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<GlobalData>,
        ) {
            type EventType = <WlOutput as Proxy>::Event;
            match event {
                EventType::Name { name } => {
                    state.outputs.insert(name, proxy.clone());
                }
                _ => {
                    println!("WlOutput: {:?}", event);
                }
            }
        }
    }
}

