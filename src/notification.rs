use crate::configuration::{Configuration, EventResponse, OutputConfiguration};
use crate::wayland::{SurfaceBackend, SurfaceID, wayland_state_read, wayland_state_write};

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, RwLock};

pub struct Notification {
    pub app_name: String,
    pub summary: String,
    pub body: String,

    pub is_dirty: bool,

    outputs: HashMap<String, Arc<OutputConfiguration>>,
    surface_ids: Vec<SurfaceID>,
}

impl Notification {
    pub fn new(app_name: String, summary: String, body: Option<String>) -> Notification {
        Notification {
            app_name,
            summary,
            body: body.unwrap_or_default(),
            is_dirty: true,
            outputs: HashMap::new(),
            surface_ids: Vec::new(),
        }
    }

    pub fn try_make_surfaces(&mut self, configuration: &Configuration) -> Option<()> {
        {
            let r_wayland_state = wayland_state_read();
            let available_output_names = r_wayland_state.get_output_names();

            for output_name in available_output_names {
                let output_configuration = configuration.get_output_configuration(output_name);
                self.try_add_output(output_name, output_configuration);
            }
        }

        let mut w_wayland_state = wayland_state_write();
        for (output_name, spec) in self.outputs.iter() {
            let surface_id =
                w_wayland_state.create_surface(spec.width?, spec.height?, output_name)?;

            // FIXME: This shouldn't be here probably.
            let surface = w_wayland_state.get_surface(&surface_id)?;
            match &mut surface.backend {
                SurfaceBackend::Wlr(wlr_surface) => {
                    if let Some(anchor) = spec.anchor {
                        wlr_surface.set_anchor(anchor);
                    }

                    if let Some(layer) = spec.layer {
                        wlr_surface.set_layer(layer);
                    }

                    let margins = spec.margins?;
                    wlr_surface.set_margin(
                        margins.top,
                        margins.right,
                        margins.bottom,
                        margins.left,
                    );
                }
            }

            self.surface_ids.push(surface_id);
        }

        Some(())
    }

    pub fn try_handle_event(&mut self, event: &EventResponse) -> bool {
        match event {
            EventResponse::CloseNotification => {
                self.expire();
            }
            _ => {
                return false;
            }
        };

        true
    }

    pub fn delete_surface(&mut self, surface_id: &SurfaceID) {
        self.surface_ids.retain(|id| id != surface_id);

        let mut wayland_state = wayland_state_write();
        wayland_state.mark_surface_for_destruction(surface_id);
    }

    pub fn has_any_surface(&self) -> bool {
        !self.surface_ids.is_empty()
    }

    pub fn for_each_surface<F>(&self, mut closure: F)
    where
        F: FnMut(&SurfaceID) -> (),
    {
        for id in self.surface_ids.iter() {
            closure(id);
        }
    }

    pub fn for_each_surface_mut<F>(&mut self, mut closure: F)
    where
        F: FnMut(&mut Self, &SurfaceID) -> (),
    {
        for id in self.surface_ids.clone().iter() {
            closure(self, id);
        }
    }

    pub fn get_output_spec(&self, output_name: &String) -> Option<&OutputConfiguration> {
        self.outputs
            .get(output_name)
            .map(|c| unsafe { &*Arc::as_ptr(c) })
    }

    fn expire(&mut self) {
        let ids = self.surface_ids.clone();
        for id in ids.iter() {
            self.delete_surface(id);
        }
    }

    fn try_add_output(&mut self, output_name: &String, spec: Arc<OutputConfiguration>) {
        self.outputs.insert(output_name.clone(), spec);
    }
}

pub enum SurfaceProcessingOutput {
    Continue,
    NoSurface,
    SurfaceDestroyed,
}

pub static NOTIFICATION_MANAGER: RwLock<NotificationManager> =
    RwLock::new(NotificationManager::new());

use crate::generate_rw_accessors;
generate_rw_accessors!(NOTIFICATION_MANAGER NOTIFICATION_WRITE_BACKTRACE notification_manager_read notification_manager_write notification_manager_panic NotificationManager);

pub struct NotificationManager {
    biggest_id_given: u32,

    active_configuration: Option<Configuration>,
    active_notifications: LazyLock<BTreeMap<u32, Notification>>,
}

impl NotificationManager {
    const fn new() -> NotificationManager {
        NotificationManager {
            biggest_id_given: 0,
            active_configuration: None,
            active_notifications: LazyLock::new(|| BTreeMap::new()),
        }
    }

    pub fn set_configuration(&mut self, configuration: Configuration) {
        self.active_configuration = Some(configuration);
    }

    pub fn get_configuration(&mut self) -> Option<&Configuration> {
        self.active_configuration.as_ref()
    }

    pub fn add_notification(&mut self, notification: Notification) -> u32 {
        self.biggest_id_given += 1;
        self.active_notifications
            .insert(self.biggest_id_given, notification);
        self.biggest_id_given
    }

    pub fn replace_notification(&mut self, id: u32, notification: Notification) {
        self.active_notifications.insert(id, notification);
    }

    pub fn is_active(&self) -> bool {
        !self.active_notifications.is_empty()
    }

    pub fn process_active_notifications<F>(
        &mut self,
        event_queue: &HashMap<SurfaceID, Vec<EventResponse>>,
        process_surface: &mut F,
    ) -> Vec<Notification>
    where
        F: FnMut(&SurfaceID, &Notification, &mut HashMap<String, i32>) -> SurfaceProcessingOutput,
    {
        let mut ids_to_remove = Vec::<u32>::new();
        let mut offset_per_output = HashMap::<String, i32>::new();

        for (id, notification) in self.active_notifications.iter_mut() {
            let mut surfaces_to_delete = Vec::<SurfaceID>::new();

            notification.for_each_surface_mut(|notif, surface_id| {
                if let Some(trigger_list) = event_queue.get(surface_id) {
                    for trigger in trigger_list {
                        notif.try_handle_event(trigger);
                    }
                }
            });

            notification.for_each_surface(|surface_id| {
                match process_surface(surface_id, notification, &mut offset_per_output) {
                    SurfaceProcessingOutput::Continue => {}
                    SurfaceProcessingOutput::NoSurface => {
                        println!("No surface is available with id {surface_id:?}.");
                        surfaces_to_delete.push(surface_id.clone());
                    }
                    SurfaceProcessingOutput::SurfaceDestroyed => {
                        println!("Surface with id {surface_id:?} has been destroyed.");
                        surfaces_to_delete.push(surface_id.clone());
                    }
                }
            });

            for surface_id in surfaces_to_delete {
                notification.delete_surface(&surface_id);
            }

            if !notification.has_any_surface() {
                ids_to_remove.push(*id);
            } else {
                notification.is_dirty = false;
            }
        }

        let mut inactive_notifications = Vec::<Notification>::new();
        for id in ids_to_remove {
            inactive_notifications.push(self.active_notifications.remove(&id).unwrap());
        }

        inactive_notifications
    }
}
