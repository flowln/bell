use dbus::arg::PropMap;


pub const NOTIFICATION_BUS_INTERFACE_NAME: &'static str = "org.freedesktop.Notifications";
pub const NOTIFICATION_BUS_OBJECT_PATH: &'static str = "/org/freedesktop/Notifications";


macro_rules! create_struct_tuple_pair {
    ($visibility:vis $struct_name:ident $type_name:ident $( $field_name:ident:$field_type:ty ) +) => {
        #[derive(Debug)]
        $visibility struct $struct_name {
            $(
                $visibility $field_name: $field_type,
            )*
        }

        $visibility type $type_name = ($( $field_type, )*);

        impl std::fmt::Display for $struct_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                $(
                    writeln!(f, "  {}: {:#?}", stringify!($field_name), self.$field_name)?;
                )+

                Ok(())
            }
        }

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

    ($visibility:vis $struct_name:ident $type_name:ident $( $field_name:ident:$field_type:ty ) +) => {
        create_struct_tuple_pair!($visibility $struct_name $type_name $( $field_name:$field_type )+);
    };
}

macro_rules! create_dbus_signal_wrapper {
    ($visibility:vis $struct_name:ident $signal_name:literal $( $field_name:ident:$field_type:ty ) +) => {
        #[derive(Debug)]
        $visibility struct $struct_name {
            $(
                $visibility $field_name: $field_type,
            )*
        }

        impl dbus::arg::AppendAll for $struct_name {
            fn append(&self, i: &mut dbus::arg::IterAppend) {
                dbus::arg::RefArg::append(&self.id, i);
                dbus::arg::RefArg::append(&self.reason, i);
            }
        }

        impl dbus::arg::ReadAll for $struct_name {
            fn read(i: &mut dbus::arg::Iter) -> Result<Self, dbus::arg::TypeMismatchError> {
                Ok($struct_name {
                    id: i.read()?,
                    reason: i.read()?,
                })
            }
        }

        impl dbus::message::SignalArgs for $struct_name {
            const NAME: &'static str = $signal_name;
            const INTERFACE: &'static str = NOTIFICATION_BUS_INTERFACE_NAME;
        }
    };
}

create_dbus_wrapper!(pub ServerInfoMessageOutput ServerInfoMessageOutputType name:String vendor:String version:String spec_version:String);
create_dbus_wrapper!(pub ServerCapsMessageOutput ServerCapsMessageOutputType capabilities:Vec<String>);

create_dbus_wrapper!(pub NotifyMessageInput NotifyMessageInputType app_name:String replaces_id:u32 app_icon:String summary:String body:String actions:Vec<String> hints:PropMap expire_timeout:i32);
create_dbus_wrapper!(pub NotifyMessageOutput NotifyMessageOutputType id:u32);

create_dbus_wrapper!(pub CloseNotificationInput CloseNotificationInputType id:u32);

create_dbus_signal_wrapper!(pub NotificationClosedSignal "NotificationClosed" id:u32 reason:u32);

