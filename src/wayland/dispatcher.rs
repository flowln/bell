pub mod dispatcher {
    use std::sync::RwLock;

    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, protocol::wl_registry};

    use wayland_client::backend::ObjectId;
    use wayland_client::protocol::wl_buffer::WlBuffer;
    use wayland_client::protocol::wl_callback::WlCallback;
    use wayland_client::protocol::wl_compositor::WlCompositor;
    use wayland_client::protocol::wl_output::WlOutput;
    use wayland_client::protocol::wl_pointer::WlPointer;
    use wayland_client::protocol::wl_seat::{Capability, WlSeat};
    use wayland_client::protocol::wl_shm::WlShm;
    use wayland_client::protocol::wl_surface::WlSurface;

    use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
        Shape, WpCursorShapeDeviceV1,
    };
    use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;

    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

    use crate::wayland::WaylandState;

    macro_rules! debug_println {
        ($string:literal) => {
            if cfg!(debug_assertions) { println!($string) }
        };
        ($format:literal,$( $value:ident ),+) => {
            if cfg!(debug_assertions) { println!($format, $( $value, )+) }
        };
    }

    pub struct UserData;
    #[derive(Debug)]
    pub struct BufferUserData {
        pub parent_id: Option<ObjectId>,
    }
    pub struct DisplayUserData;
    pub struct WlrUserData {
        pub parent_id: ObjectId,
    }
    pub struct WlOutputUserData;
    pub struct PointerUserData {
        current_surface: RwLock<Option<(u32, ObjectId)>>,
        shape_device: RwLock<Option<WpCursorShapeDeviceV1>>,
    }
    pub struct WlSurfaceUserData;
    pub struct SeatUserData {
        pointers: RwLock<Vec<WlPointer>>,
    }

    pub use wayland_client::protocol::wl_pointer::ButtonState;
    // Ref.: https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/include/uapi/linux/input-event-codes.h?h=v7.0.2#n356
    #[repr(u32)]
    pub enum LinuxButtonCode {
        Unknown(u32),
        Left = 0x110,
        Right = 0x111,
        Middle = 0x112,
    }

    impl From<u32> for LinuxButtonCode {
        fn from(data: u32) -> LinuxButtonCode {
            match data {
                0x110 => LinuxButtonCode::Left,
                0x111 => LinuxButtonCode::Right,
                0x112 => LinuxButtonCode::Middle,
                _ => LinuxButtonCode::Unknown(data),
            }
        }
    }

    fn binds_on<I: Proxy + 'static>(interface: &String) -> bool {
        interface == I::interface().name
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
        fn event(
            state: &mut Self,
            registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            queue_handle: &QueueHandle<WaylandState>,
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

                        // We'll be waiting on the server providing output information
                        state.pending_data_amount += 1;
                    } else if binds_on::<WlSeat>(&interface) {
                        state.seats.push(registry.bind(
                            name,
                            version,
                            queue_handle,
                            SeatUserData {
                                pointers: RwLock::new(Vec::new()),
                            },
                        ));
                    } else if binds_on::<WlShm>(&interface) {
                        state.shared_memory =
                            Some(registry.bind(name, version, queue_handle, UserData {}));
                    } else if binds_on::<ZwlrLayerShellV1>(&interface) {
                        state.wlr_layer_shell =
                            Some(registry.bind(name, version, queue_handle, UserData {}));
                    } else if binds_on::<WpCursorShapeManagerV1>(&interface) {
                        state.cursor_shape_manager =
                            Some(registry.bind(name, version, queue_handle, UserData {}));
                    }

                    debug_println!("Name: {name}, Interface: {interface}");
                }
                wl_registry::Event::GlobalRemove { name: _ } => {}
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl<I: Proxy> Dispatch<I, UserData> for WaylandState {
        fn event(
            _state: &mut WaylandState,
            _proxy: &I,
            _event: <I as Proxy>::Event,
            _data: &UserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<WaylandState>,
        ) {
            debug_println!("{_proxy:?}");
        }
    }

    impl Dispatch<WlCallback, DisplayUserData> for WaylandState {
        fn event(
            state: &mut WaylandState,
            _proxy: &WlCallback,
            _event: <WlCallback as Proxy>::Event,
            _data: &DisplayUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<WaylandState>,
        ) {
            state.pending_data_amount -= 1;
        }
    }

    impl Dispatch<ZwlrLayerSurfaceV1, WlrUserData> for WaylandState {
        fn event(
            state: &mut WaylandState,
            proxy: &ZwlrLayerSurfaceV1,
            event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
            data: &WlrUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<WaylandState>,
        ) {
            type EventType = <ZwlrLayerSurfaceV1 as Proxy>::Event;
            match event {
                EventType::Configure {
                    serial,
                    width: _,
                    height: _,
                } => {
                    proxy.ack_configure(serial);

                    if let Some(surface) = state.get_surface(&data.parent_id) {
                        surface.set_configured();
                    } else {
                        eprintln!("No surface associated with id {:?}.", data.parent_id);
                    }
                }
                EventType::Closed => {
                    state.mark_surface_for_destruction(&data.parent_id);
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl Dispatch<WlBuffer, BufferUserData> for WaylandState {
        fn event(
            state: &mut WaylandState,
            _proxy: &WlBuffer,
            event: <WlBuffer as Proxy>::Event,
            data: &BufferUserData,
            _conn: &Connection,
            queue_handle: &QueueHandle<WaylandState>,
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

    impl Dispatch<WlPointer, PointerUserData> for WaylandState {
        fn event(
            wayland_state: &mut WaylandState,
            _proxy: &WlPointer,
            event: <WlPointer as Proxy>::Event,
            data: &PointerUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<WaylandState>,
        ) {
            type EventType = <WlPointer as Proxy>::Event;
            match event {
                EventType::Enter {
                    serial,
                    surface,
                    surface_x: _,
                    surface_y: _,
                } => {
                    let mut current_state = data.current_surface.write().unwrap();
                    current_state.replace((serial, surface.id()));

                    let shape_device_opt = data.shape_device.write().unwrap();
                    if let Some(shape_device) = &*shape_device_opt {
                        shape_device.set_shape(serial, Shape::Default);
                    }
                }
                EventType::Leave { serial, surface } => {
                    let mut current_state = data.current_surface.write().unwrap();

                    match &*current_state {
                        Some((old_serial, old_surface_id)) => {
                            if serial > *old_serial && surface.id() == *old_surface_id {
                                current_state.take();
                            } else {
                                eprintln!(
                                    "WlPointer: Invalid leave event received - Old: {} {} | Received: {} {}",
                                    old_serial,
                                    old_surface_id,
                                    serial,
                                    surface.id()
                                );
                            }
                        }
                        None => {
                            eprintln!(
                                "WlPointer: Left a surface (id: {}) without entering it first.",
                                surface.id()
                            );
                        }
                    }
                }
                EventType::Button {
                    serial,
                    time: _,
                    button,
                    state,
                } => {
                    let current_state_opt = data.current_surface.read().unwrap();

                    if let Some(current_state) = &*current_state_opt
                        && serial > current_state.0
                    {
                        if let Ok(state) = state.into_result() {
                            wayland_state.with_surface(&current_state.1, |wl_state, surface| {
                                surface.handle_pointer_event(
                                    wl_state,
                                    LinuxButtonCode::from(button),
                                    state,
                                );
                            });
                        } else {
                            eprintln!("WlPointer: Invalid button state: {:?}", state);
                        }
                    } else {
                        eprintln!("WlPointer: Invalid button event received.");
                    }
                }
                _ => {}
            }
        }
    }

    impl Dispatch<WlSeat, SeatUserData> for WaylandState {
        fn event(
            state: &mut WaylandState,
            proxy: &WlSeat,
            event: <WlSeat as Proxy>::Event,
            data: &SeatUserData,
            _conn: &Connection,
            queue_handle: &QueueHandle<WaylandState>,
        ) {
            type EventType = <WlSeat as Proxy>::Event;
            match event {
                EventType::Capabilities { capabilities } => {
                    let capabilities: Capability = match capabilities.into_result() {
                        Ok(caps) => caps,
                        Err(error) => {
                            eprintln!(
                                "WlSeat: Error handling capabilities input: {}",
                                error.to_string()
                            );
                            Capability::empty()
                        }
                    };

                    let mut pointers = data.pointers.write().unwrap();
                    if capabilities.intersects(Capability::Pointer) {
                        let pointer = proxy.get_pointer(
                            queue_handle,
                            PointerUserData {
                                current_surface: RwLock::new(None),
                                shape_device: RwLock::new(None),
                            },
                        );

                        if let Some(cursor_shape_manager) = &state.cursor_shape_manager {
                            let shape_device = cursor_shape_manager.get_pointer(
                                &pointer,
                                queue_handle,
                                UserData {},
                            );

                            let shape_device_lock =
                                &pointer.data::<PointerUserData>().unwrap().shape_device;
                            shape_device_lock.write().unwrap().replace(shape_device);
                        }

                        pointers.push(pointer);
                    } else {
                        while let Some(pointer) = pointers.pop() {
                            pointer.release();
                        }
                    }
                }
                EventType::Name { name } => {
                    debug_println!("WlSeat: Registered seat with name '{name}'");
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }

    impl Dispatch<WlSurface, WlSurfaceUserData> for WaylandState {
        fn event(
            state: &mut WaylandState,
            proxy: &WlSurface,
            event: <WlSurface as Proxy>::Event,
            _data: &WlSurfaceUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<WaylandState>,
        ) {
            type EventType = <WlSurface as Proxy>::Event;
            match event {
                EventType::PreferredBufferScale { factor } => {
                    state
                        .with_surface(&proxy.id(), |wl_state, surface| {
                            surface.set_buffer_scale(wl_state, factor)
                        })
                        .unwrap();
                }
                _ => {
                    debug_println!("WlSurface: {:?}", event);
                }
            }
        }
    }

    impl Dispatch<WlOutput, WlOutputUserData> for WaylandState {
        fn event(
            state: &mut WaylandState,
            proxy: &WlOutput,
            event: <WlOutput as Proxy>::Event,
            _data: &WlOutputUserData,
            _conn: &Connection,
            _qhandle: &QueueHandle<WaylandState>,
        ) {
            type EventType = <WlOutput as Proxy>::Event;
            match event {
                EventType::Name { name } => {
                    state.add_output(name, proxy.clone());
                }
                EventType::Done => {
                    state.pending_data_amount -= 1;
                }
                _ => {
                    debug_println!("WlOutput: {:?}", event);
                }
            }
        }
    }
}
