use std::error::Error;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use dbus::Message;
use dbus::arg::{PropMap, RefArg};
use dbus::blocking::Connection;
use dbus_crossroads::{Context, Crossroads, MethodErr};

use crate::notification::{
    Notification, NotificationCloseReason, NotificationError, notification_manager_write,
};

macro_rules! create_struct_tuple_pair {
    ($visibility:vis $struct_name:ident $type_name:ident $( $field_name:ident:$field_type:ty ) +) => {
        #[derive(Debug)]
        $visibility struct $struct_name {
            $(
                $visibility $field_name: $field_type,
            )*
        }

        $visibility type $type_name = ($( $field_type, )*);

        impl From<$type_name> for $struct_name {
            fn from(input: $type_name) -> $struct_name {
                let ( $($field_name,)+ ) = input;

                $struct_name {
                    $(
                        $field_name,
                    )*
                }
            }
        }

        impl From<$struct_name> for $type_name {
            fn from(input: $struct_name) -> $type_name {
                ( $( input.$field_name, )+ )
            }
        }
    }
}

macro_rules! create_dbus_wrapper {
    (@as_str_ref $id:ident) => {&'static str};

    ($struct_name:ident $type_name:ident $parameter_list_name:ident $( $field_name:ident:$field_type:ty ) +) => {
        create_struct_tuple_pair!($struct_name $type_name $( $field_name:$field_type )+);

        const $parameter_list_name: ( $( create_dbus_wrapper!(@as_str_ref $field_name), )+ ) = ( $( stringify!($field_name), )+ );
    };
}

create_dbus_wrapper!(NotifyMessageInput NotifyMessageInputType DBUS_NOTIFY_PARAMETERS app_name:String replaces_id:u32 app_icon:String summary:String body:String actions:Vec<String> hints:PropMap expire_timeout:i32);
create_dbus_wrapper!(ServerInfoMessageOutput ServerInfoMessageOutputType DBUS_SERVER_INFO_PARAMETERS name:String vendor:String version:String spec_version:String);

const CAPABILITIES: [&'static str; 4] = ["body", "body-markup", "icon-static", "sound"];

pub const NOTIFICATION_BUS_INTERFACE_NAME: &'static str = "org.freedesktop.Notifications";
pub const NOTIFICATION_BUS_OBJECT_PATH: &'static str = "/org/freedesktop/Notifications";

const SERVER_NAME: &'static str = env!("CARGO_PKG_NAME");
const SERVER_VENDOR: &'static str = "Sofia & Bell";
const SERVER_VERSION: &'static str = env!("CARGO_PKG_VERSION");
const SERVER_SPEC_VERSION: &'static str = "1.3";

// https://specifications.freedesktop.org/notification/latest/icons-and-images.html#icons-and-images-formats
create_struct_tuple_pair!(pub ImageData ImageDataTuple width:i32 height:i32 rowstride:i32 has_alpha:bool bits_per_sample:i32 channels:i32 data:Vec<u8>);

struct DBusData;

pub fn create_connection(
    process_notification: Arc<(Mutex<bool>, Condvar)>,
) -> Result<Connection, Box<dyn Error>> {
    let connection = Connection::new_session()?;
    connection.request_name(NOTIFICATION_BUS_INTERFACE_NAME, true, true, false)?;

    let mut crossroads = Crossroads::new();

    let iface_token = crossroads.register(NOTIFICATION_BUS_INTERFACE_NAME, |bus| {
        bus.method(
            "GetCapabilities",
            (),
            ("reply",),
            move |_ctx: &mut Context, _data: &mut DBusData, _: ()| {
                let reply = (Vec::from(CAPABILITIES),);
                Ok(reply)
            },
        );

        let notification_sync = Arc::clone(&process_notification);
        bus.method(
            "Notify",
            DBUS_NOTIFY_PARAMETERS,
            ("id",),
            move |ctx: &mut Context, data: &mut DBusData, input| {
                let (_, condition) = notification_sync.as_ref();

                let reply = (handle_notify_message(ctx, data, input).unwrap(),);

                condition.notify_all();

                Ok(reply)
            },
        );

        bus.method(
            "GetServerInformation",
            (),
            DBUS_SERVER_INFO_PARAMETERS,
            move |_ctx: &mut Context, _data: &mut DBusData, _: ()| {
                let server_info = ServerInfoMessageOutput {
                    name: SERVER_NAME.to_owned(),
                    vendor: SERVER_VENDOR.to_owned(),
                    version: SERVER_VERSION.to_owned(),
                    spec_version: SERVER_SPEC_VERSION.to_owned(),
                };

                let reply = ServerInfoMessageOutputType::from(server_info);
                Ok(reply)
            },
        );

        let close_notification_sync = Arc::clone(&process_notification);
        bus.method(
            "CloseNotification",
            ("id",),
            (),
            move |_ctx: &mut Context, _data: &mut DBusData, (id,): (u32,)| {
                let mut notification_manager =
                    notification_manager_write(Some(Duration::from_millis(5000)));

                let close_notification_sync = Arc::clone(&close_notification_sync);
                let (_, condition) = close_notification_sync.as_ref();

                match notification_manager
                    .close_notification(id, NotificationCloseReason::Requested)
                {
                    Ok(_) => {
                        condition.notify_all();

                        Ok(())
                    }
                    Err(NotificationError::InvalidID(id)) => Err(MethodErr::invalid_arg(&id)),
                }
            },
        );
    });

    crossroads.insert(NOTIFICATION_BUS_OBJECT_PATH, &[iface_token], DBusData {});

    use dbus::channel::MatchingReceiver;
    use dbus::message::MatchRule;
    connection.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |msg, conn| crossroads.handle_message(msg, conn).is_ok()),
    );

    Ok(connection)
}

