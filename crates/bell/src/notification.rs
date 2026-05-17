use crate::configuration::{Configuration, EventResponse, EventTrigger, OutputConfiguration};
use crate::dbus::ImageData;
use crate::wayland::{SurfaceBackend, SurfaceID, wayland_state_read, wayland_state_write};

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Condvar, LazyLock, Mutex, RwLock};
use std::time;

#[derive(Debug, Eq, PartialEq)]
pub enum NotificationUrgency {
    Low,
    Normal,
    Critical,
}

impl From<u8> for NotificationUrgency {
    fn from(byte: u8) -> Self {
        match byte {
            0 => Self::Low,
            1 => Self::Normal,
            2 => Self::Critical,
            other => {
                eprintln!("Got unexpected urgency level '{other}'.");
                Self::Normal
            }
        }
    }
}

impl ToString for NotificationUrgency {
    fn to_string(&self) -> String {
        String::from(match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::Critical => "Critical",
        })
    }
}

pub struct Notification {
    pub id: Option<u32>,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub urgency: NotificationUrgency,

    pub app_icon: Option<String>,
    pub image_data: Option<ImageData>,

    pub sound_file: Option<String>,

    pub is_dirty: bool,

    creation_time: time::Instant,
    expire_timeout: Option<time::Duration>,
    expire_timeout_thread_handle: Option<std::thread::JoinHandle<()>>,

    outputs: HashMap<String, Arc<OutputConfiguration>>,
    surface_ids: Vec<SurfaceID>,
}

impl Notification {
    pub fn new(app_name: String, summary: String, body: String) -> Notification {
        Notification {
            id: None,
            app_name,
            summary,
            body,
            urgency: NotificationUrgency::Normal,
            app_icon: None,
            image_data: None,
            sound_file: None,
            is_dirty: true,
            creation_time: time::Instant::now(),
            expire_timeout: None,
            expire_timeout_thread_handle: None,
            outputs: HashMap::new(),
            surface_ids: Vec::new(),
        }
    }

    pub fn try_make_surfaces(&mut self, configuration: &Configuration) -> Option<()> {
        {
            let r_wayland_state = wayland_state_read(None);
            let available_output_names = r_wayland_state.get_output_names();

            for output_name in available_output_names {
                let output_configuration = configuration.get_output_configuration(output_name);
                if output_configuration.enabled.unwrap_or(false) {
                    self.try_add_output(output_name, output_configuration);
                }
            }
        }

        let mut w_wayland_state = wayland_state_write(None);
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

                    let config = spec.get_by_urgency(&self.urgency.to_string());
                    if let Some(layer) = config.layer {
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
            EventResponse::ExecuteCommand(command_string) => {
                use std::process::Command;

                let (command, args_string) = command_string.split_once(' ').unwrap();
                let args = args_string.split_whitespace();

                if let Err(error) = Command::new(command).args(args).spawn() {
                    eprintln!(
                        "Failed to execute command '{}' with args '{}': {}",
                        command, args_string, error
                    );
                }
            }
            EventResponse::PlaySound(sound_command) => {
                use std::path::Path;
                use std::process::Command;

                match self.sound_file.as_ref() {
                    Some(sound_file_path) => {
                        let sound_file_exists = Path::new(sound_file_path).exists();

                        if !sound_file_exists {
                            eprintln!(
                                "Sound file '{}' does not exist, but an event to play it was triggered.",
                                sound_file_path
                            );
                            return true;
                        }

                        let (command, extra_args_string) =
                            sound_command.split_once(' ').unwrap_or((sound_command, ""));
                        let extra_args = extra_args_string.split_whitespace();

                        if let Err(error) = Command::new(command)
                            .args(extra_args)
                            .arg(sound_file_path)
                            .spawn()
                        {
                            eprintln!(
                                "Failed to execute command '{} {}' with arg '{}': {}",
                                sound_command, extra_args_string, sound_file_path, error
                            );
                        }
                    }
                    None => return true,
                }
            }
            _ => {
                return false;
            }
        };

        true
    }

