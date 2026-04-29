mod dispatcher;
mod epoll;

pub mod wayland {
    use std::cell::Cell;

    use std::collections::HashMap;
    use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};

    use libc::{c_void, off_t};
    use libc::{ftruncate, mmap, shm_open, shm_unlink};
    use libc::{
        EEXIST, MAP_SHARED, O_CREAT, O_EXCL, O_RDWR, PROT_READ, PROT_WRITE, S_IRUSR, S_IWUSR,
    };

    pub use wayland_client::backend::ObjectId;
    use wayland_client::protocol::wl_buffer::WlBuffer;
    use wayland_client::protocol::wl_compositor::WlCompositor;
    use wayland_client::protocol::wl_output::WlOutput;
    use wayland_client::protocol::wl_shm::{Format, WlShm};
    use wayland_client::protocol::wl_shm_pool::WlShmPool;
    use wayland_client::protocol::wl_surface::WlSurface;
    use wayland_client::{EventQueue, Proxy, QueueHandle};

    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

    pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;

    use super::dispatcher::dispatcher::*;

    fn format_shm_name(attempt_number: u16) -> String {
        format!("/wl_shm_bell-{:08}\0", attempt_number)
    }

    pub struct MemoryPool {
        fd: RawFd,

        shm_pool_name: String,

        shared_memory_pool: Option<WlShmPool>,
        buffer: Option<WlBuffer>,

        width: i32,
        height: i32,
        bit_depth: u8,
        buffer_scale: i32,

        mmap_addr: Option<*mut u8>,
    }

    impl MemoryPool {
        fn new(
            width: i32,
            height: i32,
            bit_depth: u8,
            buffer_scale: i32,
            buffer_data: Option<BufferUserData>,
            wl_shared_memory: &WlShm,
            queue_handle: &QueueHandle<GlobalData>,
        ) -> MemoryPool {
            let mut new_mp = unsafe {
                let mut attempt_number: u16 = 0;
                let mut shm_pool_name;

                let mut fd;
                loop {
                    shm_pool_name = format_shm_name(attempt_number);
                    fd = shm_open(
                        shm_pool_name.as_ptr() as *const i8,
                        O_RDWR | O_CREAT | O_EXCL,
                        S_IRUSR | S_IWUSR,
                    );

                    if fd >= 0 {
                        break;
                    }

                    let error = std::io::Error::last_os_error();

                    if let Some(error_code) = error.raw_os_error() {
                        if error_code == EEXIST {
                            attempt_number += 1;

                            continue;
                        }
                    }

                    panic!("{}", error.to_string());
                }

                MemoryPool {
                    fd,
                    shm_pool_name,
                    shared_memory_pool: None,
                    buffer: None,
                    width,
                    height,
                    bit_depth,
                    buffer_scale,
                    mmap_addr: None,
                }
            };

            unsafe {
                ftruncate(new_mp.fd, new_mp.size() as off_t);

                let mut mmap_addr: *mut u8 = std::ptr::null_mut();
                mmap_addr = mmap(
                    mmap_addr as *mut c_void,
                    new_mp.size() as usize,
                    PROT_READ | PROT_WRITE,
                    MAP_SHARED,
                    new_mp.fd,
                    0,
                ) as *mut u8;

                new_mp.mmap_addr = Some(mmap_addr);
            }

            new_mp.create_shared_memory_pool(wl_shared_memory, queue_handle);
            new_mp.create_buffer(
                buffer_data.unwrap_or(BufferUserData { parent_id: None }),
                queue_handle,
            );

            return new_mp;
        }

        fn page_size(&self) -> i32 {
            self.width * self.height * (self.bit_depth as i32)
        }

        fn size(&self) -> i32 {
            self.page_size() * self.buffer_scale.pow(2)
        }

        fn create_shared_memory_pool(
            &mut self,
            wl_shared_memory: &WlShm,
            queue_handle: &QueueHandle<GlobalData>,
        ) {
            let wl_memory_pool =
                wl_shared_memory.create_pool(self.as_fd(), self.size(), &queue_handle, UserData {});

            self.shared_memory_pool = Some(wl_memory_pool);
        }

        fn create_buffer(
            &mut self,
            buffer_data: BufferUserData,
            queue_handle: &QueueHandle<GlobalData>,
        ) {
            match self.shared_memory_pool.as_ref() {
                Some(shm_pool) => {
                    self.buffer = Some(shm_pool.create_buffer(
                        0,
                        self.width * self.buffer_scale,
                        self.height * self.buffer_scale,
                        self.width * self.buffer_scale * self.bit_depth as i32,
                        Format::Argb8888,
                        &queue_handle,
                        buffer_data,
                    ))
                }
                None => {
                    eprintln!("Failed to create buffer for memory pool.")
                }
            }
        }
    }

    impl Drop for MemoryPool {
        fn drop(&mut self) {
            unsafe {
                shm_unlink(self.shm_pool_name.as_ptr() as *const i8);
            }

            if let Some(buffer) = &self.buffer {
                buffer.destroy();
            }

            if let Some(shm_pool) = &self.shared_memory_pool {
                shm_pool.destroy();
            }
        }
    }

    impl AsFd for MemoryPool {
        fn as_fd(&self) -> BorrowedFd<'_> {
            unsafe { BorrowedFd::borrow_raw(self.fd) }
        }
    }

    pub enum SurfaceBackend {
        Wlr(ZwlrLayerSurfaceV1),
    }

    pub struct Surface {
        id: ObjectId,
        destroyed: bool,

        wl_surface: WlSurface,

        pub backend: SurfaceBackend,

        memory_pool: Option<MemoryPool>,

        pub output_name: String,
        pub surface_width: i32,
        pub surface_height: i32,

        pub is_dirty: bool,

        ready_to_draw: bool,
        draw_is_requested: bool,
        is_configured: bool,
    }

    impl Surface {
        pub fn new(
            width: i32,
            height: i32,
            output: Option<(String, Option<&WlOutput>)>,
            wl_compositor: &WlCompositor,
            wlr_layer: Option<&ZwlrLayerShellV1>,
            queue_handle: &QueueHandle<GlobalData>,
        ) -> Surface {
            let wl_surface = wl_compositor.create_surface(queue_handle, WlSurfaceUserData {});

            let id = wl_surface.id();

            let output = output.unwrap_or(("".to_owned(), None));

            let surface_backend: SurfaceBackend;
            if let Some(wlr_layer) = wlr_layer {
                let wlr_surface = wlr_layer.get_layer_surface(
                    &wl_surface,
                    output.1,
                    Layer::Overlay,
                    "".to_owned(),
                    queue_handle,
                    WlrUserData {
                        parent_id: id.clone(),
                    },
                );
                wlr_surface.set_size(width as u32, height as u32);

                // NOTE: The documentation says we have to do this:
                // https://wayland.app/protocols/wlr-layer-shell-unstable-v1#zwlr_layer_shell_v1:request:get_layer_surface
                wl_surface.commit();

                surface_backend = SurfaceBackend::Wlr(wlr_surface);
            } else {
                panic!("Tried to create a surface without a valid backend!");
            }

            Surface {
                id,
                destroyed: false,
                wl_surface,
                backend: surface_backend,
                memory_pool: None,
                output_name: output.0,
                surface_width: width,
                surface_height: height,
                is_dirty: true,
                ready_to_draw: true,
                draw_is_requested: false,
                is_configured: false,
            }
        }

        pub fn set_buffer_scale(
            &mut self,
            scale_factor: i32,
            wl_shm: &mut WlShm,
            queue_handle: &QueueHandle<GlobalData>,
        ) {
            if self.memory_pool.is_some() && scale_factor == self.get_buffer_scale() {
                return;
            }

            let buffer_data = BufferUserData {
                parent_id: Some(self.id.clone()),
            };
            let memory_pool = MemoryPool::new(
                self.surface_width,
                self.surface_height,
                4,
                scale_factor,
                Some(buffer_data),
                wl_shm,
                queue_handle,
            );

            let buffer_contents = memory_pool.mmap_addr.unwrap();
            assert_ne!(buffer_contents as usize, 0);

            self.wl_surface.set_buffer_scale(scale_factor);

            self.memory_pool = Some(memory_pool);

            self.is_dirty = true;
        }

        pub fn get_buffer_scale(&self) -> i32 {
            match self.memory_pool.as_ref() {
                Some(memory_pool) => memory_pool.buffer_scale,
                None => 1,
            }
        }

        pub fn destroy(&mut self) {
            if self.destroyed {
                return;
            }

            match &mut self.backend {
                SurfaceBackend::Wlr(surface) => {
                    surface.destroy();
                }
            }

            self.wl_surface.destroy();

            self.destroyed = true;
        }

        pub fn is_destroyed(&self) -> bool {
            self.destroyed
        }

        pub fn write(&mut self, data: &[u32]) {
            let memory_pool = self.memory_pool.as_mut();
            if memory_pool.is_none() {
                println!("Attempted to write data without a memory pool.");
                return;
            }

            let memory_pool = memory_pool.unwrap();
            let addr = memory_pool.mmap_addr.unwrap();

            unsafe {
                for (idx, data_point) in data.iter().enumerate() {
                    let data_point_le = data_point.to_le_bytes();

                    *addr.offset((4 * idx + 0) as isize) = data_point_le[0];
                    *addr.offset((4 * idx + 1) as isize) = data_point_le[1];
                    *addr.offset((4 * idx + 2) as isize) = data_point_le[2];
                    *addr.offset((4 * idx + 3) as isize) = data_point_le[3];
                }
            }

            self.request_draw();
        }

        pub fn set_ready_to_draw(&mut self, queue_handle: &QueueHandle<GlobalData>) {
            self.ready_to_draw = true;

            // Configure next callback
            self.wl_surface.frame(queue_handle, UserData {});

            if self.draw_is_requested && self.is_configured {
                self.draw();
            }
        }

        pub fn set_configured(&mut self) {
            self.is_configured = true;

            if self.ready_to_draw {
                self.draw();
            }
        }

        fn attach(&mut self) {
            if self.memory_pool.is_none() {
                println!("Attempted to attach a buffer without a memory pool.");
                return;
            }

            let memory_pool = self.memory_pool.as_mut().unwrap();

            match memory_pool.buffer.as_mut() {
                Some(buffer) => {
                    self.wl_surface.attach(Some(&buffer), 0, 0);
                }
                None => {
                    panic!("No buffer available for attach.");
                }
            }
        }

        fn request_draw(&mut self) {
            self.draw_is_requested = true;

            if self.ready_to_draw && self.is_configured {
                self.draw();
            }
        }

        fn draw(&mut self) {
            self.ready_to_draw = false;
            self.draw_is_requested = false;

            self.attach();

            self.wl_surface.damage_buffer(
                0,
                0,
                self.surface_width * self.get_buffer_scale(),
                self.surface_height * self.get_buffer_scale(),
            );

            self.wl_surface.commit();

            self.is_dirty = false;
        }
    }

    pub struct GlobalData {
        pub compositor: Option<WlCompositor>,
        pub wlr_layer_shell: Option<ZwlrLayerShellV1>,
        pub shared_memory: Option<WlShm>,

        pub queue_handle: QueueHandle<Self>,

        pub surfaces: HashMap<ObjectId, Surface>,
        pub outputs: HashMap<String, WlOutput>,
    }

    impl GlobalData {
        pub fn new(queue_handle: QueueHandle<Self>) -> Cell<GlobalData> {
            Cell::new(GlobalData {
                compositor: None,
                wlr_layer_shell: None,
                shared_memory: None,
                queue_handle,
                surfaces: HashMap::new(),
                outputs: HashMap::new(),
            })
        }

        pub fn create_surface(
            &mut self,
            width: i32,
            height: i32,
            output_name: &String,
        ) -> Option<ObjectId> {
            if self.compositor.is_none() {
                return None;
            }
            if self.shared_memory.is_none() {
                return None;
            }
            if self.wlr_layer_shell.is_none() {
                return None;
            }

            let mut surface = Surface::new(
                width,
                height,
                Some((output_name.clone(), self.outputs.get(output_name))),
                self.compositor.as_ref().unwrap(),
                self.wlr_layer_shell.as_ref(),
                &self.queue_handle,
            );

            surface.set_buffer_scale(1, self.shared_memory.as_mut().unwrap(), &self.queue_handle);

            let id = surface.id.clone();

            self.surfaces.insert(id.clone(), surface);
            return Some(id);
        }

        pub fn get_surface(&mut self, id: &ObjectId) -> Option<&mut Surface> {
            self.surfaces.get_mut(id)
        }

        pub fn destroy_surface(&mut self, id: &ObjectId) {
            let surface_maybe = self.surfaces.remove(id);

            match surface_maybe {
                Some(mut surface) => {
                    surface.destroy();
                }
                None => {}
            }
        }
    }

    use super::epoll::epoll;

    pub struct SocketManager<'a> {
        epoll_fd: RawFd,

        event_queue: &'a mut EventQueue<GlobalData>,
        global_data: *mut GlobalData,
    }

    impl<'a> SocketManager<'a> {
        pub fn new(
            event_queue: &'a mut EventQueue<GlobalData>,
            global_data: &mut Cell<GlobalData>,
        ) -> SocketManager<'a> {
            let epoll_fd = epoll::create();

            SocketManager {
                epoll_fd,
                event_queue,
                global_data: global_data.as_ptr(),
            }
        }

        pub fn handle(&mut self, timeout: i32) {
            let mut global_data = unsafe { self.global_data.as_mut().unwrap() };

            self.event_queue.flush().unwrap();
            self.event_queue.dispatch_pending(&mut global_data).unwrap();

            let read_guard = self.event_queue.prepare_read().unwrap();
            let fd = read_guard.connection_fd().as_raw_fd();

            let received =
                epoll::wait_on_fds(self.epoll_fd, [fd].to_vec(), Some(epoll::EPOLLIN), timeout);

            let mut socket_ready = false;
            if let Ok(mapping) = received {
                socket_ready = mapping.contains(&fd);
            }

            if socket_ready {
                if let Ok(_) = read_guard.read() {
                    self.event_queue.dispatch_pending(&mut global_data).unwrap();
                }
            } else {
                std::mem::drop(read_guard);
            }
        }
    }

    impl Drop for SocketManager<'_> {
        fn drop(&mut self) {
            epoll::close(self.epoll_fd);
        }
    }
}
