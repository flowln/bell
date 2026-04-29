use crate::configuration::OutputConfiguration;
use crate::wayland::wayland as backend;

use std::collections::HashMap;

pub struct Notification<'n> {
    pub title: String,
    pub message: String,

    pub is_dirty: bool,

    outputs: HashMap<String, &'n OutputConfiguration>,
    surface_ids: Vec<backend::ObjectId>,
}

impl<'n> Notification<'n> {
    pub fn new(title: String, message: String) -> Notification<'n> {
        Notification {
            title,
            message,
            is_dirty: true,
            outputs: HashMap::new(),
            surface_ids: Vec::new(),
        }
    }

    pub fn set_message(&mut self, message: String) {
        if message == self.message {
            return;
        }

        self.message = message;
        self.is_dirty = true;
    }

    pub fn add_output(&mut self, output_name: &String, spec: &'n OutputConfiguration) {
        self.outputs.insert(output_name.clone(), spec);
    }

    pub fn create_surfaces(&mut self, global_data: &mut backend::GlobalData) {
        for (output_name, spec) in self.outputs.iter() {
            let surface_id = global_data
                .create_surface(spec.width.unwrap(), spec.height.unwrap(), &output_name)
                .unwrap();

            // FIXME: This shouldn't be here probably.
            let surface = global_data.get_surface(&surface_id).unwrap();
            match &mut surface.backend {
                backend::SurfaceBackend::Wlr(wlr_surface) => {
                    if let Some(anchor) = spec.anchor {
                        wlr_surface.set_anchor(anchor);
                    }

                    if let Some(layer) = spec.layer {
                        wlr_surface.set_layer(layer);
                    }

                    let margins = spec.margins.unwrap();
                    wlr_surface.set_margin(
                        margins.top,
                        margins.right,
                        margins.bottom,
                        margins.left,
                    );
                }
                _ => {}
            }

            self.surface_ids.push(surface_id);
        }
    }

    pub fn delete_surface(&mut self, surface_id: &backend::ObjectId) {
        self.surface_ids.retain(|id| id != surface_id);
    }

    pub fn has_any_surface(&self) -> bool {
        !self.surface_ids.is_empty()
    }

    pub fn for_each_surface<F>(&self, mut closure: F)
    where
        F: FnMut(&backend::ObjectId) -> (),
    {
        for id in self.surface_ids.iter() {
            closure(id);
        }
    }

    pub fn expire(&mut self) {
        for id in self.surface_ids.clone().iter() {
            self.delete_surface(id);
        }
    }

    pub fn get_output_spec(&self, output_name: &String) -> Option<&OutputConfiguration> {
        self.outputs.get(output_name).map(|&c| c)
    }
}

pub enum SurfaceProcessingOutput {
    Continue,
    NoSurface,
    SurfaceDestroyed,
}

pub struct NotificationManager<'m> {
    active_notifications: Vec<Notification<'m>>,
}

impl<'m> NotificationManager<'m> {
    pub fn new() -> NotificationManager<'m> {
        NotificationManager {
            active_notifications: Vec::new(),
        }
    }

    pub fn add_notification(&mut self, notification: Notification<'m>) {
        self.active_notifications.push(notification)
    }

    pub fn remove_by_index(&mut self, index: usize) -> Notification<'_> {
        self.active_notifications.remove(index)
    }

    pub fn get_by_index(&mut self, index: usize) -> Option<&mut Notification<'m>> {
        self.active_notifications.get_mut(index)
    }

    pub fn is_active(&self) -> bool {
        !self.active_notifications.is_empty()
    }

    pub fn process_active_notifications<F>(
        &mut self,
        mut process_surface: F,
    ) -> Vec<Notification<'_>>
    where
        F: FnMut(
            &backend::ObjectId,
            &Notification,
            &mut HashMap<String, i32>,
        ) -> SurfaceProcessingOutput,
    {
        let mut indexes_to_remove = Vec::<usize>::new();
        let mut offset_per_output = HashMap::<String, i32>::new();

        for (idx, notification) in self.active_notifications.iter_mut().enumerate().rev() {
            let mut surfaces_to_delete = Vec::<backend::ObjectId>::new();

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
                indexes_to_remove.push(idx);
            } else {
                notification.is_dirty = false;
            }
        }

        let mut inactive_notifications = Vec::<Notification>::new();
        for idx in indexes_to_remove {
            inactive_notifications.push(self.active_notifications.remove(idx));
        }

        inactive_notifications
    }
}
