mod dispatcher;
mod epoll;

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::sync::{OnceLock, RwLock};

use libc::{EEXIST, MAP_SHARED, O_CREAT, O_EXCL, O_RDWR, PROT_READ, PROT_WRITE, S_IRUSR, S_IWUSR};
use libc::{c_void, off_t};
use libc::{ftruncate, mmap, munmap, shm_open, shm_unlink};

pub use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_display::WlDisplay;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::{Format, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{EventQueue, Proxy, QueueHandle};

use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;

use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;

pub use crate::configuration::EventTrigger;
use dispatcher::dispatcher::*;

impl TryFrom<LinuxButtonCode> for EventTrigger {
    type Error = u32;

    fn try_from(code: LinuxButtonCode) -> Result<Self, Self::Error> {
        match code {
            LinuxButtonCode::Left => Ok(EventTrigger::OnLeftClick),
            LinuxButtonCode::Right => Ok(EventTrigger::OnRightClick),
            LinuxButtonCode::Middle => Ok(EventTrigger::OnMiddleClick),
            LinuxButtonCode::Unknown(raw_code) => Err(raw_code),
        }
    }
}

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

    mmap_addr: Option<usize>,
}

impl MemoryPool {
    fn new(
        width: i32,
        height: i32,
        bit_depth: u8,
        buffer_scale: i32,
        buffer_data: Option<BufferUserData>,
        wl_shared_memory: &WlShm,
        queue_handle: &QueueHandle<WaylandState>,
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

            new_mp.mmap_addr = Some(mmap_addr as usize);
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
        queue_handle: &QueueHandle<WaylandState>,
    ) {
        let wl_memory_pool =
            wl_shared_memory.create_pool(self.as_fd(), self.size(), &queue_handle, UserData {});

        self.shared_memory_pool = Some(wl_memory_pool);
    }

    fn create_buffer(
        &mut self,
        buffer_data: BufferUserData,
        queue_handle: &QueueHandle<WaylandState>,
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
        if let Some(buffer) = &self.buffer {
            buffer.destroy();
        }

        if let Some(shm_pool) = &self.shared_memory_pool {
            shm_pool.destroy();
        }

        unsafe {
            shm_unlink(self.shm_pool_name.as_ptr() as *const i8);

            if let Some(addr) = self.mmap_addr {
                munmap(addr as *mut c_void, self.size() as usize);
            }
        }
    }
}

impl AsFd for MemoryPool {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.fd) }
    }
}

pub type SurfaceID = ObjectId;

pub enum SurfaceBackend {
    Wlr(ZwlrLayerSurfaceV1),
}

pub struct Surface {
    id: SurfaceID,

