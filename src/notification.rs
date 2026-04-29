use crate::wayland::wayland as backend;
use crate::render::Color;

use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct Margin {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl Margin {
    pub fn with_top(&self, top: i32) -> Margin {
        let mut ret = self.clone();
        ret.top = top;
        ret
    }
    pub fn with_right(&self, right: i32) -> Margin {
        let mut ret = self.clone();
        ret.right = right;
        ret
    }
    pub fn with_bottom(&self, bottom: i32) -> Margin {
        let mut ret = self.clone();
        ret.bottom = bottom;
        ret
    }
    pub fn with_left(&self, left: i32) -> Margin {
        let mut ret = self.clone();
        ret.left = left;
        ret
    }
}

pub enum GrowthDirection {
    Up,
    Right,
    Down,
    Left,
}

pub struct OutputSpecifier {
    pub width: i32,
    pub height: i32,

    pub background_color: Color,

    pub border_color: Option<Color>,
    pub border_size: usize,

    pub anchor: Option<backend::Anchor>,
    pub direction: GrowthDirection,

    pub layer: Option<backend::Layer>,

    pub margins: Margin,
}

impl OutputSpecifier {
    pub fn new(width: i32, height: i32) -> OutputSpecifier {
        OutputSpecifier {
            width,
            height,
            background_color: Color::rgba(0x80, 0x80, 0x80, 0xFF),
            border_color: None,
            border_size: 0,
            anchor: None,
            direction: GrowthDirection::Up,
            layer: None,
            margins: Margin {
                left: 0,
                right: 0,
                top: 0,
                bottom: 0,
            },
        }
    }

    pub fn set_anchor(&mut self, anchor: backend::Anchor) {
        self.anchor.replace(anchor);
    }

    pub fn set_growth_direction(&mut self, direction: GrowthDirection) {
        self.direction = direction;
    }

    pub fn set_layer(&mut self, layer: backend::Layer) {
        self.layer.replace(layer);
    }

    pub fn set_margins(
        &mut self,
        left: i32,
        right: i32,
        top: i32,
        bottom: i32,
    ) {
        self.margins.left = left;
        self.margins.right = right;
        self.margins.top = top;
        self.margins.bottom = bottom;
    }
}

pub struct Notification {
    pub title: String,
    pub message: String,

    pub is_dirty: bool,

    outputs: HashMap<String, OutputSpecifier>,
    surface_ids: Vec<backend::ObjectId>,
}

impl Notification {
    pub fn new(title: String, message: String) -> Notification {
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

    pub fn add_output(&mut self, output_name: &String, spec: OutputSpecifier) {
        self.outputs.insert(output_name.clone(), spec);
    }

    pub fn create_surfaces(&mut self, global_data: &mut backend::GlobalData) {
        for (output_name, spec) in self.outputs.iter() {
            let surface_id = global_data
                .create_surface(spec.width, spec.height, &output_name)
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

                    wlr_surface.set_margin(
                        spec.margins.top,
                        spec.margins.right,
                        spec.margins.bottom,
                        spec.margins.left,
                    );
                }
                _ => {}
            }

            self.surface_ids.push(surface_id);
        }
    }

    pub fn delete_surface(
        &mut self,
        surface_id: &backend::ObjectId,
    ) {
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

    pub fn get_output_spec(&self, output_name: &String) -> Option<&OutputSpecifier> {
        self.outputs.get(output_name)
    }
}

pub enum SurfaceProcessingOutput {
    Continue,
    NoSurface,
    SurfaceDestroyed,
}

pub struct NotificationManager {
    active_notifications: Vec<Notification>,
}

impl NotificationManager {
    pub fn new() -> NotificationManager {
        NotificationManager { active_notifications: Vec::new() }
    }

    pub fn add_notification(&mut self, notification: Notification) {
        self.active_notifications.push(notification)
    }

    pub fn remove_by_index(&mut self, index: usize) -> Notification {
        self.active_notifications.remove(index)
    }

    pub fn get_by_index(&mut self, index: usize) -> Option<&mut Notification> {
        self.active_notifications.get_mut(index)
    }

    pub fn is_active(&self) -> bool {
        !self.active_notifications.is_empty()
    }

    pub fn process_active_notifications<F>(&mut self, mut process_surface: F) -> Vec<Notification>
        where F: FnMut(&backend::ObjectId, &Notification, &mut HashMap<String, i32>) -> SurfaceProcessingOutput
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
            inactive_notifications.push(self.remove_by_index(idx));
        }

        inactive_notifications
    }
}