    pub fn has_surface(&self, id: &SurfaceID) -> bool {
        self.surface_ids.contains(id)
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

    pub fn get_output_spec(&self, output_name: &String) -> Option<&OutputConfiguration> {
        self.outputs
            .get(output_name)
            .map(|c| unsafe { &*Arc::as_ptr(c) })
    }

    pub fn has_timed_out(&self) -> bool {
        match self.expire_timeout {
            Some(_) => {
                if let Some(thread_handle) = self.expire_timeout_thread_handle.as_ref() {
                    thread_handle.is_finished()
                } else {
                    false
                }
            }
            None => false,
        }
    }

    pub(crate) fn set_timeout(&mut self, mut timeout: std::time::Duration) {
        // TODO: Take handle into old thread (if it exists) and interrupt it.

        self.expire_timeout = Some(timeout);

        if timeout == std::time::Duration::MAX {
            // TODO: Use idle status to persist notifications when idling.
            timeout = std::time::Duration::from_millis(5000);
        }

        // Critical notifications should not automatically expire, as they are things that the user will most likely want to know about.
        if self.urgency == NotificationUrgency::Critical {
            return;
        }

        let id = self.id;
        self.expire_timeout_thread_handle = Some(std::thread::spawn(move || {
            std::thread::sleep(timeout);

            if let Some(id) = id {
                let mut manager = notification_manager_write(None);
                let _ = manager.close_notification(id, NotificationCloseReason::Expired);
            }
        }));
    }

    fn expire(&mut self) {
        let ids = self.surface_ids.clone();
        for id in ids.iter() {
            self.delete_surface(id);
        }
    }

    fn delete_surface(&mut self, surface_id: &SurfaceID) {
        self.surface_ids.retain(|id| id != surface_id);

        let mut wayland_state = wayland_state_write(None);
        wayland_state.mark_surface_for_destruction(surface_id);
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

#[derive(Debug)]
pub enum NotificationError {
    InvalidID(u32),
}

impl std::fmt::Display for NotificationError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            Self::InvalidID(id) => write!(fmt, "Invalid ID ({})", id),
        }
    }
}

/// https://specifications.freedesktop.org/notification/latest/protocol.html#id-1.10.4.2.4
#[derive(Clone, Copy)]
pub enum NotificationCloseReason {
    Expired = 1,
    Dismissed = 2,
    Requested = 3,
    Undefined = 4,
}

pub static NOTIFICATION_MANAGER: RwLock<NotificationManager> =
    RwLock::new(NotificationManager::new());

use crate::generate_rw_accessors;
generate_rw_accessors!(NOTIFICATION_MANAGER NOTIFICATION_WRITE_BACKTRACE notification_manager_read notification_manager_write notification_manager_panic NotificationManager);

pub struct NotificationManager {
    biggest_id_given: u32,

    active_configuration: Option<Configuration>,
    active_notifications: LazyLock<BTreeMap<u32, Notification>>,

    event_triggers_to_process: LazyLock<HashMap<u32, Vec<EventTrigger>>>,
    event_handler: LazyLock<HashMap<EventTrigger, EventResponse>>,

    notify_change_handler: Option<Arc<(Mutex<bool>, Condvar)>>,

    inactive_uncommited_notification_ids: Vec<(u32, NotificationCloseReason)>,
}

impl NotificationManager {
    const fn new() -> NotificationManager {
        NotificationManager {
            biggest_id_given: 0,
            active_configuration: None,
            active_notifications: LazyLock::new(|| BTreeMap::new()),
            event_triggers_to_process: LazyLock::new(|| HashMap::new()),
            event_handler: LazyLock::new(|| HashMap::new()),
            notify_change_handler: None,
            inactive_uncommited_notification_ids: Vec::new(),
        }
    }

    pub fn set_configuration(&mut self, configuration: Configuration) {
        self.active_configuration = Some(configuration);
    }

    pub fn get_configuration(&mut self) -> Option<&Configuration> {
        self.active_configuration.as_ref()
    }

    pub fn set_notify_change_handler(&mut self, handler: Arc<(Mutex<bool>, Condvar)>) {
        self.notify_change_handler = Some(handler);
    }

    pub fn add_notification(&mut self, mut notification: Notification) -> u32 {
        self.biggest_id_given = self.biggest_id_given.wrapping_add(1);
        if self.biggest_id_given == 0 {
            // From the dbus notification documentation:
            //   Servers must make sure not to return zero as an ID.
            self.biggest_id_given += 1;
        }

        notification.try_handle_event(
            self.event_handler
                .get(&EventTrigger::OnNotificationReceived)
                .unwrap_or(&EventResponse::default()),
        );

        notification.id = Some(self.biggest_id_given);

        self.active_notifications
            .insert(self.biggest_id_given, notification);

        if let Some(handler) = &self.notify_change_handler {
            handler.1.notify_all();
        }

        self.biggest_id_given
    }

