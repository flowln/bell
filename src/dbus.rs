use std::error::Error;

use dbus::arg::PropMap;
use dbus::blocking::Connection;
use dbus_crossroads::{Context, Crossroads, MethodErr};

use crate::notification::{Notification, notification_manager_write};

#[macro_export]
macro_rules! create_dbus_wrapper {
    (@as_str_ref $id:ident) => {&'static str};

    ($struct_name:ident $type_name:ident $parameter_list_name:ident $( $field_name:ident:$field_type:ty ) +) => {
        struct $struct_name {
            $(
                $field_name: $field_type,
            )*
        }

        type $type_name = ($( $field_type, )*);

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

        const $parameter_list_name: ( $( create_dbus_wrapper!(@as_str_ref $field_name), )+ ) = ( $( stringify!($field_name), )+ );
    };
}

create_dbus_wrapper!(NotifyMessageInput NotifyMessageInputType DBUS_NOTIFY_PARAMETERS app_name:String replaces_id:u32 app_icon:String summary:String body:String actions:Vec<String> hints:PropMap expire_timeout:i32);
create_dbus_wrapper!(ServerInfoMessageOutput ServerInfoMessageOutputType DBUS_SERVER_INFO_PARAMETERS name:String vendor:String version:String spec_version:String);

const CAPABILITIES: [&'static str; 1] = ["body"];

const NOTIFICATION_BUS_NAME: &'static str = "org.freedesktop.Notifications";
const NOTIFICATION_BUS_OBJECT_PATH: &'static str = "/org/freedesktop/Notifications";

const SERVER_NAME: &'static str = env!("CARGO_PKG_NAME");
const SERVER_VENDOR: &'static str = "Sofia & Bell";
const SERVER_VERSION: &'static str = env!("CARGO_PKG_VERSION");
const SERVER_SPEC_VERSION: &'static str = "1.3";

struct DBusData;

pub fn create_server() -> Result<Connection, Box<dyn Error>> {
    let connection = Connection::new_session()?;
    connection.request_name(NOTIFICATION_BUS_NAME, true, true, false)?;

    let mut crossroads = Crossroads::new();

    let iface_token = crossroads.register(NOTIFICATION_BUS_NAME, |bus| {
        bus.method(
            "GetCapabilities",
            (),
            ("reply",),
            move |_ctx: &mut Context, _data: &mut DBusData, _: ()| {
                let reply = (Vec::from(CAPABILITIES),);
                Ok(reply)
            },
        );

        bus.method(
            "Notify",
            DBUS_NOTIFY_PARAMETERS,
            ("id",),
            move |ctx: &mut Context, data: &mut DBusData, input| {
                let reply = (handle_notify_message(ctx, data, input).unwrap(),);
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

fn handle_notify_message(
    _ctx: &mut Context,
    _data: &mut DBusData,
    input: NotifyMessageInputType,
) -> Result<u32, MethodErr> {
    let input = NotifyMessageInput::from(input);

    let body = if input.body.len() != 0 { Some(input.body) } else { None };
    let mut notification = Notification::new(input.app_name, input.summary, body);

    let mut notification_manager = notification_manager_write();
    let configuration = notification_manager
        .get_configuration()
        .ok_or(MethodErr::failed(
            "Failed to retrieve configuration from notification manager.",
        ))?;
    notification
        .try_make_surfaces(configuration)
        .ok_or(MethodErr::failed(
            "Failed creating Wayland notification surfaces.",
        ))?;

    let mut id = input.replaces_id;
    if id == 0 {
        id = notification_manager.add_notification(notification);
    } else {
        notification_manager.replace_notification(id, notification);
    }

    Ok(id)
}