pub fn emit_notification_closed(
    connection: &mut Connection,
    id: u32,
    reason: u32,
) -> Result<u32, ()> {
    let mut message = Message::new_signal(
        NOTIFICATION_BUS_OBJECT_PATH,
        NOTIFICATION_BUS_INTERFACE_NAME,
        "NotificationClosed",
    )
    .unwrap();

    message.append_all((id, reason as u32));

    connection.channel().send(message)
}

// Ref.: https://specifications.freedesktop.org/notification/latest/protocol.html#id-1.10.3.3.4
fn handle_notify_message(
    _ctx: &mut Context,
    _data: &mut DBusData,
    input: NotifyMessageInputType,
) -> Result<u32, MethodErr> {
    let input = NotifyMessageInput::from(input);

    let mut notification = Notification::new(input.app_name, input.summary, input.body);

    // The optional program icon of the calling application.
    // Can be an empty string, indicating no icon.
    if !input.app_icon.is_empty() {
        notification.app_icon = Some(input.app_icon);
    }

    if let Some(image_data) = input.hints.get("image-data") {
        notification.image_data = parse_image_data_struct(&image_data.0);
    } else if let Some(_image_path) = input.hints.get("image-path") {
        todo!();
    } else if let Some(icon_data) = input.hints.get("icon_data") {
        notification.image_data = parse_image_data_struct(&icon_data.0);
    }

    // The timeout time in milliseconds since the display of the notification at which
    // the notification should automatically close.
    let expire_timeout = {
        if input.expire_timeout < 0 {
            // If -1, the notification's expiration time is dependent on the notification server's settings,
            // and may vary for the type of notification.
            Some(std::time::Duration::MAX)
        } else {
            let timeout = input.expire_timeout as u64;
            if timeout == 0 {
                //  If 0, never expire.
                None
            } else {
                Some(std::time::Duration::from_millis(timeout))
            }
        }
    };

    // Notifications have an urgency level associated with them. This defines the importance of the notification.
    let urgency_level = {
        if let Some(urgency_variant) = input.hints.get("urgency") {
            *urgency_variant.0.as_any().downcast_ref::<u8>().unwrap()
        } else {
            1u8
        }
    };

    notification.urgency = urgency_level.into();

    let mut notification_manager = notification_manager_write(None);
    let configuration = notification_manager
        .get_configuration()
        .ok_or(MethodErr::failed(
            "Failed to retrieve configuration from notification manager.",
        ))?;

    // Causes the server to suppress playing any sounds, if it has that ability.
    let suppress_sound: bool = {
        if let Some(suppress_hint) = input.hints.get("suppress-sound") {
            *suppress_hint
                .0
                .as_any()
                .downcast_ref::<bool>()
                .unwrap_or(&false)
        } else {
            false
        }
    };

    if !suppress_sound {
        // The path to a sound file to play when the notification pops up.
        let sound_file = match input.hints.get("sound-file") {
            Some(file_path) => file_path.as_str().map(|s| s.to_owned()),
            // A themeable named sound from the freedesktop.org sound naming specification
            None => match input.hints.get("sound-name") {
                // FIXME: Actually search for a valid sound theme on the system.
                Some(sound_name) => Some(format!(
                    "/usr/share/sounds/freedesktop/stereo/{}.oga",
                    sound_name.as_str().unwrap_or_default()
                )),
                None => Some(configuration.default_sound.clone()),
            },
        };

        notification.sound_file = sound_file;
    }

    notification
        .try_make_surfaces(configuration)
        .ok_or(MethodErr::failed(
            "Failed creating Wayland notification surfaces.",
        ))?;

    // The optional notification ID that this notification replaces.
    // The server must atomically (ie with no flicker or other visual cues) replace
    // the given notification with this one.
    let mut id = input.replaces_id;
    if id == 0 {
        // If replaces_id is 0, the return value is a UINT32 that represent the notification.
        id = notification_manager.add_notification(notification);
    } else {
        // If replaces_id is not 0, the returned value is the same value as replaces_id.
        notification_manager.replace_notification(id, notification);
    }

    if let Some(timeout) = expire_timeout {
        notification_manager.set_timeout_for_notification(&id, timeout);
    }

    Ok(id)
}

macro_rules! extract_field {
    ($source:ident $index:literal $type:ty ) => {
        *$source
            .as_static_inner($index)?
            .as_any()
            .downcast_ref::<$type>()?
    };
}

fn parse_image_data_struct(raw_data: &Box<dyn RefArg>) -> Option<ImageData> {
    let width = extract_field!(raw_data 0 i32);
    let height = extract_field!(raw_data 1 i32);
    let rowstride = extract_field!(raw_data 2 i32);
    let has_alpha = extract_field!(raw_data 3 bool);
    let bits_per_sample = extract_field!(raw_data 4 i32);
    let channels = extract_field!(raw_data 5 i32);

    let data = raw_data
        .as_static_inner(6)?
        .as_iter()?
        .map(|arg| arg.as_i64().unwrap_or(0) as u8)
        .collect();

    let parsed = ImageData {
        width,
        height,
        rowstride,
        has_alpha,
        bits_per_sample,
        channels,
        data,
    };

    Some(parsed)
}