    destruction_scheduled: bool,
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
        wayland_state: &mut WaylandState,
        width: i32,
        height: i32,
        output_name: Option<String>,
    ) -> Surface {
        let queue_handle = wayland_state.queue_handle.as_ref().unwrap();

        let wl_compositor = wayland_state.compositor.as_ref().unwrap();
        let wl_surface = wl_compositor.create_surface(queue_handle, WlSurfaceUserData {});
        let wlr_layer = wayland_state.wlr_layer_shell.as_ref();

        let id = wl_surface.id();

        let output_name = output_name.unwrap_or_default();
        let output = wayland_state.get_output_by_name(output_name.as_str());

        let surface_backend: SurfaceBackend;
        if let Some(wlr_layer) = wlr_layer {
            let wlr_surface = wlr_layer.get_layer_surface(
                &wl_surface,
                output,
                Layer::Top,
                "bell".to_owned(),
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
            destruction_scheduled: false,
            destroyed: false,
            wl_surface,
            backend: surface_backend,
            memory_pool: None,
            output_name,
            surface_width: width,
            surface_height: height,
            is_dirty: true,
            ready_to_draw: true,
            draw_is_requested: false,
            is_configured: false,
        }
    }

    pub fn set_buffer_scale(&mut self, wayland_state: &mut WaylandState, scale_factor: i32) {
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
            &wayland_state.shared_memory.as_ref().unwrap(),
            &wayland_state.queue_handle.as_ref().unwrap(),
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

    pub fn handle_pointer_event(
        &mut self,
        wayland_state: &mut WaylandState,
        button_code: LinuxButtonCode,
        button_state: ButtonState,
    ) {
        match (EventTrigger::try_from(button_code), button_state) {
            (Err(code), _) => {
                eprintln!("Surface: Got unknown button code '{code}'.");
            }
            (Ok(trigger_event), ButtonState::Pressed) => {
                wayland_state.insert_surface_event(&self.id, trigger_event);
            }
            _ => {}
        }
    }

    /// Mark surface for destruction on the next draw.
    pub fn destroy_later(&mut self) {
        if self.destroyed {
            return;
        }

        self.destruction_scheduled = true;
    }

    pub fn will_destroy_later(&self) -> bool {
        self.destruction_scheduled
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
        let addr = memory_pool.mmap_addr.unwrap() as *mut u8;

        unsafe {
            addr.copy_from_nonoverlapping(data.as_ptr().cast(), data.len() * 4);
        }

        self.request_draw();
    }

    pub fn set_ready_to_draw(&mut self, _queue_handle: &QueueHandle<WaylandState>) {
        self.ready_to_draw = true;

        if self.draw_is_requested && self.is_configured {
            self.draw();
        }
    }

    pub fn set_configured(&mut self) {
        self.is_configured = true;

        if self.draw_is_requested && self.ready_to_draw {
            self.draw();
        }
    }

    pub fn request_draw(&mut self) {
        self.draw_is_requested = true;

        if self.ready_to_draw && self.is_configured {
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

    fn draw(&mut self) {
        if self.destruction_scheduled {
            self.destroy_now();

            return;
        }

        self.wl_surface.damage_buffer(
            0,
            0,
            self.surface_width * self.get_buffer_scale(),
            self.surface_height * self.get_buffer_scale(),
        );

        self.ready_to_draw = false;
        self.draw_is_requested = false;

        self.attach();
        self.wl_surface.commit();

        self.is_dirty = false;
    }

    fn destroy_now(&mut self) {
        match &mut self.backend {
            SurfaceBackend::Wlr(surface) => {
                surface.destroy();
            }
        }

        self.wl_surface.destroy();

        self.destruction_scheduled = false;
        self.destroyed = true;
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        if !self.destroyed {
            self.destroy_now();
        }
    }
}

static WAYLAND_STATE: RwLock<WaylandState> = RwLock::new(WaylandState::new());

use crate::generate_rw_accessors;
generate_rw_accessors!(WAYLAND_STATE WAYLAND_WRITE_BACKTRACE wayland_state_read wayland_state_write wayland_state_panic WaylandState);

pub struct WaylandState {
    pub pending_data_amount: usize,

    pub compositor: Option<WlCompositor>,
    pub wlr_layer_shell: Option<ZwlrLayerShellV1>,
    pub shared_memory: Option<WlShm>,
    pub seats: Vec<WlSeat>,

    pub cursor_shape_manager: Option<WpCursorShapeManagerV1>,

    pub queue_handle: Option<QueueHandle<Self>>,

    trigger_events: OnceLock<HashMap<SurfaceID, Vec<EventTrigger>>>,

    surfaces: OnceLock<HashMap<SurfaceID, Surface>>,
    outputs: OnceLock<HashMap<String, WlOutput>>,
}

impl WaylandState {
    const fn new() -> WaylandState {
        WaylandState {
            pending_data_amount: 1,
            compositor: None,
            wlr_layer_shell: None,
            shared_memory: None,
            seats: Vec::new(),
            cursor_shape_manager: None,
            queue_handle: None,
            trigger_events: OnceLock::new(),
            surfaces: OnceLock::new(),
            outputs: OnceLock::new(),
        }
    }

    pub fn initialize(&mut self, queue_handle: QueueHandle<Self>, display: &WlDisplay) {
        display.sync(&queue_handle, DisplayUserData {});

        self.queue_handle = Some(queue_handle);

        self.trigger_events.get_or_init(|| HashMap::new());
        self.surfaces.get_or_init(|| HashMap::new());
        self.outputs.get_or_init(|| HashMap::new());
    }

    pub fn get_output_by_name(&self, name: &str) -> Option<&WlOutput> {
        self.outputs.get()?.get(name)
    }

    pub fn get_output_names(&self) -> impl ExactSizeIterator<Item = &String> {
        self.outputs.get().unwrap().keys()
    }

    pub fn add_output(&mut self, output_name: String, output: WlOutput) {
        self.outputs.get_mut().unwrap().insert(output_name, output);
    }

    pub fn consume_trigger_events(&mut self) -> &mut HashMap<SurfaceID, Vec<EventTrigger>> {
        self.trigger_events.get_mut().unwrap()
    }

    pub fn create_surface(
        &mut self,
        width: i32,
        height: i32,
        output_name: &String,
    ) -> Option<SurfaceID> {
        if self.compositor.is_none() {
            return None;
        }
        if self.shared_memory.is_none() {
            return None;
        }
        if self.wlr_layer_shell.is_none() {
            return None;
        }

        let mut surface = Surface::new(self, width, height, Some(output_name.clone()));

        surface.set_buffer_scale(self, 1);

        let id = surface.id.clone();
        self.surfaces.get_mut().unwrap().insert(id.clone(), surface);
        return Some(id);
    }

    pub fn get_surface(&mut self, id: &SurfaceID) -> Option<&mut Surface> {
        self.surfaces.get_mut()?.get_mut(id)
    }

    pub fn with_surface<F>(&mut self, id: &SurfaceID, mut closure: F) -> Option<()>
    where
        F: FnMut(&mut WaylandState, &mut Surface) -> (),
    {
        // Take it out of the map so we don't keep a mutable borrow to self
        let (id_mov, mut surface) = self.surfaces.get_mut()?.remove_entry(id)?;
        closure(self, &mut surface);
        self.surfaces.get_mut()?.insert(id_mov, surface);

        Some(())
    }

    pub fn mark_surface_for_destruction(&mut self, id: &SurfaceID) {
        let surface_maybe = self.surfaces.get_mut().unwrap().get_mut(id);

        match surface_maybe {
            Some(surface) => {
                surface.destroy_later();

                self.dirty_all_surfaces();
            }
            None => {}
        }
    }

    pub fn destroy_scheduled_surfaces(&mut self) {
        // Remove the reference so the Surface object is dropped.
        self.surfaces
            .get_mut()
            .unwrap()
            .retain(|_, surface| !surface.will_destroy_later());
    }

    fn dirty_all_surfaces(&mut self) {
        let surfaces = self.surfaces.get_mut().unwrap();
        for surface in surfaces.values_mut() {
            surface.is_dirty = true;
        }
    }

    fn insert_surface_event(&mut self, id: &SurfaceID, event: EventTrigger) {
        let trigger_events = self.trigger_events.get_mut().unwrap();
        if !trigger_events.contains_key(id) {
            trigger_events.insert(id.clone(), Vec::new());
        }
        trigger_events.get_mut(id).unwrap().push(event);
    }
}

pub struct SocketManager {
    epoll_fd: RawFd,

    pub event_queue: EventQueue<WaylandState>,
}

impl SocketManager {
    pub fn new(event_queue: EventQueue<WaylandState>) -> SocketManager {
        let epoll_fd = epoll::create();

        let manager = SocketManager {
            epoll_fd,
            event_queue,
        };

        manager
    }

    pub fn wait_on_socket_ready(&mut self, timeout: std::time::Duration) -> bool {
        let read_guard = self.event_queue.prepare_read().unwrap();
        let fd = read_guard.connection_fd().as_raw_fd();

        let received = epoll::wait_on_fds(
            self.epoll_fd,
            [fd].to_vec(),
            Some(epoll::EPOLLIN),
            timeout.as_millis() as i32,
        );

        let mut socket_ready = false;
        if let Ok(mapping) = received {
            socket_ready = mapping.contains(&fd);
        }

        if socket_ready {
            if let Ok(read_count) = read_guard.read() {
                read_count != 0
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn flush(&mut self) {
        self.event_queue.flush().unwrap();
    }

    pub fn dispatch_events_in_queue(&mut self, wayland_state: &mut WaylandState) -> usize {
        self.event_queue.dispatch_pending(wayland_state).unwrap()
    }
}

impl Drop for SocketManager {
    fn drop(&mut self) {
        self.flush();

        epoll::close(self.epoll_fd);
    }
}