    pub fn replace_notification(&mut self, id: u32, mut notification: Notification) {
        // Move this out of zero so that 'has_had_any_notification' works properly.
        if self.biggest_id_given == 0 {
            self.biggest_id_given += 1;
        }

        // Expire old notification if it exists.
        let _ = self.expire_notification(&id);

        notification.id = Some(self.biggest_id_given);
        self.active_notifications.insert(id, notification);
    }

    pub fn set_timeout_for_notification(&mut self, id: &u32, timeout: std::time::Duration) {
        match self.active_notifications.get_mut(id) {
            Some(notification) => notification.set_timeout(timeout),
            None => eprintln!("Failed to retrieve notification with id {}.", id),
        }
    }

    pub fn close_notification(
        &mut self,
        id: u32,
        reason: NotificationCloseReason,
    ) -> Result<(), NotificationError> {
        self.expire_notification(&id)?;

        self.inactive_uncommited_notification_ids.push((id, reason));

        if let Some(handler) = &self.notify_change_handler {
            handler.1.notify_all();
        }

        Ok(())
    }

    pub fn has_had_any_notification(&self) -> bool {
        // It will only increment when adding a new notification.
        self.biggest_id_given != 0
    }

    pub fn has_any_active_notification(&self) -> bool {
        !self.active_notifications.is_empty()
    }

    pub fn set_event_handler(&mut self, event_handler: HashMap<EventTrigger, EventResponse>) {
        *self.event_handler = event_handler;
    }

    pub fn add_event_triggers(&mut self, triggers: &mut HashMap<SurfaceID, Vec<EventTrigger>>) {
        for (surface_id, event_triggers) in triggers.drain() {
            for (notification_id, notification) in self.active_notifications.iter() {
                if !notification.has_surface(&surface_id) {
                    continue;
                }

                if self.event_triggers_to_process.contains_key(notification_id) {
                    self.event_triggers_to_process
                        .get_mut(notification_id)
                        .unwrap()
                        .extend(event_triggers);
                } else {
                    self.event_triggers_to_process
                        .insert(notification_id.clone(), event_triggers);
                }

                break;
            }
        }
    }

    pub fn process_active_notifications<F>(
        &mut self,
        process_surface: &mut F,
    ) -> Vec<(u32, NotificationCloseReason)>
    where
        F: FnMut(&SurfaceID, &Notification, &mut HashMap<String, i32>) -> SurfaceProcessingOutput,
    {
        let mut offset_per_output = HashMap::<String, i32>::new();
        let mut recentry_inactived_notifications = Vec::new();

        for (id, notification) in self.active_notifications.iter_mut() {
            let mut surfaces_to_delete = Vec::<SurfaceID>::new();

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

            for surface_id in surfaces_to_delete.iter() {
                notification.delete_surface(&surface_id);
            }

            if !notification.has_any_surface() {
                let surfaces_previously_deleted = surfaces_to_delete.is_empty();

                recentry_inactived_notifications.push((
                    *id,
                    if surfaces_previously_deleted {
                        NotificationCloseReason::Dismissed
                    } else {
                        NotificationCloseReason::Undefined
                    },
                ));
            } else {
                notification.is_dirty = false;
            }
        }

        for (id, event_triggers) in self.event_triggers_to_process.drain() {
            if let Some(notification) = self.active_notifications.get_mut(&id) {
                for trigger in event_triggers {
                    notification.try_handle_event(
                        self.event_handler
                            .get(&trigger)
                            .unwrap_or(&EventResponse::default()),
                    );
                }
            }
        }

        for (id, _) in recentry_inactived_notifications.iter() {
            if let Err(error) = self.expire_notification(id) {
                eprintln!("Failed to set notification as expired: {}", error);
            }
        }

        let mut ret_value = self.inactive_uncommited_notification_ids.clone();
        self.inactive_uncommited_notification_ids.clear();
        ret_value.extend(&recentry_inactived_notifications);

        ret_value
    }

    fn expire_notification(&mut self, id: &u32) -> Result<(), NotificationError> {
        let mut notification = self
            .active_notifications
            .remove(id)
            .ok_or(NotificationError::InvalidID(*id))?;

        notification.try_handle_event(
            self.event_handler
                .get(&EventTrigger::OnNotificationClosed)
                .unwrap_or(&EventResponse::default()),
        );

        notification.expire();

        if cfg!(debug_assertions) {
            println!(
                "Expired notification with id '{}' after {}ms have elapsed.",
                id,
                notification.creation_time.elapsed().as_millis()
            );
        }

        Ok(())
    }
}
